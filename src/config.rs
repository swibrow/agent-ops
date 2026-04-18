use std::path::PathBuf;

use anyhow::{Context, Result};
use serde::Deserialize;

/// On-disk config: ~/.config/agent-ops/config.toml
#[derive(Debug, Deserialize, Default)]
struct FileConfig {
    #[serde(default)]
    claude_dirs: Vec<PathBuf>,
    /// Keep transcripts older than this many days? 0 or unset = unlimited.
    #[serde(default)]
    transcript_retention_days: Option<u32>,
}

pub struct Config {
    pub claude_dirs: Vec<PathBuf>,
    pub db_path: PathBuf,
    pub log_path: PathBuf,
    pub poll_interval_secs: u64,
    pub tick_rate_ms: u64,
    pub notifications_enabled: bool,
    /// Days of transcript history to keep. None = unlimited.
    pub transcript_retention_days: Option<u32>,
}

impl Config {
    pub fn new(extra_claude_dirs: &[PathBuf]) -> Result<Self> {
        let home = dirs::home_dir().context("could not find home directory")?;
        let data_dir = dirs::data_local_dir()
            .unwrap_or_else(|| home.join(".local/share"))
            .join("agent-ops");

        // Load global config file
        let config_path = dirs::config_dir()
            .unwrap_or_else(|| home.join(".config"))
            .join("agent-ops")
            .join("config.toml");

        let file_config = if config_path.exists() {
            let content = std::fs::read_to_string(&config_path)
                .with_context(|| format!("failed to read {}", config_path.display()))?;
            toml::from_str::<FileConfig>(&content)
                .with_context(|| format!("failed to parse {}", config_path.display()))?
        } else {
            FileConfig::default()
        };

        // Build claude_dirs: config file replaces default, CLI extras always added
        let mut dirs = if file_config.claude_dirs.is_empty() {
            vec![home.join(".claude")]
        } else {
            file_config.claude_dirs
        };
        dirs.extend(extra_claude_dirs.iter().cloned());

        let mut seen = std::collections::HashSet::new();
        dirs.retain(|d| seen.insert(d.clone()));

        Ok(Self {
            claude_dirs: dirs,
            db_path: data_dir.join("agent-ops.db"),
            log_path: data_dir.join("agent-ops.log"),
            poll_interval_secs: 3,
            tick_rate_ms: 50,
            notifications_enabled: true,
            transcript_retention_days: file_config.transcript_retention_days,
        })
    }

    pub fn config_path() -> PathBuf {
        dirs::config_dir()
            .unwrap_or_else(|| {
                dirs::home_dir()
                    .expect("could not find home directory")
                    .join(".config")
            })
            .join("agent-ops")
            .join("config.toml")
    }
}
