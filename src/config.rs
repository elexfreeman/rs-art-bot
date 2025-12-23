use anyhow::{Context, Result};
use serde::Deserialize;
use std::fs;

#[derive(Debug, Deserialize, Clone)]
pub struct Config {
    #[serde(alias = "TELOXIDE_TOKEN", alias = "teloxide_token")]
    pub teloxide_token: String,
    #[serde(alias = "CHANNEL_ID", alias = "channel_id")]
    pub channel_id: Option<i64>,
    #[serde(alias = "DB_PATH", alias = "db_path", default = "default_db_path")]
    pub db_path: String,
    #[serde(alias = "FILES_DIR", alias = "files_dir", default = "default_files_dir")]
    pub files_dir: String,
    #[serde(
        alias = "POST_INTERVAL_SECS",
        alias = "post_interval_secs",
        default
    )]
    pub post_interval_secs: u64,
    #[serde(alias = "POST_CRON", alias = "post_cron")]
    pub post_cron: Option<String>,
    #[serde(alias = "OPENAI_API_KEY", alias = "openai_api_key")]
    pub openai_api_key: Option<String>,
    #[serde(alias = "OPENAI_MODEL", alias = "openai_model", default = "default_openai_model")]
    pub openai_model: String,
    #[serde(alias = "OPENAI_BASE", alias = "openai_base", default = "default_openai_base")]
    pub openai_base: String,
    #[serde(alias = "OPENAI_USE_VISION", alias = "openai_use_vision")]
    pub openai_use_vision: Option<bool>,
    #[serde(alias = "OPENAI_VISION_MODEL", alias = "openai_vision_model")]
    pub openai_vision_model: Option<String>,
    #[serde(alias = "OPENAI_SYSTEM_PROMPT", alias = "openai_system_prompt")]
    pub openai_system_prompt: Option<String>,
    #[serde(alias = "LOG_LEVEL", alias = "log_level")]
    pub log_level: Option<String>,
}

fn default_db_path() -> String {
    "bot.db".to_string()
}

fn default_files_dir() -> String {
    "files".to_string()
}

fn default_openai_model() -> String {
    "gpt-5.2".to_string()
}

fn default_openai_base() -> String {
    "https://api.openai.com".to_string()
}

pub fn load_config(path: &str) -> Result<Config> {
    let raw = fs::read_to_string(path)
        .with_context(|| format!("не удалось прочитать config: {}", path))?;
    let cfg: Config =
        serde_json::from_str(&raw).with_context(|| format!("некорректный JSON: {}", path))?;
    Ok(cfg)
}
