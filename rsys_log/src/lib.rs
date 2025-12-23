// SUBSYSTEM: common-types-logging
//! LCARS-стиль форматирования логов для R-SYS.
//! Генерирует компактные строки вида:
//! `2025.1205.10:15:30|SSYS=db|CTRL=migrator|LVL=INFO|CID=abc123|MSG=Migration applied|name:2025-12-01-001-rbac dur_ms:214`
/*
## МИССИЯ ФАЙЛА
- Краткая цель: Форматировать строки логов в LCARS-стиле и управлять их выводом.
- Роль в системе: Утилита-форматтер/диспетчер логов для всех сервисов.

## ПОЛОЖЕНИЕ В СИСТЕМЕ
- Крейt (crate): rsys_log
- Модуль (module): rsys_log::lib
- Связанные файлы/модули: rsys_log/src/colorscheme.rs, rsys_log/src/bin/demo.rs, rsys_log/README.md

## ВНЕШНИЕ ЗАВИСИМОСТИ
- Crates: chrono
- Внутренние модули: colorscheme

## ПУБЛИЧНЫЙ ИНТЕРФЕЙС (API)
- Структуры (struct): LogBuilder — конструктор LCARS-строк.
- Перечисления (enum): Level — уровни логов TRACE..ERROR.
- Функции/методы:
  - set_global_level: `fn set_global_level(level: Level)` — установить глобальный уровень.
  - global_level: `fn global_level() -> Level` — получить текущий глобальный уровень.
  - set_color_scheme: `fn set_color_scheme(resolver: ColorResolver)` — настроить палитру.
  - subscribe_logs: `fn subscribe_logs() -> Receiver<String>` — подписка на поток готовых строк.
  - log_line: `fn log_line(builder: LogBuilder) -> Option<String>` — собрать строку и распространить.

## АЛГОРИТМЫ И ПОТОКИ ДАННЫХ
- Строит базовую строку с датой/полями, добавляет пары key:value, при необходимости красит ANSI.
- Рассылает готовую строку всем подписчикам через mpsc, очищая закрытые каналы.

## ВЗАИМОДЕЙСТВИЯ
- Каналы/очереди: std::sync::mpsc канал для подписчиков логов (рассылка строк).
- Сеть/файлы: не используются; вывод осуществляется вызовом println вне библиотеки.

## ТЕСТЫ
- Модульные: проверяют сборку строк, многострочные детали, парсинг уровня и доставку в канал.

## ПРИМЕРЫ ИСПОЛЬЗОВАНИЯ
- Базовый лог: LogBuilder::new(...).data(...).print()
- Подписка: subscribe_logs() -> Receiver, чтение новых строк.

## ИСТОРИЯ ИЗМЕНЕНИЙ
- Stardate 2025.1207: Добавлен канал подписки на лог-строки.
*/

mod colorscheme;
use chrono::{DateTime, Utc};
use colorscheme::ColorResolver;

/// Уровень логирования.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Level {
    Trace,
    Debug,
    Info,
    Warn,
    Error,
}

impl Level {
    /// Представление уровня в виде строки (`TRACE|DEBUG|INFO|WARN|ERROR`).
    fn as_str(&self) -> &'static str {
        match self {
            Level::Trace => "TRACE",
            Level::Debug => "DEBUG",
            Level::Info => "INFO",
            Level::Warn => "WARN",
            Level::Error => "ERROR",
        }
    }

    /// Проверка, должен ли лог с указанным уровнем выводиться при установленном уровне `current`.
    fn enabled(self, current: Level) -> bool {
        (self as u8) >= (current as u8)
    }

    /// Разбор уровня из строки (`trace|debug|info|warn|error`, регистронезависимо).
    pub fn from_str(raw: &str) -> Option<Self> {
        match raw.to_ascii_lowercase().as_str() {
            "trace" => Some(Level::Trace),
            "debug" => Some(Level::Debug),
            "info" => Some(Level::Info),
            "warn" | "warning" => Some(Level::Warn),
            "error" => Some(Level::Error),
            _ => None,
        }
    }
}

/// Глобальный уровень логирования (потокобезопасный).
static GLOBAL_LEVEL: std::sync::OnceLock<std::sync::atomic::AtomicU8> = std::sync::OnceLock::new();
static COLOR_SCHEME: std::sync::OnceLock<std::sync::RwLock<ColorResolver>> =
    std::sync::OnceLock::new();
static LOG_CHANNELS: std::sync::OnceLock<std::sync::Mutex<Vec<std::sync::mpsc::Sender<String>>>> =
    std::sync::OnceLock::new();

