use std::path::PathBuf;

use clap::{Parser, Subcommand, value_parser};

use crate::config::{Config, Peer, DEFAULT_CHUNK_SIZE_MB, DEFAULT_MAX_PARALLEL_TRANSFERS, DEFAULT_ZIP_COMPRESSION_LEVEL};
use crate::error::AppResult;

#[derive(Parser, Debug)]
#[clap(
    name = "streamline",
    version,
    author,
    about = "Resumable, bidirectional file transfer with tray + mDNS"
)]
pub struct Cli {
    #[clap(subcommand)]
    pub command: Command,

    #[clap(long, global = true, help = "Verbose tracing output (RUST_LOG-style).")]
    pub verbose: bool,
}

#[derive(Subcommand, Debug)]
pub enum Command {
    Server {
        #[clap(short = 's', long, default_value_t = String::from("0.0.0.0:8080"), help = "Address to listen on")]
        address: String,
        #[clap(short = 'o', long, value_parser, help = "Output path for received files")]
        output_path: Option<String>,
        #[clap(long, help = "Require explicit accept for each incoming transfer")]
        require_accept: bool,
        #[clap(long, help = "Disable mDNS advertising")]
        no_advertise: bool,
    },
    Client {
        #[clap(value_parser, help = "Server address (host:port) or saved peer name")]
        target: String,
        #[clap(value_parser, help = "File or directory paths to send")]
        paths: Vec<PathBuf>,
        #[clap(short = 'c', long, default_value_t = DEFAULT_CHUNK_SIZE_MB, help = "Chunk size in MB")]
        chunk_size_mb: usize,
        #[clap(short = 'p', long, default_value_t = DEFAULT_MAX_PARALLEL_TRANSFERS, help = "Maximum parallel file transfers")]
        parallel: usize,
        #[clap(short = 'z', long, default_value_t = DEFAULT_ZIP_COMPRESSION_LEVEL, value_parser = value_parser!(u8).range(0..=9), help = "Zip compression level (0-9) for directories")]
        zip_level: u8,
    },
    Pair {
        name: String,
        address: String,
        #[clap(long, help = "Mark peer as trusted (auto-accept incoming transfers)")]
        trust: bool,
    },
    Peers {
        #[clap(subcommand)]
        action: PeersAction,
    },
    Queue {
        #[clap(subcommand)]
        action: QueueAction,
    },
    Discover {
        #[clap(long, default_value_t = 3, help = "Discovery timeout in seconds")]
        timeout: u64,
    },
    Install {
        #[clap(long, help = "Default peer name to use for shell integration")]
        peer: Option<String>,
    },
    Uninstall,
    SendViaShell {
        #[clap(long, help = "Peer name to send to")]
        peer: Option<String>,
        #[clap(value_parser, required = true, help = "One or more file/folder paths to send")]
        paths: Vec<PathBuf>,
    },
}

