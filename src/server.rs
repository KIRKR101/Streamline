use std::path::PathBuf;
use std::sync::Arc;

use tokio::net::{TcpListener, TcpStream};

use crate::config::{Config, Peer, PeerBook};
use crate::error::{AppError, AppResult};
use crate::transfer::receive_one;

#[derive(Debug, Clone)]
pub struct ServerOptions {
    pub address: String,
    pub output_dir: PathBuf,
    pub require_accept: bool,
    pub trusted_peers: Vec<String>,
}

impl ServerOptions {
    pub fn from_config(cfg: &Config) -> AppResult<Self> {
        let output_dir = match &cfg.output_path {
            Some(p) => PathBuf::from(p),
            None => std::env::current_dir()?,
        };
        crate::config::ensure_output_dir(&output_dir)?;
        Ok(Self {
            address: cfg.listen_address.clone(),
            output_dir,
            require_accept: cfg.require_accept,
            trusted_peers: cfg.trusted_peers.clone(),
        })
    }
}

pub async fn run(
    opts: Arc<ServerOptions>,
    mut shutdown: tokio::sync::watch::Receiver<bool>,
) -> AppResult<()> {
    let listener = TcpListener::bind(&opts.address)
        .await
        .map_err(|e| match e.kind() {
            std::io::ErrorKind::AddrInUse => AppError::AddrInUse(opts.address.clone()),
            _ => AppError::Io(e),
        })?;
    tracing::info!("server listening on {}", opts.address);

    let book = PeerBook::load().unwrap_or_default();
    let trusted_peers = opts.trusted_peers.clone();

    loop {
        if *shutdown.borrow() {
            tracing::info!("server shutting down");
            return Ok(());
        }
        tokio::select! {
            _ = shutdown.changed() => continue,
            res = listener.accept() => {
                let (socket, addr) = res?;
                tracing::info!("connection from {addr}");
                let opts = opts.clone();
                let peer_ip = addr.ip().to_string();
                let auto_trust = is_trusted(&book, &trusted_peers, &peer_ip);
                tokio::spawn(async move {
                    if let Err(e) = handle_conn(socket, opts, auto_trust).await {
                        tracing::error!("error handling {addr}: {e}");
                    }
                });
            }
        }
    }
}

fn is_trusted(book: &PeerBook, trusted_names: &[String], peer_addr: &str) -> bool {
    let candidates: Vec<&Peer> = if trusted_names.is_empty() {
        book.peers.iter().filter(|p| p.trusted).collect()
    } else {
        book.peers
            .iter()
            .filter(|p| p.trusted && trusted_names.iter().any(|n| n == &p.name))
            .collect()
    };
    candidates
        .iter()
        .any(|p| address_compatible(&p.address, peer_addr))
}

fn address_compatible(saved: &str, connecting: &str) -> bool {
    if saved == connecting {
        return true;
    }
    let saved_host = saved.rsplit(':').nth(1).unwrap_or(saved);
    let connecting_host = connecting.rsplit(':').next().unwrap_or(connecting);
    saved_host == connecting_host
}

async fn handle_conn(
    socket: TcpStream,
    opts: Arc<ServerOptions>,
    auto_trust: bool,
) -> AppResult<()> {
    let outcome = receive_one(
        socket,
        opts.output_dir.clone(),
        opts.require_accept,
        auto_trust,
    )
    .await?;
    tracing::info!(
        "received '{}' ({} bytes, verified={})",
        outcome.name,
        outcome.bytes,
        outcome.verified
    );
    Ok(())
}
