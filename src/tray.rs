use std::path::PathBuf;
use std::sync::Arc;

use tokio::sync::{Mutex, Notify};
use tray_icon::menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use tray_icon::{Icon, TrayIconBuilder};

use crate::config::{Config, Direction, Queue, QueueItem, QueueStatus};
use crate::error::AppResult;

#[derive(Debug, Clone)]
pub enum TrayCommand {
    OpenOutputFolder,
    SendFile,
    SendFolder,
    ShowStatus,
    Quit,
}

pub fn spawn_event_loop(
    handle_tx: tokio::sync::mpsc::UnboundedSender<TrayCommand>,
) -> AppResult<()> {
    std::thread::Builder::new()
        .name("streamline-tray".into())
        .spawn(move || -> AppResult<()> {
            let icon = build_icon()?;
            let menu = Menu::new();
            let status = MenuItem::with_id("status", "Streamline running", false, None);
            let open = MenuItem::with_id("open_output", "Open output folder", true, None);
            let send_file = MenuItem::with_id("send_file", "Send file...", true, None);
            let send_folder = MenuItem::with_id("send_folder", "Send folder...", true, None);
            let quit = MenuItem::with_id("quit", "Quit", true, None);

            menu.append_items(&[
                &status,
                &PredefinedMenuItem::separator(),
                &open,
                &send_file,
                &send_folder,
                &PredefinedMenuItem::separator(),
                &quit,
            ])?;

            let _tray = TrayIconBuilder::new()
                .with_menu(Box::new(menu))
                .with_tooltip("Streamline")
                .with_icon(icon)
                .build()?;

            MenuEvent::set_event_handler(Some(move |event: MenuEvent| {
                let cmd = match event.id.as_ref() {
                    "open_output" => Some(TrayCommand::OpenOutputFolder),
                    "send_file" => Some(TrayCommand::SendFile),
                    "send_folder" => Some(TrayCommand::SendFolder),
                    "show_status" => Some(TrayCommand::ShowStatus),
                    "quit" => Some(TrayCommand::Quit),
                    _ => None,
                };
                if let Some(c) = cmd {
                    let _ = handle_tx.send(c);
                }
            }));

            loop {
                std::thread::park_timeout(std::time::Duration::from_secs(60 * 60 * 24));
            }
        })
        .map_err(|e| crate::error::AppError::Other(format!("tray thread: {e}")))?;
    Ok(())
}

fn build_icon() -> AppResult<Icon> {
    let size = 32u32;
    let mut buf = vec![0u8; (size * size * 4) as usize];
    for y in 0..size {
        for x in 0..size {
            let i = ((y * size + x) * 4) as usize;
            buf[i] = 0x4A;
            buf[i + 1] = 0x90;
            buf[i + 2] = 0xE2;
            buf[i + 3] = 0xFF;
            let dx = x as i32 - size as i32 / 2;
            let dy = y as i32 - size as i32 / 2;
            if (dx * dx + dy * dy) > (size as i32 / 2 - 2).pow(2) {
                buf[i + 3] = 0;
            }
        }
    }
    Icon::from_rgba(buf, size, size)
        .map_err(|e| crate::error::AppError::Other(format!("icon: {e}")))
}

pub async fn run_event_loop(
    cfg: Arc<Mutex<Config>>,
    mut cmd_rx: tokio::sync::mpsc::UnboundedReceiver<TrayCommand>,
    output_dir: PathBuf,
    shutdown_tx: tokio::sync::watch::Sender<bool>,
) -> AppResult<()> {
    let queue_notify = Arc::new(Notify::new());
    spawn_queue_worker(cfg.clone(), queue_notify.clone());

    while let Some(cmd) = cmd_rx.recv().await {
        match cmd {
            TrayCommand::OpenOutputFolder => {
                let _ = open_in_os(&output_dir);
            }
            TrayCommand::SendFile => {
                if let Some((path, peer)) = pick_path_and_peer(false).await {
                    enqueue_send(path, peer);
                    queue_notify.notify_one();
                }
            }
            TrayCommand::SendFolder => {
                if let Some((path, peer)) = pick_path_and_peer(true).await {
                    enqueue_send(path, peer);
                    queue_notify.notify_one();
                }
            }
            TrayCommand::ShowStatus => {
                tracing::info!("tray status requested");
            }
            TrayCommand::Quit => {
                let _ = shutdown_tx.send(true);
                return Ok(());
            }
        }
    }
    Ok(())
}

