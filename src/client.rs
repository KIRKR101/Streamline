use std::path::PathBuf;
use std::sync::Arc;

use tokio::sync::Semaphore;
use tokio::task::JoinSet;

use crate::config::Config;
use crate::error::{AppError, AppResult};
use crate::transfer::{SendRequest, send_with_resume};

pub struct ClientOptions {
    pub address: String,
    pub paths: Vec<PathBuf>,
    pub max_parallel: usize,
    pub chunk_size: usize,
    pub zip_compression_level: u8,
}

impl ClientOptions {
    #[allow(dead_code)]
    pub fn from_config(cfg: &Config, address: String, paths: Vec<PathBuf>) -> Self {
        Self {
            address,
            paths,
            max_parallel: cfg.max_parallel_transfers,
            chunk_size: cfg.chunk_size_bytes(),
            zip_compression_level: cfg.zip_compression_level,
        }
    }
}

#[derive(Debug, Clone)]
pub struct ClientSummary {
    pub total: usize,
    pub succeeded: usize,
    pub failed: usize,
}

pub async fn run(opts: ClientOptions) -> AppResult<ClientSummary> {
    if opts.paths.is_empty() {
        return Err(AppError::config("no paths to send"));
    }
    let semaphore = Arc::new(Semaphore::new(opts.max_parallel.max(1)));
    let mut set: JoinSet<(PathBuf, AppResult<()>)> = JoinSet::new();
    for path in opts.paths {
        let sem = semaphore.clone();
        let req = SendRequest {
            address: opts.address.clone(),
            path: path.clone(),
            chunk_size: opts.chunk_size,
            zip_compression_level: opts.zip_compression_level,
        };
        set.spawn(async move {
            let _permit = sem.acquire_owned().await.expect("semaphore closed");
            let r = send_with_resume(req).await;
            (path, r.map(|_| ()))
        });
    }

    let mut succeeded = 0usize;
    let mut failed = 0usize;
    let total = set.len();
    while let Some(res) = set.join_next().await {
        match res {
            Ok((_, Ok(()))) => succeeded += 1,
            Ok((p, Err(e))) => {
                failed += 1;
                tracing::error!("error sending '{}': {e}", p.display());
            }
            Err(e) => {
                failed += 1;
                tracing::error!("join error: {e}");
            }
        }
    }
    Ok(ClientSummary {
        total,
        succeeded,
        failed,
    })
}
