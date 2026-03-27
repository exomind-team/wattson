use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// Top-level configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    #[serde(default)]
    pub serial: SerialConfig,
    #[serde(default)]
    pub device: DeviceConfig,
    #[serde(default)]
    pub cost: CostConfig,
    #[serde(default)]
    pub chart: ChartConfig,
    #[serde(default)]
    pub api: ApiConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SerialConfig {
    #[serde(default = "default_port")]
    pub port: String,
    #[serde(default = "default_baud")]
    pub baud: u32,
    #[serde(default = "default_mode")]
    pub mode: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceConfig {
    #[serde(default = "default_profile")]
    pub profile: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CostConfig {
    #[serde(default = "default_price_per_kwh")]
    pub price_per_kwh: f64,
    #[serde(default = "default_currency")]
    pub currency: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChartConfig {
    #[serde(default = "default_output_dir")]
    pub output_dir: String,
    #[serde(default = "default_watermark")]
    pub watermark: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiConfig {
    #[serde(default = "default_api_port")]
    pub port: u16,
}

// Default value functions
fn default_port() -> String {
    "COM4".to_string()
}
fn default_baud() -> u32 {
    115200
}
fn default_mode() -> String {
    "active".to_string()
}
fn default_profile() -> String {
    "segotep_dm".to_string()
}
fn default_price_per_kwh() -> f64 {
    0.56
}
fn default_currency() -> String {
    "CNY".to_string()
}
fn default_output_dir() -> String {
    "./charts".to_string()
}
fn default_watermark() -> String {
    "Wattson | exomind-team/wattson".to_string()
}
fn default_api_port() -> u16 {
    8066
}

impl Default for SerialConfig {
    fn default() -> Self {
        Self {
            port: default_port(),
            baud: default_baud(),
            mode: default_mode(),
        }
    }
}

impl Default for DeviceConfig {
    fn default() -> Self {
        Self {
            profile: default_profile(),
        }
    }
}

impl Default for CostConfig {
    fn default() -> Self {
        Self {
            price_per_kwh: default_price_per_kwh(),
            currency: default_currency(),
        }
    }
}

impl Default for ChartConfig {
    fn default() -> Self {
        Self {
            output_dir: default_output_dir(),
            watermark: default_watermark(),
        }
    }
}

impl Default for ApiConfig {
    fn default() -> Self {
        Self {
            port: default_api_port(),
        }
    }
}

#[allow(clippy::derivable_impls)]
impl Default for Config {
    fn default() -> Self {
        Self {
            serial: SerialConfig::default(),
            device: DeviceConfig::default(),
            cost: CostConfig::default(),
            chart: ChartConfig::default(),
            api: ApiConfig::default(),
        }
    }
}

impl Config {
    /// Find config file: current dir first, then ~/.config/wattson/
    fn find_path() -> Option<PathBuf> {
        let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        Self::find_path_for(&cwd, dirs::config_dir().as_deref())
    }

    fn find_path_for(cwd: &Path, global_config_dir: Option<&Path>) -> Option<PathBuf> {
        if let Some(shared_root) = Self::linked_worktree_primary_root(cwd) {
            let shared = shared_root.join("wattson.toml");
            if shared.exists() {
                return Some(shared);
            }
        }

        let local = cwd.join("wattson.toml");
        if local.exists() {
            return Some(local);
        }

        if let Some(config_dir) = global_config_dir {
            let global = config_dir.join("wattson").join("wattson.toml");
            if global.exists() {
                return Some(global);
            }
        }

        None
    }

    fn default_path() -> PathBuf {
        let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        Self::default_path_for(&cwd)
    }

    fn default_path_for(cwd: &Path) -> PathBuf {
        if let Some(shared_root) = Self::linked_worktree_primary_root(cwd) {
            return shared_root.join("wattson.toml");
        }

        cwd.join("wattson.toml")
    }

    fn linked_worktree_primary_root(cwd: &Path) -> Option<PathBuf> {
        let dot_git = cwd.join(".git");
        if !dot_git.is_file() {
            return None;
        }

        let git_pointer = fs::read_to_string(&dot_git).ok()?;
        let gitdir = git_pointer.strip_prefix("gitdir:")?.trim();
        let gitdir_path = PathBuf::from(gitdir);
        let gitdir_path = if gitdir_path.is_absolute() {
            gitdir_path
        } else {
            cwd.join(gitdir_path)
        };

        let worktrees_dir = gitdir_path.parent()?;
        if worktrees_dir.file_name()? != "worktrees" {
            return None;
        }

        let common_git_dir = worktrees_dir.parent()?;
        common_git_dir.parent().map(Path::to_path_buf)
    }

    /// Load config from file, or return default if no file found
    pub fn load() -> Self {
        match Self::find_path() {
            Some(path) => Self::load_from(&path).unwrap_or_default(),
            None => Self::default(),
        }
    }

    /// Load config from a specific path
    pub fn load_from(path: &Path) -> Result<Self, String> {
        let content = fs::read_to_string(path)
            .map_err(|e| format!("Failed to read {}: {}", path.display(), e))?;
        toml::from_str(&content).map_err(|e| format!("Failed to parse {}: {}", path.display(), e))
    }

    /// Save config to the given path (or default location)
    pub fn save(&self) -> Result<PathBuf, String> {
        let path = Self::find_path().unwrap_or_else(Self::default_path);
        self.save_to(&path)?;
        Ok(path)
    }

    /// Save config to a specific path
    pub fn save_to(&self, path: &Path) -> Result<(), String> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .map_err(|e| format!("Failed to create dir {}: {}", parent.display(), e))?;
        }
        let content = toml::to_string_pretty(self)
            .map_err(|e| format!("Failed to serialize config: {}", e))?;
        fs::write(path, content).map_err(|e| format!("Failed to write {}: {}", path.display(), e))
    }

    /// Set a dotted config key (e.g. "serial.port") to a value string
    pub fn set_value(&mut self, key: &str, value: &str) -> Result<(), String> {
        match key {
            "serial.port" => self.serial.port = value.to_string(),
            "serial.baud" => {
                self.serial.baud = value
                    .parse()
                    .map_err(|_| format!("Invalid baud rate: {}", value))?;
            }
            "serial.mode" => {
                if value != "passive" && value != "active" {
                    return Err(format!(
                        "Invalid mode: {} (use 'passive' or 'active')",
                        value
                    ));
                }
                self.serial.mode = value.to_string();
            }
            "device.profile" => self.device.profile = value.to_string(),
            "cost.price_per_kwh" => {
                self.cost.price_per_kwh = value
                    .parse()
                    .map_err(|_| format!("Invalid price: {}", value))?;
            }
            "cost.currency" => self.cost.currency = value.to_string(),
            "chart.output_dir" => self.chart.output_dir = value.to_string(),
            "chart.watermark" => self.chart.watermark = value.to_string(),
            "api.port" => {
                self.api.port = value
                    .parse()
                    .map_err(|_| format!("Invalid port: {}", value))?;
            }
            _ => return Err(format!("Unknown config key: {}", key)),
        }
        Ok(())
    }

    /// Initialize default config file in current directory
    pub fn init_default() -> Result<PathBuf, String> {
        let path = Self::default_path();
        if path.exists() {
            return Err(format!("wattson.toml already exists at {}", path.display()));
        }
        let config = Self::default();
        config.save_to(&path)?;
        Ok(path)
    }

    /// Return the path of the config file being used (or None)
    pub fn active_path() -> Option<PathBuf> {
        Self::find_path()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_dir(prefix: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time")
            .as_nanos();
        std::env::temp_dir().join(format!("wattson-{prefix}-{unique}"))
    }

    #[test]
    fn linked_worktree_prefers_primary_worktree_config() {
        let root = temp_dir("primary-config");
        let main_repo = root.join("main");
        let worktree = root.join("gui-worktree");
        let worktree_gitdir = main_repo.join(".git").join("worktrees").join("gui");

        fs::create_dir_all(&worktree_gitdir).expect("create worktree gitdir");
        fs::create_dir_all(&worktree).expect("create worktree dir");
        fs::write(main_repo.join("wattson.toml"), "from='main'").expect("write main config");
        fs::write(worktree.join("wattson.toml"), "from='worktree'").expect("write worktree config");
        fs::write(
            worktree.join(".git"),
            format!("gitdir: {}\n", worktree_gitdir.display()),
        )
        .expect("write worktree .git");

        let resolved = Config::find_path_for(&worktree, None);

        assert_eq!(resolved, Some(main_repo.join("wattson.toml")));

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn normal_repo_prefers_local_config_when_not_in_linked_worktree() {
        let root = temp_dir("local-config");
        fs::create_dir_all(&root).expect("create temp root");
        fs::write(root.join("wattson.toml"), "from='local'").expect("write local config");

        let resolved = Config::find_path_for(&root, None);

        assert_eq!(resolved, Some(root.join("wattson.toml")));

        let _ = fs::remove_dir_all(&root);
    }
}
