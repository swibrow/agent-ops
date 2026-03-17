use std::path::PathBuf;

use anyhow::{Context, Result};

pub struct Config {
    pub claude_dir: PathBuf,
    pub db_path: PathBuf,
    pub log_path: PathBuf,
    pub poll_interval_secs: u64,
    pub tick_rate_ms: u64,
    pub notifications_enabled: bool,
}

impl Config {
    pub fn new() -> Result<Self> {
        let home = dirs::home_dir().context("could not find home directory")?;
        let data_dir = dirs::data_local_dir()
            .unwrap_or_else(|| home.join(".local/share"))
            .join("agent-ops");

        Ok(Self {
            claude_dir: home.join(".claude"),
            db_path: data_dir.join("agent-ops.db"),
            log_path: data_dir.join("agent-ops.log"),
            poll_interval_secs: 3,
            tick_rate_ms: 50,
            notifications_enabled: true,
        })
    }
}
