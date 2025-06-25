use crate::{PostError, Result};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tokio::fs;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PostConfig {
    pub node: NodeConfig,
    pub network: NetworkConfig,
    pub security: SecurityConfig,
    pub ui: UiConfig,
    pub filters: FilterConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeConfig {
    pub name: String,
    pub id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkConfig {
    pub tailscale_socket: Option<String>,
    pub port: u16,
    pub discovery_interval: u64,
    pub heartbeat_interval: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecurityConfig {
    pub enable_encryption: bool,
    pub key_derivation_iterations: u32,
    pub max_content_size: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UiConfig {
    pub enable_tui: bool,
    pub vim_keys: bool,
    pub colors: ColorConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ColorConfig {
    pub connected: String,
    pub syncing: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FilterConfig {
    pub lua_hooks: Vec<String>,
    pub js_hooks: Vec<String>,
    pub max_length: Option<usize>,
    pub exclude_patterns: Vec<String>,
}

impl Default for PostConfig {
    fn default() -> Self {
        Self {
            node: NodeConfig {
                name: hostname::get()
                    .unwrap_or_else(|_| "unknown".into())
                    .to_string_lossy()
                    .to_string(),
                id: None,
            },
            network: NetworkConfig {
                tailscale_socket: None,
                port: 19827,
                discovery_interval: 30,
                heartbeat_interval: 10,
            },
            security: SecurityConfig {
                enable_encryption: true,
                key_derivation_iterations: 100_000,
                max_content_size: 1024 * 1024,
            },
            ui: UiConfig {
                enable_tui: true,
                vim_keys: true,
                colors: ColorConfig {
                    connected: "green".to_string(),
                    syncing: "yellow".to_string(),
                    error: "red".to_string(),
                },
            },
            filters: FilterConfig {
                lua_hooks: vec![],
                js_hooks: vec![],
                max_length: Some(10_000),
                exclude_patterns: vec![],
            },
        }
    }
}

impl PostConfig {
    pub fn config_dir() -> Result<PathBuf> {
        dirs::config_dir()
            .map(|d| d.join("post"))
            .ok_or_else(|| PostError::Config("Unable to determine config directory".to_string()))
    }

    pub fn config_path() -> Result<PathBuf> {
        Ok(Self::config_dir()?.join("config.toml"))
    }

    pub async fn load() -> Result<Self> {
        let path = Self::config_path()?;

        if !path.exists() {
            let config = Self::default();
            config.save().await?;
            return Ok(config);
        }

        let contents = fs::read_to_string(&path).await?;
        let config: PostConfig = toml::from_str(&contents)?;
        Ok(config)
    }

    pub async fn save(&self) -> Result<()> {
        let config_dir = Self::config_dir()?;
        fs::create_dir_all(&config_dir).await?;

        // Set secure permissions on config directory (700 - owner only)
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let permissions = std::fs::Permissions::from_mode(0o700);
            std::fs::set_permissions(&config_dir, permissions)?;
        }

        let path = Self::config_path()?;
        let contents = toml::to_string_pretty(self)
            .map_err(|e| PostError::Config(format!("Failed to serialize config: {}", e)))?;

        fs::write(&path, contents).await?;

        // Set secure permissions on config file (600 - owner read/write only)
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let permissions = std::fs::Permissions::from_mode(0o600);
            std::fs::set_permissions(&path, permissions)?;
        }

        Ok(())
    }
}