#[derive(Subcommand, Debug)]
pub enum PeersAction {
    List,
    Remove { name: String },
    Trust { name: String, #[clap(long)] revoke: bool },
}

#[derive(Subcommand, Debug)]
pub enum QueueAction {
    List,
    Clear {
        #[clap(long)]
        done: bool,
    },
}

pub fn resolve_target(target: &str) -> AppResult<String> {
    if target.contains(':') {
        return Ok(target.to_string());
    }
    let book = crate::config::PeerBook::load()?;
    book.get(target)
        .map(|p| p.address.clone())
        .ok_or_else(|| crate::error::AppError::PeerNotFound(target.to_string()))
}

pub fn run(cli: Cli) -> AppResult<()> {
    match cli.command {
        Command::Server { address, output_path, require_accept, no_advertise } => {
            let mut cfg = Config::load_or_default()?;
            cfg.listen_address = address;
            cfg.output_path = output_path;
            cfg.require_accept = require_accept;
            cfg.advertise = !no_advertise;
            cfg.save()?;
            run_server(cfg)
        }
        Command::Client { target, paths, chunk_size_mb, parallel, zip_level } => {
            let address = resolve_target(&target)?;
            let _cfg = Config::load_or_default()?;
            let opts = crate::client::ClientOptions {
                address,
                paths,
                max_parallel: parallel,
                chunk_size: chunk_size_mb * 1024 * 1024,
                zip_compression_level: zip_level,
            };
            let summary = tokio_block_on(crate::client::run(opts))?;
            println!("Sent: {} ok, {} failed (of {})", summary.succeeded, summary.failed, summary.total);
            if summary.failed > 0 {
                return Err(crate::error::AppError::Other(format!(
                    "{} of {} transfers failed",
                    summary.failed, summary.total
                )));
            }
            Ok(())
        }
        Command::Pair { name, address, trust } => {
            let mut book = crate::config::PeerBook::load()?;
            book.add(Peer { name: name.clone(), address, trusted: trust, last_seen: None, port: None })?;
            println!("Saved peer '{name}'");
            Ok(())
        }
        Command::Peers { action } => {
            let mut book = crate::config::PeerBook::load()?;
            match action {
                PeersAction::List => {
                    if book.peers.is_empty() {
                        println!("(no saved peers)");
                    } else {
                        for p in &book.peers {
                            let trust = if p.trusted { "trusted" } else { "untrusted" };
                            println!("{} -> {} [{trust}]", p.name, p.address);
                        }
                    }
                }
                PeersAction::Remove { name } => {
                    if book.remove(&name)? {
                        println!("Removed '{name}'");
                    } else {
                        println!("No such peer '{name}'");
                    }
                }
                PeersAction::Trust { name, revoke } => {
                    book.trust(&name, !revoke)?;
                    println!("Updated trust for '{name}'");
                }
            }
            Ok(())
        }
        Command::Queue { action } => {
            let mut q = crate::config::Queue::load()?;
            match action {
                QueueAction::List => {
                    if q.items.is_empty() {
                        println!("(empty queue)");
                    } else {
                        for item in &q.items {
                            let dir = match item.direction {
                                crate::config::Direction::Send => "send",
                                crate::config::Direction::Recv => "recv",
                            };
                            println!("[{:?}] {} {} -> {} ({} / {})", item.status, dir, item.peer, item.path, item.transferred, item.size);
                        }
                    }
                }
                QueueAction::Clear { done } => {
                    if done {
                        q.items.retain(|i| !matches!(i.status, crate::config::QueueStatus::Done));
                    } else {
                        q.items.clear();
                    }
                    q.save()?;
                    println!("Queue cleared");
                }
            }
            Ok(())
        }
        Command::Discover { timeout } => {
            tokio_block_on(crate::discover::discover(timeout))
        }
        Command::Install { peer } => {
            crate::shell::install(peer)?;
            println!("Shell integration installed.");
            Ok(())
        }
        Command::Uninstall => {
            crate::shell::uninstall()?;
            println!("Shell integration removed.");
            Ok(())
        }
        Command::SendViaShell { peer, paths } => {
            let peer_name = peer.unwrap_or_else(|| "default".into());
            let address = {
                let book = crate::config::PeerBook::load()?;
                book.get(&peer_name)
                    .map(|p| p.address.clone())
                    .ok_or_else(|| crate::error::AppError::PeerNotFound(peer_name.clone()))?
            };
            let cfg = Config::load_or_default()?;
            let opts = crate::client::ClientOptions {
                address,
                paths,
                max_parallel: cfg.max_parallel_transfers,
                chunk_size: cfg.chunk_size_bytes(),
                zip_compression_level: cfg.zip_compression_level,
            };
            let summary = tokio_block_on(crate::client::run(opts))?;
            if summary.failed > 0 {
                return Err(crate::error::AppError::Other(format!(
                    "{} of {} transfers failed",
                    summary.failed, summary.total
                )));
            }
            Ok(())
        }
    }
}

fn tokio_block_on<F: std::future::Future>(f: F) -> F::Output {
    crate::runtime::block_on(f)
}

fn run_server(cfg: Config) -> AppResult<()> {
    let rt = tokio::runtime::Runtime::new().map_err(crate::error::AppError::from)?;
    rt.block_on(async move {
        let opts = crate::server::ServerOptions::from_config(&cfg)?;
        let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);
        let (cmd_tx, cmd_rx) = tokio::sync::mpsc::unbounded_channel();

        let cfg_arc = std::sync::Arc::new(tokio::sync::Mutex::new(cfg.clone()));
        let output_dir = opts.output_dir.clone();
        let server_opts = std::sync::Arc::new(opts);
        let server_handle = tokio::spawn({
            let opts = server_opts.clone();
            let rx = shutdown_rx.clone();
            async move { crate::server::run(opts, rx).await }
        });

        #[cfg(feature = "tray")]
        {
            if let Err(e) = crate::tray::spawn_event_loop(cmd_tx.clone()) {
                tracing::warn!("tray init failed: {e}");
            }
            let cfg_arc = cfg_arc.clone();
            let output_dir = output_dir.clone();
            let shutdown_tx_clone = shutdown_tx.clone();
            tokio::spawn(async move {
                if let Err(e) =
                    crate::tray::run_event_loop(cfg_arc, cmd_rx, output_dir, shutdown_tx_clone).await
                {
                    tracing::error!("tray event loop: {e}");
                }
            });
        }
        #[cfg(not(feature = "tray"))]
        {
            drop(cmd_tx);
            drop(cmd_rx);
        }

        #[cfg(feature = "discovery")]
        {
            let _daemon = if cfg.advertise {
                let port: u16 = cfg
                    .listen_address
                    .rsplit(':')
                    .next()
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(8080);
                match crate::discover::advertise(cfg.device_name.clone(), port).await {
                    Ok(d) => {
                        tracing::info!("mDNS advertising as '{}'", cfg.device_name);
                        Some(d)
                    }
                    Err(e) => {
                        tracing::warn!("mDNS advertise failed: {e}");
                        None
                    }
                }
            } else {
                None
            };
        }

        let _ = server_handle.await;
        let _ = shutdown_tx.send(true);
        Ok::<(), crate::error::AppError>(())
    })
}
