use crate::Level;

/// Цветовая схема ANSI для логов.
#[derive(Debug, Clone, Copy)]
pub struct ColorScheme<'a> {
    pub level: &'a str,
    pub header: &'a str,
    pub context: &'a str,
    pub cid: &'a str,
    pub msg: &'a str,
    pub key: &'a str,
    pub value: &'a str,
}

/// Резолвер цветовой схемы по уровню.
pub type ColorResolver = fn(super::Level) -> ColorScheme<'static>;

/// Gruvbox Dark палитра по уровням.
pub fn gruvbox_dark(level: Level) -> ColorScheme<'static> {
    match level {
        Level::Trace => ColorScheme {
            level: "38;5;109",
            header: "38;5;246",
            context: "38;5;222",
            cid: "38;5;175",
            msg: "38;5;223",
            key: "38;5;208",
            value: "38;5;223",
        },
        Level::Debug => ColorScheme {
            level: "38;5;108",
            header: "38;5;246",
            context: "38;5;222",
            cid: "38;5;175",
            msg: "38;5;223",
            key: "38;5;208",
            value: "38;5;223",
        },
        Level::Info => ColorScheme {
            level: "38;5;142",
            header: "38;5;246",
            context: "38;5;222",
            cid: "38;5;175",
            msg: "38;5;223",
            key: "38;5;208",
            value: "38;5;223",
        },
        Level::Warn => ColorScheme {
            level: "38;5;214",
            header: "38;5;246",
            context: "38;5;222",
            cid: "38;5;175",
            msg: "38;5;223",
            key: "38;5;208",
            value: "38;5;223",
        },
        Level::Error => ColorScheme {
            level: "38;5;167",
            header: "38;5;246",
            context: "38;5;222",
            cid: "38;5;175",
            msg: "38;5;223",
            key: "38;5;208",
            value: "38;5;223",
        },
    }
}