async fn pick_path_and_peer(dir: bool) -> Option<(PathBuf, String)> {
    let title = if dir { "Choose a folder to send" } else { "Choose a file to send" };
    let task = rfd::AsyncFileDialog::new().set_title(title);
    let handle = if dir { task.pick_folder().await } else { task.pick_file().await }?;
    let path = handle.path().to_path_buf();
    let peer = pick_peer().await?;
    Some((path, peer))
}

async fn pick_peer() -> Option<String> {
    let book = crate::config::PeerBook::load().ok()?;
    if book.peers.is_empty() {
        tracing::warn!("no saved peers; configure a peer before sending");
        return None;
    }
    let names: Vec<String> = book.peers.iter().map(|p| p.name.clone()).collect();
    let mut task = rfd::AsyncFileDialog::new().set_title("Choose a peer");
    for n in &names {
        task = task.add_filter(n.clone(), std::slice::from_ref(n));
    }
    let handle = task.pick_file().await?;
    let chosen = handle.file_name().to_string();
    if names.contains(&chosen) {
        Some(chosen)
    } else {
        None
    }
}

fn enqueue_send(path: PathBuf, peer: String) {
    let mut queue = Queue::load().unwrap_or_default();
    let item = QueueItem {
        id: uuid::Uuid::new_v4().to_string(),
        direction: Direction::Send,
        peer,
        path: path.to_string_lossy().to_string(),
        size: std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0),
        transferred: 0,
        status: QueueStatus::Queued,
        error: None,
        created_at: chrono::Local::now().to_rfc3339(),
    };
    if let Err(e) = queue.enqueue(item) {
        tracing::error!("failed to enqueue '{}': {e}", path.display());
        return;
    }
    tracing::info!("queued '{}' for sending", path.display());
}

fn spawn_queue_worker(cfg: Arc<Mutex<Config>>, notify: Arc<Notify>) {
    tokio::spawn(async move {
        loop {
            notify.notified().await;
            process_queue(&cfg).await;
        }
    });
}

async fn process_queue(cfg: &Arc<Mutex<Config>>) {
    let pending: Vec<(String, String, String)> = match Queue::load() {
        Ok(q) => q
            .items
            .iter()
            .filter(|i| i.status == QueueStatus::Queued && i.direction == Direction::Send)
            .map(|i| (i.id.clone(), i.peer.clone(), i.path.clone()))
            .collect(),
        Err(e) => {
            tracing::error!("queue load: {e}");
            return;
        }
    };

    for (id, peer, path) in pending {
        set_status(&id, QueueStatus::Active, None);

        let (max_parallel, chunk_size, zip_level) = {
            let cfg_guard = cfg.lock().await;
            (
                cfg_guard.max_parallel_transfers.max(1),
                cfg_guard.chunk_size_bytes(),
                cfg_guard.zip_compression_level,
            )
        };

        let address = match resolve_peer(&peer) {
            Some(a) => a,
            None => {
                set_status(&id, QueueStatus::Failed, Some(format!("unknown peer: {peer}")));
                continue;
            }
        };

        let opts = crate::client::ClientOptions {
            address,
            paths: vec![PathBuf::from(&path)],
            max_parallel,
            chunk_size,
            zip_compression_level: zip_level,
        };

        match crate::client::run(opts).await {
            Ok(s) if s.failed == 0 => {
                set_status(&id, QueueStatus::Done, None);
            }
            Ok(s) => {
                set_status(
                    &id,
                    QueueStatus::Failed,
                    Some(format!("{}/{} failed", s.failed, s.total)),
                );
            }
            Err(e) => {
                set_status(&id, QueueStatus::Failed, Some(e.to_string()));
            }
        }
    }
}

fn set_status(id: &str, status: QueueStatus, error: Option<String>) {
    let mut queue = match Queue::load() {
        Ok(q) => q,
        Err(e) => {
            tracing::error!("queue load: {e}");
            return;
        }
    };
    if let Some(item) = queue.items.iter_mut().find(|i| i.id == id) {
        item.status = status;
        item.error = error;
    }
    let _ = queue.save();
}

fn resolve_peer(name: &str) -> Option<String> {
    let book = crate::config::PeerBook::load().ok()?;
    book.get(name).map(|p| p.address.clone())
}

fn open_in_os(path: &std::path::Path) -> std::io::Result<()> {
    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("explorer").arg(path).spawn()?;
    }
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open").arg(path).spawn()?;
    }
    #[cfg(target_os = "linux")]
    {
        std::process::Command::new("xdg-open").arg(path).spawn()?;
    }
    Ok(())
}

#[allow(dead_code)]
pub fn notify_incoming(name: &str, from: &str) {
    let _ = notify_rust::Notification::new()
        .summary("Streamline: incoming transfer")
        .body(&format!("{from} wants to send '{name}'"))
        .show();
}
