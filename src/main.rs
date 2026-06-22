mod cli;
mod client;
mod config;
mod discover;
mod error;
mod paths;
mod protocol;
mod runtime;
mod server;
mod shell;
#[cfg(feature = "tray")]
mod tray;
mod transfer;
mod zip_util;

use clap::Parser;
use tracing_subscriber::EnvFilter;

fn main() -> error::AppResult<()> {
    paths::ensure_dirs().ok();

    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("streamline=info,warn"));

    let log_path = paths::log_file();
    if let Ok(file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)
    {
        let (non_blocking, _guard) = tracing_appender::non_blocking(file);
        tracing_subscriber::fmt()
            .with_env_filter(filter)
            .with_writer(non_blocking)
            .with_ansi(false)
            .init();
    } else {
        tracing_subscriber::fmt()
            .with_env_filter(filter)
            .with_writer(std::io::stderr)
            .init();
    }

    let args = cli::Cli::parse();
    if args.verbose {
        tracing::info!("verbose mode enabled");
    }
    if let Err(e) = cli::run(args) {
        eprintln!("error: {e}");
        return Err(e);
    }
    Ok(())
}