/// Ячейка с глобальным уровнем логирования.
fn global_level_cell() -> &'static std::sync::atomic::AtomicU8 {
    GLOBAL_LEVEL.get_or_init(|| std::sync::atomic::AtomicU8::new(Level::Info as u8))
}

/// Ячейка с цветовой схемой.
fn color_scheme_cell() -> &'static std::sync::RwLock<ColorResolver> {
    COLOR_SCHEME.get_or_init(|| std::sync::RwLock::new(colorscheme::gruvbox_dark))
}

/// Ячейка со списком подписчиков логов.
fn log_channels_cell() -> &'static std::sync::Mutex<Vec<std::sync::mpsc::Sender<String>>> {
    LOG_CHANNELS.get_or_init(|| std::sync::Mutex::new(Vec::new()))
}

/// Установить глобальный уровень логирования.
pub fn set_global_level(level: Level) {
    global_level_cell().store(level as u8, std::sync::atomic::Ordering::Relaxed);
}

/// Получить текущий глобальный уровень логирования.
pub fn global_level() -> Level {
    match global_level_cell().load(std::sync::atomic::Ordering::Relaxed) {
        0 => Level::Trace,
        1 => Level::Debug,
        2 => Level::Info,
        3 => Level::Warn,
        _ => Level::Error,
    }
}

/// Установить кастомную цветовую тему (по умолчанию Gruvbox Dark).
pub fn set_color_scheme(resolver: ColorResolver) {
    match color_scheme_cell().write() {
        Ok(mut guard) => *guard = resolver,
        Err(poisoned) => *poisoned.into_inner() = resolver,
    }
}

/// Подписаться на поток логов. Возвращает `Receiver`, из которого можно читать строки по мере появления.
pub fn subscribe_logs() -> std::sync::mpsc::Receiver<String> {
    let (sender, receiver) = std::sync::mpsc::channel();
    match log_channels_cell().lock() {
        Ok(mut guard) => guard.push(sender),
        Err(poisoned) => poisoned.into_inner().push(sender),
    }
    receiver
}

/// Builder LCARS-строки лога.
#[derive(Debug, Clone)]
pub struct LogBuilder {
    timestamp: Option<DateTime<Utc>>,
    subsystem: String,
    controller: String,
    level: Level,
    cid: String,
    msg: String,
    data: Vec<(String, String)>,
    details: Vec<String>,
    colorize: bool,
}

impl LogBuilder {
    /// Создать builder только с сообщением (остальные поля можно установить сеттерами).
    pub fn msg(msg: impl Into<String>) -> Self {
        Self::new("-", "-", Level::Info, msg)
    }

    /// Создать builder c обязательными полями.
    pub fn new(
        subsystem: impl Into<String>,
        controller: impl Into<String>,
        level: Level,
        msg: impl Into<String>,
    ) -> Self {
        Self {
            timestamp: None,
            subsystem: subsystem.into(),
            controller: controller.into(),
            level,
            cid: "-".to_string(),
            msg: msg.into(),
            data: Vec::new(),
            details: Vec::new(),
            colorize: true,
        }
    }

    /// Установить подсистему.
    pub fn ssys(mut self, subsystem: impl Into<String>) -> Self {
        self.subsystem = subsystem.into();
        self
    }

    /// Установить контроллер/модуль.
    pub fn ctrl(mut self, controller: impl Into<String>) -> Self {
        self.controller = controller.into();
        self
    }

    /// Установить уровень INFO.
    pub fn info(mut self) -> Self {
        self.level = Level::Info;
        self
    }

    /// Установить уровень WARN.
    pub fn warn(mut self) -> Self {
        self.level = Level::Warn;
        self
    }

    /// Установить уровень ERROR.
    pub fn error(mut self) -> Self {
        self.level = Level::Error;
        self
    }

    /// Установить уровень DEBUG.
    pub fn debug(mut self) -> Self {
        self.level = Level::Debug;
        self
    }

    /// Установить уровень TRACE.
    pub fn trace(mut self) -> Self {
        self.level = Level::Trace;
        self
    }

    /// Установить correlation id (`CID`).
    pub fn cid(mut self, cid: impl Into<String>) -> Self {
        self.cid = cid.into();
        self
    }

    /// Добавить ключ-значение в дополнительную информацию.
    pub fn data(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.data.push((key.into(), value.into()));
        self
    }

    /// Добавить многострочную деталь (используется для ошибок).
    pub fn detail(mut self, line: impl Into<String>) -> Self {
        self.details.push(line.into());
        self
    }

    /// Задать timestamp (UTC). Если не задан, возьмётся текущее время.
    pub fn timestamp(mut self, ts: DateTime<Utc>) -> Self {
        self.timestamp = Some(ts);
        self
    }

