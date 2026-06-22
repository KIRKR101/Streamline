#[cfg(target_os = "linux")]
use std::path::PathBuf;

use crate::error::{AppError, AppResult};

pub fn install(default_peer: Option<String>) -> AppResult<()> {
    let exe = std::env::current_exe()?;
    let exe_str = exe.to_string_lossy().to_string();
    let peer_arg = default_peer.unwrap_or_else(|| "default".to_string());

    #[cfg(target_os = "windows")]
    {
        install_windows(&exe_str, &peer_arg)
    }
    #[cfg(target_os = "linux")]
    {
        install_linux(&exe_str, &peer_arg)
    }
    #[cfg(target_os = "macos")]
    {
        install_macos(&exe_str, &peer_arg)
    }
    #[cfg(not(any(target_os = "windows", target_os = "linux", target_os = "macos")))]
    {
        Err(AppError::other(
            "OS shell integration not supported on this platform",
        ))
    }
}

pub fn uninstall() -> AppResult<()> {
    #[cfg(target_os = "windows")]
    {
        uninstall_windows()
    }
    #[cfg(target_os = "linux")]
    {
        uninstall_linux()
    }
    #[cfg(target_os = "macos")]
    {
        uninstall_macos()
    }
    #[cfg(not(any(target_os = "windows", target_os = "linux", target_os = "macos")))]
    {
        Err(AppError::other(
            "OS shell integration not supported on this platform",
        ))
    }
}

#[cfg(target_os = "windows")]
fn install_windows(exe: &str, peer_arg: &str) -> AppResult<()> {
    use std::process::Command;
    let cmd = format!("\"{exe}\" send-via-shell --peer {peer_arg} \"%1\"");
    let base = r"HKCU\Software\Classes\*\shell\Streamline.Send";
    let cmd_key = format!(r"{base}\command");
    let status = Command::new("reg")
        .args(["add", base, "/ve", "/d", "Send with Streamline", "/f"])
        .status()
        .map_err(|e| AppError::Other(format!("reg add: {e}")))?;
    if !status.success() {
        return Err(AppError::other("reg add (Streamline.Send) failed"));
    }
    let status = Command::new("reg")
        .args(["add", &cmd_key, "/ve", "/d", &cmd, "/f"])
        .status()
        .map_err(|e| AppError::Other(format!("reg add: {e}")))?;
    if !status.success() {
        return Err(AppError::other("reg add (command) failed"));
    }
    Ok(())
}

#[cfg(target_os = "windows")]
fn uninstall_windows() -> AppResult<()> {
    use std::process::Command;
    let base = r"HKCU\Software\Classes\*\shell\Streamline.Send";
    let status = Command::new("reg")
        .args(["delete", base, "/f"])
        .status()
        .map_err(|e| AppError::Other(format!("reg delete: {e}")))?;
    if !status.success() {
        return Err(AppError::other("reg delete failed"));
    }
    Ok(())
}

#[cfg(target_os = "linux")]
fn install_linux(exe: &str, peer_arg: &str) -> AppResult<()> {
    let home = std::env::var("HOME").map_err(|_| AppError::other("HOME not set"))?;
    let apps: PathBuf = [home.as_str(), ".local", "share", "applications"]
        .iter()
        .collect();
    std::fs::create_dir_all(&apps)?;
    let desktop = apps.join("streamline-send.desktop");
    let body = format!(
        "[Desktop Entry]\nType=Application\nName=Send with Streamline\nExec=\"{exe}\" send-via-shell --peer {peer_arg} %f\nMimeType=*/*;\nIcon=mail-send\nTerminal=false\nCategories=Network;FileTransfer;\n"
    );
    std::fs::write(desktop, body)?;
    Ok(())
}

#[cfg(target_os = "linux")]
fn uninstall_linux() -> AppResult<()> {
    let home = std::env::var("HOME").map_err(|_| AppError::other("HOME not set"))?;
    let desktop: PathBuf = [
        home.as_str(),
        ".local",
        "share",
        "applications",
        "streamline-send.desktop",
    ]
    .iter()
    .collect();
    if desktop.exists() {
        std::fs::remove_file(desktop)?;
    }
    Ok(())
}

#[cfg(target_os = "macos")]
fn install_macos(_exe: &str, _peer_arg: &str) -> AppResult<()> {
    Err(AppError::other(
        "macOS shell integration requires a .app bundle (TODO). Run `streamline send` from the CLI for now.",
    ))
}

#[cfg(target_os = "macos")]
fn uninstall_macos() -> AppResult<()> {
    Err(AppError::other(
        "macOS shell integration uninstall is a no-op (TODO).",
    ))
}
