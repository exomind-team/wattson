use std::fs;
use std::path::PathBuf;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::config::Config;

/// Persistent history data across sessions
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct History {
    /// Total accumulated energy in watt-hours
    pub total_wh: f64,
    /// Total monitoring duration in seconds
    pub total_duration_s: f64,
    /// Number of completed sessions
    pub session_count: u64,
    /// Timestamp of first session
    pub first_session: Option<DateTime<Utc>>,
    /// Timestamp of most recent session
    pub last_session: Option<DateTime<Utc>>,
}

impl Default for History {
    fn default() -> Self {
        Self {
            total_wh: 0.0,
            total_duration_s: 0.0,
            session_count: 0,
            first_session: None,
            last_session: None,
        }
    }
}

impl History {
    /// Find history file path (same directory as wattson.toml)
    fn file_path() -> PathBuf {
        if let Some(config_path) = Config::active_path() {
            config_path.with_file_name("wattson_history.json")
        } else {
            PathBuf::from("wattson_history.json")
        }
    }

    /// Load history from file, or return default if not found
    pub fn load() -> Self {
        let path = Self::file_path();
        match fs::read_to_string(&path) {
            Ok(content) => serde_json::from_str(&content).unwrap_or_else(|e| {
                log::warn!("Failed to parse {}: {}", path.display(), e);
                Self::default()
            }),
            Err(_) => Self::default(),
        }
    }

    /// Save history to file
    pub fn save(&self) -> Result<PathBuf, String> {
        let path = Self::file_path();
        let json = serde_json::to_string_pretty(self).map_err(|e| format!("Serialize: {}", e))?;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).ok();
        }
        fs::write(&path, json).map_err(|e| format!("Write {}: {}", path.display(), e))?;
        Ok(path)
    }

    /// Update history with current session data and save
    pub fn finish_session(&mut self, session_wh: f64, session_duration_s: f64) {
        self.total_wh += session_wh;
        self.total_duration_s += session_duration_s;
        self.session_count += 1;
        let now = Utc::now();
        if self.first_session.is_none() {
            self.first_session = Some(now);
        }
        self.last_session = Some(now);
    }

    /// Get all-time average power in watts (across all sessions)
    pub fn avg_power_w(&self) -> Option<f64> {
        let hours = self.total_duration_s / 3600.0;
        if hours > 0.0 {
            Some(self.total_wh / hours)
        } else {
            None
        }
    }
}
