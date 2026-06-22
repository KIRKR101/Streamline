use std::io;
use thiserror::Error;

#[derive(Debug, Error)]
#[allow(dead_code)]
pub enum AppError {
    #[error("io error: {0}")]
    Io(#[from] io::Error),

    #[error("config error: {0}")]
    Config(String),

    #[error("peer not found: {0}")]
    PeerNotFound(String),

    #[error("invalid address: {0}")]
    InvalidAddress(String),

    #[error("invalid header: {0}")]
    InvalidHeader(String),

    #[error("invalid utf-8 in {field}: {source}")]
    Utf8 {
        field: &'static str,
        #[source]
        source: std::str::Utf8Error,
    },

    #[error("transfer declined by peer")]
    Declined,

    #[error("transfer cancelled")]
    Cancelled,

    #[error("transfer failed: {0}")]
    Transfer(String),

    #[error("zip error: {0}")]
    Zip(#[from] zip::result::ZipError),

    #[error("toml decode error: {0}")]
    TomlDe(#[from] toml::de::Error),

    #[error("toml encode error: {0}")]
    TomlSer(#[from] toml::ser::Error),

    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("address already in use: {0}")]
    AddrInUse(String),

    #[error("{0}")]
    Other(String),
}

impl AppError {
    pub fn config(msg: impl Into<String>) -> Self {
        Self::Config(msg.into())
    }
    pub fn other(msg: impl Into<String>) -> Self {
        Self::Other(msg.into())
    }
}

impl From<tokio::task::JoinError> for AppError {
    fn from(e: tokio::task::JoinError) -> Self {
        Self::Other(format!("task join error: {e}"))
    }
}

#[cfg(feature = "tray")]
impl From<tray_icon::Error> for AppError {
    fn from(e: tray_icon::Error) -> Self {
        Self::Other(format!("tray icon: {e}"))
    }
}

#[cfg(feature = "tray")]
impl From<tray_icon::menu::Error> for AppError {
    fn from(e: tray_icon::menu::Error) -> Self {
        Self::Other(format!("tray menu: {e}"))
    }
}

pub type AppResult<T> = Result<T, AppError>;
