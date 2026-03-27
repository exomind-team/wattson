use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::config::Config;

/// Preferred application theme / 主题偏好
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ThemePreference {
    /// Follow operating system preference / 跟随系统
    System,
    /// Force light visuals / 强制浅色
    Light,
    /// Force dark visuals / 强制深色
    Dark,
}

/// Power chart scale mode / 图表缩放模式
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ChartScaleMode {
    /// Auto-fit the interesting range / 自动贴合数据范围
    Auto,
    /// Keep the Y axis grounded at zero / 零基线显示
    ZeroBased,
}

/// Persisted desktop controls / 图形界面持久化设置
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct GuiSettings {
    pub theme: ThemePreference,
    pub chart_window_s: u64,
    pub ui_refresh_ms: u64,
    pub poll_interval_ms: u64,
    pub show_ac_input: bool,
    pub show_dc_output: bool,
    pub show_full_serial: bool,
    pub chart_scale: ChartScaleMode,
    pub window_width: u32,
    pub window_height: u32,
}

impl Default for GuiSettings {
    fn default() -> Self {
        Self {
            theme: ThemePreference::System,
            chart_window_s: 120,
            ui_refresh_ms: 16,
            poll_interval_ms: 300,
            show_ac_input: true,
            show_dc_output: true,
            show_full_serial: false,
            chart_scale: ChartScaleMode::Auto,
            window_width: 1440,
            window_height: 900,
        }
    }
}

impl GuiSettings {
    /// Default GUI settings path / 默认 GUI 设置路径
    pub fn active_path() -> PathBuf {
        if let Some(config_path) = Config::active_path() {
            return config_path.with_file_name("wattson_gui.json");
        }

        if let Some(config_dir) = dirs::config_dir() {
            return config_dir.join("wattson").join("wattson_gui.json");
        }

        PathBuf::from("wattson_gui.json")
    }

    /// Load persisted settings, falling back to defaults / 读取设置，缺失则回退默认值
    pub fn load() -> Self {
        let path = Self::active_path();
        Self::load_from(&path).unwrap_or_default()
    }

    /// Load from a specific file / 从指定文件读取
    pub fn load_from(path: &Path) -> Result<Self, String> {
        let content = fs::read_to_string(path)
            .map_err(|e| format!("Failed to read {}: {}", path.display(), e))?;
        serde_json::from_str(&content)
            .map_err(|e| format!("Failed to parse {}: {}", path.display(), e))
    }

    /// Save to the default file / 保存到默认设置文件
    pub fn save(&self) -> Result<PathBuf, String> {
        let path = Self::active_path();
        self.save_to(&path)?;
        Ok(path)
    }

    /// Save to a specific file / 保存到指定文件
    pub fn save_to(&self, path: &Path) -> Result<(), String> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .map_err(|e| format!("Failed to create dir {}: {}", parent.display(), e))?;
        }

        let json = serde_json::to_string_pretty(self)
            .map_err(|e| format!("Failed to serialize GUI settings: {}", e))?;
        fs::write(path, json).map_err(|e| format!("Failed to write {}: {}", path.display(), e))
    }
}