    /// Включить/выключить цветовое форматирование ANSI.
    pub fn colorize(mut self, enabled: bool) -> Self {
        self.colorize = enabled;
        self
    }

    /// Собрать строку лога. Если есть `details`, многострочная строка соединяется через `\n`.
    pub fn build(self) -> String {
        self.build_lines().join("\n")
    }

    /// Собрать строки (первая и дополнительные `> ...`).
    pub fn build_lines(&self) -> Vec<String> {
        // ВХОДНЫЕ ДАННЫЕ: фиксируем timestamp и базовые поля LCARS.
        let ts = self
            .timestamp
            .unwrap_or_else(|| Utc::now())
            .format("%Y.%m%d.%H:%M:%S")
            .to_string();

        let mut base = format!(
            "{}|SSYS={}|CTRL={}|LVL={}|CID={}|MSG={}",
            ts,
            self.subsystem,
            self.controller,
            self.level.as_str(),
            self.cid,
            self.msg
        );

        if !self.data.is_empty() {
            // ОСНОВНАЯ ЛОГИКА: дополняем key:value блок.
            let extra = self
                .data
                .iter()
                .map(|(k, v)| format!("{}:{}", k, v))
                .collect::<Vec<_>>()
                .join(" ");
            base.push('|');
            base.push_str(&extra);
        }

        if self.colorize {
            // ВСПОМОГАТЕЛЬНЫЕ ОПЕРАЦИИ: раскрашиваем строку.
            base = apply_colors(&base, self.level);
        }

        if self.details.is_empty() {
            vec![base]
        } else {
            // ВЫХОДНЫЕ ДАННЫЕ: добавляем многострочные детали.
            let mut lines = Vec::with_capacity(1 + self.details.len());
            lines.push(base);
            for d in &self.details {
                lines.push(format!("  > {}", d));
            }
            lines
        }
    }

    /// Вывести лог в stdout с учётом глобального уровня.
    /// Возвращает `None`, если уровень ниже установленного глобально.
    pub fn print(self) -> Option<()> {
        if let Some(line) = log_line(self) {
            for l in line.split('\n') {
                println!("{}", l);
            }
            Some(())
        } else {
            None
        }
    }
}

/// Построить лог с учётом глобального уровня.
/// Возвращает `None`, если уровень сообщения ниже установленного глобально.
pub fn log_line(builder: LogBuilder) -> Option<String> {
    if builder.level.enabled(global_level()) {
        // ОСНОВНАЯ ЛОГИКА: строим строку и отправляем подписчикам.
        let line = builder.build();
        broadcast_log_line(&line);
        Some(line)
    } else {
        None
    }
}

/// Разослать строку лога всем подписчикам, удаляя закрытые каналы.
fn broadcast_log_line(line: &str) {
    // ОСНОВНАЯ ЛОГИКА: отправляем строку всем подписчикам, удаляя закрытые каналы.
    let mut senders = match log_channels_cell().lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    };
    senders.retain(|sender| sender.send(line.to_string()).is_ok());
}

