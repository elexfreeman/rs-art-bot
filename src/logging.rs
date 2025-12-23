pub use rsys_log::Level;

pub fn init_logging() {
    let raw = std::env::var("RSYS_LOG_LEVEL")
        .ok()
        .or_else(|| std::env::var("RUST_LOG").ok());
    let level = raw
        .as_deref()
        .and_then(|v| v.split(',').next())
        .and_then(|v| Level::from_str(v.trim()))
        .unwrap_or(Level::Info);
    rsys_log::set_global_level(level);
}

pub fn log(
    ssys: &str,
    ctrl: &str,
    level: Level,
    msg: impl Into<String>,
) -> rsys_log::LogBuilder {
    rsys_log::LogBuilder::new(ssys, ctrl, level, msg)
}

pub fn compact(value: &str, max: usize) -> String {
    if value.chars().count() <= max {
        value.to_string()
    } else {
        let mut s = value.chars().take(max).collect::<String>();
        s.push_str("...");
        s
    }
}
