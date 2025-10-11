use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Config {
    pub channel_id: Option<i64>,
    #[serde(default)]
    pub post_cron: Option<String>,
}

impl Config {
    fn path() -> PathBuf {
        PathBuf::from("config.json")
    }

    pub fn load() -> Result<Self> {
        let p = Self::path();
        if !p.exists() {
            // Try env fallback
            let env_channel = std::env::var("CHANNEL_ID").ok().and_then(|v| v.parse::<i64>().ok());
            let cfg = Self { channel_id: env_channel, post_cron: None };
            cfg.save()?;
            return Ok(cfg);
        }
        let data = fs::read_to_string(p)?;
        let cfg: Self = serde_json::from_str(&data)?;
        Ok(cfg)
    }

    pub fn save(&self) -> Result<()> {
        let p = Self::path();
        let data = serde_json::to_string_pretty(self)?;
        fs::write(p, data)?;
        Ok(())
    }

    pub fn from_json_str(s: &str) -> Result<Self> {
        let cfg: Self = serde_json::from_str(s)?;
        Ok(cfg)
    }
}
