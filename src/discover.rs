use std::time::Duration;

use mdns_sd::{ServiceDaemon, ServiceEvent};

use crate::config::SERVICE_TYPE;
use crate::error::{AppError, AppResult};

pub async fn discover(timeout_secs: u64) -> AppResult<()> {
    let timeout = Duration::from_secs(timeout_secs);
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<String>();

    let handle = tokio::task::spawn_blocking(move || -> AppResult<()> {
        let daemon =
            ServiceDaemon::new().map_err(|e| AppError::Other(format!("mDNS daemon: {e}")))?;
        let receiver = daemon
            .browse(SERVICE_TYPE)
            .map_err(|e| AppError::Other(format!("mDNS browse: {e}")))?;
        let deadline = std::time::Instant::now() + timeout;
        while std::time::Instant::now() < deadline {
            let remaining = deadline.saturating_duration_since(std::time::Instant::now());
            match receiver.recv_timeout(remaining) {
                Ok(ServiceEvent::ServiceResolved(info)) => {
                    let name = info.get_fullname().to_string();
                    let port = info.get_port();
                    let addrs: Vec<_> = info
                        .get_addresses()
                        .iter()
                        .map(|a| a.to_string())
                        .collect();
                    if let Some(addr) = addrs.first() {
                        let _ = tx.send(format!("{name} -> {addr}:{port}"));
                    }
                }
                Ok(_) => {}
                Err(_) => break,
            }
        }
        let _ = daemon.shutdown();
        Ok(())
    });

    println!("Searching for Streamline peers ({timeout_secs}s)...");
    while let Some(line) = rx.recv().await {
        println!("{line}");
    }
    handle.await.map_err(AppError::from)?
}

pub async fn advertise(name: String, port: u16) -> AppResult<ServiceDaemon> {
    let (tx, rx) = tokio::sync::oneshot::channel::<AppResult<ServiceDaemon>>();
    tokio::task::spawn_blocking(move || {
        let res = (|| -> AppResult<ServiceDaemon> {
            let daemon =
                ServiceDaemon::new().map_err(|e| AppError::Other(format!("mDNS daemon: {e}")))?;
            let fullname = format!("{name}.{SERVICE_TYPE}");
            let info = mdns_sd::ServiceInfo::new(
                SERVICE_TYPE.trim_end_matches('.'),
                &name,
                &fullname,
                "",
                port,
                None,
            )
            .map_err(|e| AppError::Other(format!("mDNS ServiceInfo: {e}")))?;
            daemon
                .register(info)
                .map_err(|e| AppError::Other(format!("mDNS register: {e}")))?;
            Ok(daemon)
        })();
        let _ = tx.send(res);
    })
    .await
    .map_err(AppError::from)?;
    rx.await.map_err(|e| AppError::Other(format!("mDNS task: {e}")))?
}
