use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::error::{AppError, AppResult};

pub const DEFAULT_CHUNK_SIZE_MB: usize = 1;
pub const DEFAULT_MAX_PARALLEL_TRANSFERS: usize = 5;
pub const DEFAULT_ZIP_COMPRESSION_LEVEL: u8 = 6;
pub const DEFAULT_PORT: u16 = 8080;
pub const SERVICE_TYPE: &str = "_streamline._tcp.local.";
pub const PROTOCOL_VERSION: u8 = 2;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub listen_address: String,
    pub output_path: Option<String>,
    pub chunk_size_mb: usize,
    pub max_parallel_transfers: usize,
    pub zip_compression_level: u8,
    pub require_accept: bool,
    pub trusted_peers: Vec<String>,
    pub device_name: String,
    pub advertise: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            listen_address: format!("0.0.0.0:{DEFAULT_PORT}"),
            output_path: None,
            chunk_size_mb: DEFAULT_CHUNK_SIZE_MB,
            max_parallel_transfers: DEFAULT_MAX_PARALLEL_TRANSFERS,
            zip_compression_level: DEFAULT_ZIP_COMPRESSION_LEVEL,
            require_accept: false,
            trusted_peers: Vec::new(),
            device_name: default_device_name(),
            advertise: true,
        }
    }
}

impl Config {
    pub fn chunk_size_bytes(&self) -> usize {
        self.chunk_size_mb * 1024 * 1024
    }

    pub fn load_or_default() -> AppResult<Self> {
        let path = crate::paths::config_file();
        if !path.exists() {
            return Ok(Self::default());
        }
        let text = std::fs::read_to_string(&path)?;
        let cfg: Self = toml::from_str(&text)?;
        Ok(cfg)
    }

    pub fn save(&self) -> AppResult<()> {
        crate::paths::ensure_dirs()?;
        let text = toml::to_string_pretty(self)?;
        std::fs::write(crate::paths::config_file(), text)?;
        Ok(())
    }

    #[allow(dead_code)]
    pub fn merge_from_args(
        &mut self,
        listen_address: Option<String>,
        output_path: Option<Option<String>>,
        chunk_size_mb: Option<usize>,
        max_parallel: Option<usize>,
        zip_level: Option<u8>,
        require_accept: Option<bool>,
    ) {
        if let Some(a) = listen_address {
            self.listen_address = a;
        }
        if let Some(o) = output_path {
            self.output_path = o;
        }
        if let Some(c) = chunk_size_mb {
            self.chunk_size_mb = c;
        }
        if let Some(p) = max_parallel {
            self.max_parallel_transfers = p;
        }
        if let Some(z) = zip_level {
            self.zip_compression_level = z;
        }
        if let Some(r) = require_accept {
            self.require_accept = r;
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Peer {
    pub name: String,
    pub address: String,
    #[serde(default)]
    pub trusted: bool,
    #[serde(default)]
    pub last_seen: Option<String>,
    #[serde(default)]
    pub port: Option<u16>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PeerBook {
    pub peers: Vec<Peer>,
}

impl PeerBook {
    pub fn load() -> AppResult<Self> {
        let path = crate::paths::peers_file();
        if !path.exists() {
            return Ok(Self::default());
        }
        let text = std::fs::read_to_string(&path)?;
        let book: Self = toml::from_str(&text)?;
        Ok(book)
    }

    pub fn save(&self) -> AppResult<()> {
        crate::paths::ensure_dirs()?;
        let text = toml::to_string_pretty(self)?;
        std::fs::write(crate::paths::peers_file(), text)?;
        Ok(())
    }

    pub fn add(&mut self, peer: Peer) -> AppResult<()> {
        if peer.name.trim().is_empty() {
            return Err(AppError::config("peer name cannot be empty"));
        }
        if peer.address.trim().is_empty() {
            return Err(AppError::InvalidAddress(peer.address));
        }
        self.peers.retain(|p| p.name != peer.name);
        self.peers.push(peer);
        self.save()
    }

    pub fn remove(&mut self, name: &str) -> AppResult<bool> {
        let before = self.peers.len();
        self.peers.retain(|p| p.name != name);
        let removed = self.peers.len() != before;
        if removed {
            self.save()?;
        }
        Ok(removed)
    }

    pub fn get(&self, name: &str) -> Option<&Peer> {
        self.peers.iter().find(|p| p.name == name)
    }

    pub fn trust(&mut self, name: &str, trusted: bool) -> AppResult<()> {
        if let Some(p) = self.peers.iter_mut().find(|p| p.name == name) {
            p.trusted = trusted;
            self.save()?;
            Ok(())
        } else {
            Err(AppError::PeerNotFound(name.to_string()))
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum QueueStatus {
    Queued,
    Active,
    Done,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Direction {
    Send,
    Recv,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueueItem {
    pub id: String,
    pub direction: Direction,
    pub peer: String,
    pub path: String,
    pub size: u64,
    pub transferred: u64,
    pub status: QueueStatus,
    pub error: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Queue {
    pub items: Vec<QueueItem>,
}

impl Queue {
    pub fn load() -> AppResult<Self> {
        let path = crate::paths::queue_file();
        if !path.exists() {
            return Ok(Self::default());
        }
        let text = std::fs::read_to_string(&path)?;
        let q: Self = serde_json::from_str(&text)?;
        Ok(q)
    }

    pub fn save(&self) -> AppResult<()> {
        crate::paths::ensure_dirs()?;
        let text = serde_json::to_string_pretty(self)?;
        let tmp = crate::paths::queue_file().with_extension("json.tmp");
        std::fs::write(&tmp, text)?;
        std::fs::rename(&tmp, crate::paths::queue_file())?;
        Ok(())
    }

    pub fn enqueue(&mut self, item: QueueItem) -> AppResult<()> {
        self.items.push(item);
        self.save()
    }

    #[allow(dead_code)]
    pub fn update<F: FnOnce(&mut QueueItem)>(&mut self, id: &str, f: F) -> AppResult<()> {
        if let Some(item) = self.items.iter_mut().find(|i| i.id == id) {
            f(item);
            self.save()?;
        }
        Ok(())
    }

    #[allow(dead_code)]
    pub fn list(&self) -> &[QueueItem] {
        &self.items
    }
}

fn default_device_name() -> String {
    hostname::get()
        .ok()
        .and_then(|h| h.into_string().ok())
        .unwrap_or_else(|| "streamline-host".to_string())
}

pub fn ensure_output_dir(path: &Path) -> AppResult<()> {
    if !path.exists() {
        std::fs::create_dir_all(path)?;
    }
    if !path.is_dir() {
        return Err(AppError::config(format!(
            "output path is not a directory: {}",
            path.display()
        )));
    }
    Ok(())
}
