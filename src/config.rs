use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use directories::ProjectDirs;
use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    pub github: GitHubConfig,
    pub ntfy: NtfyConfig,
    #[serde(default)]
    pub app: AppConfig,
    #[serde(default)]
    pub filters: FiltersConfig,
}

#[derive(Debug, Clone, Deserialize)]
pub struct GitHubConfig {
    #[serde(default)]
    pub token: Option<String>,
    #[serde(default = "default_api_base")]
    pub api_base: String,
    #[serde(default)]
    pub participating: bool,
    #[serde(default = "default_per_page")]
    pub per_page: u32,
    #[serde(default = "default_true")]
    pub enrich_pull_requests: bool,
    #[serde(default = "default_true")]
    pub enrich_issues: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct NtfyConfig {
    pub publish_url: String,
    #[serde(default)]
    pub token: Option<String>,
    #[serde(default = "default_timeout_secs")]
    pub timeout_secs: u64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct AppConfig {
    #[serde(default = "default_poll_interval_secs")]
    pub poll_interval_secs: u64,
    #[serde(default = "default_max_seen")]
    pub max_seen: usize,
    #[serde(default = "default_log_level")]
    pub log_level: String,
    #[serde(default)]
    pub state_path: Option<PathBuf>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct FiltersConfig {
    #[serde(default)]
    pub block: Vec<BlockRule>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct BlockRule {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub repo: Option<String>,
    #[serde(default)]
    pub owner: Option<String>,
    #[serde(default)]
    pub actor: Option<String>,
    #[serde(default)]
    pub actor_is_bot: Option<bool>,
    #[serde(default)]
    pub reason: Option<String>,
    #[serde(default)]
    pub subject_type: Option<String>,
    #[serde(default)]
    pub activity: Option<String>,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            poll_interval_secs: default_poll_interval_secs(),
            max_seen: default_max_seen(),
            log_level: default_log_level(),
            state_path: None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct LoadedConfig {
    pub config: Config,
    pub config_path: PathBuf,
    pub state_path: PathBuf,
}

impl LoadedConfig {
    pub fn load(path: Option<PathBuf>) -> Result<Self> {
        let config_path = match path {
            Some(path) => path,
            None => default_config_path()?,
        };
        let config_text = fs::read_to_string(&config_path)
            .with_context(|| format!("failed to read config: {}", config_path.display()))?;
        let config: Config = toml::from_str(&config_text)
            .with_context(|| format!("failed to parse config: {}", config_path.display()))?;
        validate(&config)?;
        let state_path = resolve_state_path(&config, &config_path)?;

        Ok(Self {
            config,
            config_path,
            state_path,
        })
    }
}

pub fn default_config_path() -> Result<PathBuf> {
    let project_dirs = project_dirs()?;
    Ok(project_dirs.config_dir().join("config.toml"))
}

fn resolve_state_path(config: &Config, config_path: &Path) -> Result<PathBuf> {
    if let Some(path) = &config.app.state_path {
        return Ok(path.clone());
    }

    if let Some(parent) = config_path.parent() {
        if parent
            .file_name()
            .is_some_and(|name| name == "github-ntfy-agent")
        {
            return Ok(parent.join("state.json"));
        }
    }

    let project_dirs = project_dirs()?;
    let state_dir = project_dirs
        .state_dir()
        .context("failed to resolve platform state directory")?;
    Ok(state_dir.join("state.json"))
}

fn validate(config: &Config) -> Result<()> {
    if config.ntfy.publish_url.trim().is_empty() {
        bail!("ntfy.publish_url must not be empty");
    }
    if config.github.per_page == 0 || config.github.per_page > 100 {
        bail!("github.per_page must be between 1 and 100");
    }
    if config.app.poll_interval_secs == 0 {
        bail!("app.poll_interval_secs must be greater than zero");
    }
    if config.app.max_seen == 0 {
        bail!("app.max_seen must be greater than zero");
    }
    Ok(())
}

fn project_dirs() -> Result<ProjectDirs> {
    ProjectDirs::from("dev", "github-ntfy", "github-ntfy-agent")
        .context("failed to resolve platform config directories")
}

fn default_api_base() -> String {
    String::from("https://api.github.com")
}

fn default_per_page() -> u32 {
    100
}

fn default_true() -> bool {
    true
}

fn default_timeout_secs() -> u64 {
    5
}

fn default_poll_interval_secs() -> u64 {
    60
}

fn default_max_seen() -> usize {
    2000
}

fn default_log_level() -> String {
    String::from("info")
}