/// Применить цветовую схему к строке лога.
fn apply_colors(line: &str, level: Level) -> String {
    // ВХОДНЫЕ ДАННЫЕ: выбираем палитру для уровня.
    let resolver = match color_scheme_cell().read() {
        Ok(guard) => *guard,
        Err(poisoned) => *poisoned.into_inner(),
    };
    let palette = resolver(level);
    let (level_color, header_color, context_color, cid_color, msg_color, key_color, value_color) = (
        palette.level,
        palette.header,
        palette.context,
        palette.cid,
        palette.msg,
        palette.key,
        palette.value,
    );

    // Разукрашиваем части по разделителям `|` и ключам.
    // Предполагаем формат: TS|SSYS=...|CTRL=...|LVL=...|CID=...|MSG=...|...
    let mut out = String::with_capacity(line.len() + 16);
    for (idx, part) in line.split('|').enumerate() {
        if idx > 0 {
            out.push('|');
        }
        let colored = if idx == 0 {
            format!("\x1b[{}m{}\x1b[0m", header_color, part)
        } else if part.starts_with("SSYS=") || part.starts_with("CTRL=") {
            format!("\x1b[{}m{}\x1b[0m", context_color, part)
        } else if part.starts_with("LVL=") {
            format!("\x1b[{}m{}\x1b[0m", level_color, part)
        } else if part.starts_with("CID=") {
            format!("\x1b[{}m{}\x1b[0m", cid_color, part)
        } else if part.starts_with("MSG=") {
            format!("\x1b[{}m{}\x1b[0m", msg_color, part)
        } else {
            let colored_data = part
                .split_whitespace()
                .map(|token| {
                    if let Some((k, v)) = token.split_once(':') {
                        format!(
                            "\x1b[{}m{}\x1b[0m:\x1b[{}m{}\x1b[0m",
                            key_color, k, value_color, v
                        )
                    } else {
                        token.to_string()
                    }
                })
                .collect::<Vec<_>>()
                .join(" ");
            colored_data
        };
        out.push_str(&colored);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_basic_line() {
        let ts = DateTime::parse_from_rfc3339("2025-12-05T10:15:30Z")
            .unwrap()
            .with_timezone(&Utc);
        let line = LogBuilder::new("db", "migrator", Level::Info, "Migration applied")
            .timestamp(ts)
            .cid("op12")
            .data("name", "2025-12-01-001-rbac")
            .data("dur_ms", "214")
            .colorize(false)
            .build();
        assert_eq!(
            line,
            "2025.1205.10:15:30|SSYS=db|CTRL=migrator|LVL=INFO|CID=op12|MSG=Migration applied|name:2025-12-01-001-rbac dur_ms:214"
        );
    }

    #[test]
    fn build_with_details() {
        let ts = DateTime::parse_from_rfc3339("2025-12-05T10:15:30Z")
            .unwrap()
            .with_timezone(&Utc);
        let line = LogBuilder::new("auth", "jwt", Level::Error, "JWT verify failed")
            .timestamp(ts)
            .cid("ab7c")
            .data("code", "401")
            .colorize(false)
            .detail("token: eyJhbGciOi...")
            .detail("hint: refresh token")
            .build();
        let expected = "\
2025.1205.10:15:30|SSYS=auth|CTRL=jwt|LVL=ERROR|CID=ab7c|MSG=JWT verify failed|code:401
  > token: eyJhbGciOi...
  > hint: refresh token";
        assert_eq!(line, expected);
    }

    #[test]
    fn broadcasts_log_line_to_subscribers() {
        set_global_level(Level::Info);
        let ts = DateTime::parse_from_rfc3339("2025-12-05T10:15:30Z")
            .unwrap()
            .with_timezone(&Utc);
        let receiver = subscribe_logs();
        let expected_line =
            "2025.1205.10:15:30|SSYS=db|CTRL=db_info|LVL=INFO|CID=123|MSG=Лог для канала";
        let line = log_line(
            LogBuilder::new("db", "db_info", Level::Info, "Лог для канала")
                .timestamp(ts)
                .cid("123")
                .colorize(false),
        )
        .unwrap();
        assert_eq!(line, expected_line);
        let received = receiver
            .recv_timeout(std::time::Duration::from_millis(100))
            .expect("Должны получить строку лога из канала");
        assert_eq!(received, expected_line);
    }

    #[test]
    fn example_chain_from_msg() {
        set_global_level(Level::Info);
        let ts = DateTime::parse_from_rfc3339("2025-12-05T10:15:30Z")
            .unwrap()
            .with_timezone(&Utc);
        let log = LogBuilder::msg("Сообщени лога")
            .ssys("db")
            .ctrl("db_info")
            .cid("123")
            .info()
            .timestamp(ts)
            .colorize(false)
            .build();
        assert_eq!(
            log,
            "2025.1205.10:15:30|SSYS=db|CTRL=db_info|LVL=INFO|CID=123|MSG=Сообщени лога"
        );
    }

    #[test]
    fn colorize_enabled_by_default() {
        set_global_level(Level::Info);
        let ts = DateTime::parse_from_rfc3339("2025-12-05T10:15:30Z")
            .unwrap()
            .with_timezone(&Utc);
        let line = LogBuilder::new("db", "demo", Level::Warn, "Цветная строка")
            .timestamp(ts)
            .build();
        assert!(
            line.contains("\u{1b}[38;5;246m2025.1205.10:15:30\u{1b}[0m"),
            "ожидается ANSI раскраска timestamp"
        );
        assert!(
            line.contains("LVL=WARNING") || line.contains("LVL=WARN"),
            "уровень должен присутствовать"
        );
    }

    #[test]
    fn parses_level_from_str() {
        assert_eq!(Level::from_str("trace"), Some(Level::Trace));
        assert_eq!(Level::from_str("DEBUG"), Some(Level::Debug));
        assert_eq!(Level::from_str("Info"), Some(Level::Info));
        assert_eq!(Level::from_str("Warn"), Some(Level::Warn));
        assert_eq!(Level::from_str("warning"), Some(Level::Warn));
        assert_eq!(Level::from_str("ERROR"), Some(Level::Error));
        assert_eq!(Level::from_str("unknown"), None);
    }
}
