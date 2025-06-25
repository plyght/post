use thiserror::Error;

#[derive(Error, Debug)]
pub enum PostError {
    #[error("Clipboard error: {0}")]
    Clipboard(String),

    #[error("Tailscale error: {0}")]
    Tailscale(String),

    #[error("Crypto error: {0}")]
    Crypto(String),

    #[error("Config error: {0}")]
    Config(String),

    #[error("Network error: {0}")]
    Network(String),

    #[error("Serialization error: {0}")]
    Serialization(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("TOML error: {0}")]
    Toml(#[from] toml::de::Error),

    #[error("Other error: {0}")]
    Other(String),
}

pub type Result<T> = std::result::Result<T, PostError>;
