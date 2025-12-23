# rsys_log — LCARS форматтер логов

Лёгкий билдер лог-строк в стиле LCARS/Star Trek для сервисов RS-CRM.

## Формат
```
2025.1205.10:15:30|SSYS=db|CTRL=migrator|LVL=INFO|CID=op12|MSG=Migration applied|name:2025-12-01-001-rbac dur_ms:214
```
- Timestamp — `YYYY.MMDD.HH:MM:SS` (UTC).
- `SSYS` — подсистема (`db`, `auth`, `ws`, `migrator` и т.д.).
- `CTRL` — контроллер/модуль.
- `LVL` — `TRACE|DEBUG|INFO|WARN|ERROR`.
- `CID` — correlation id или `-`.
- `MSG` — краткое действие в настоящем времени.
- Далее `key:val` пары с дополнительными данными.

Глобальный уровень можно установить один раз: `rsys_log::set_global_level(Level::Warn)`. Сообщения с уровнем ниже заданного не выводятся, если используете `rsys_log::log_line(builder)`.

## Пример использования
```rust
use rsys_log::{Level, LogBuilder};

fn main() {
    rsys_log::set_global_level(Level::Info);

    LogBuilder::new("db", "migrator", Level::Info, "Migration applied")
        .cid("op12")
        .data("name", "2025-12-01-001-rbac")
        .data("dur_ms", "214")
        .print(); // печатает лог, если INFO >= глобального уровня
}
```

Многострочная ошибка с деталями:
```rust
let err = LogBuilder::new("auth", "jwt", Level::Error, "JWT verify failed")
    .cid("ab7c")
    .data("code", "401")
    .detail("token: eyJhbGciOi...")
    .detail("hint: refresh token")
    .print(); // ERROR пройдёт при глобальном INFO/WARN/ERROR
```

С короткими сеттерами (сообщение первым аргументом):
```rust
LogBuilder::msg("Сообщени лога")
    .ssys("db")
    .ctrl("db_info")
    .cid("123")
    .info()
    .print();
```

### Подписка на поток логов
```rust
use rsys_log::subscribe_logs;

let receiver = subscribe_logs();
// Любая печать через rsys_log::log_line или LogBuilder::print()
// отправит строку всем подписчикам:
std::thread::spawn(move || {
    while let Ok(line) = receiver.recv() {
        println!("Новый лог: {}", line);
    }
});
```

### Демонстрация
Запустить демонстрационное приложение с разноцветными логами:
```
cargo run -p rsys_log --bin demo
```
Выводит 100 разноуровневых логов подряд.

### Кастомная цветовая тема
Можно подключить собственную схему:
```rust
use rsys_log::{set_color_scheme, Level};

fn main() {
    set_color_scheme(|level| match level {
        Level::Error => rsys_log::colorscheme::ColorScheme {
            level: "31",
            header: "90",
            cid: "95",
            msg: "91",
            key: "33",
            value: "97",
        },
        _ => rsys_log::colorscheme::ColorScheme {
            level: "36",
            header: "90",
            cid: "95",
            msg: "92",
            key: "94",
            value: "97",
        },
    });
}
```
По умолчанию используется Gruvbox Dark (`colorscheme::gruvbox_dark`).

## Цвета (опционально)
- По умолчанию ANSI-цвета включены; отключить можно `.colorize(false)`.
- Палитра Gruvbox Dark: TRACE `38;5;109`, DEBUG `38;5;108`, INFO `38;5;142`, WARN `38;5;214`, ERROR `38;5;167`; timestamp `38;5;246`, `SSYS/CTRL` `38;5;222`, CID `38;5;175`, MSG `38;5;223`, ключи `38;5;208`, значения `38;5;223`.
- Без поддержки ANSI вывод остаётся чистым текстом.

## Ограничения и правила
- Не логируйте PII: используйте id/slug вместо email/ФИО.
- Строка лога ≤ 240 символов.
- Все поля обязательны: SSYS, CTRL, LVL, CID, MSG.
