use std::path::{Path, PathBuf};
use std::time::Instant;

use indicatif::{ProgressBar, ProgressStyle};
use sha2::{Digest, Sha256};
use tokio::fs::{File, OpenOptions};
use tokio::io::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt};

use crate::error::{AppError, AppResult};
use crate::protocol::{
    Header, HeaderFlags, ACCEPT_BYTE, DECLINE_BYTE, READY_BYTE, RESUME_RESET, read_byte, read_hash,
    read_header, read_resume_offset, write_byte, write_hash, write_header, write_resume_offset,
};

const PROGRESS_TEMPLATE: &str =
    "[{elapsed_precise}] [{bar:40.cyan/blue}] {bytes}/{total_bytes} ({eta}) {msg}";

fn progress_style() -> ProgressStyle {
    ProgressStyle::default_bar()
        .template(PROGRESS_TEMPLATE)
        .expect("valid progress template")
        .progress_chars("#>-")
}

fn part_path(final_path: &Path) -> PathBuf {
    let mut s = final_path.as_os_str().to_owned();
    s.push(".part");
    PathBuf::from(s)
}

fn meta_path(final_path: &Path) -> PathBuf {
    let mut s = final_path.as_os_str().to_owned();
    s.push(".meta.json");
    PathBuf::from(s)
}

#[derive(serde::Serialize, serde::Deserialize)]
struct PartMeta {
    total_size: u64,
    received: u64,
}

pub struct SendRequest {
    pub address: String,
    pub path: PathBuf,
    pub chunk_size: usize,
    pub zip_compression_level: u8,
}

pub struct SendOutcome {
    #[allow(dead_code)]
    pub name: String,
    #[allow(dead_code)]
    pub bytes: u64,
    #[allow(dead_code)]
    pub duration: std::time::Duration,
}

fn prepare_payload(
    path: &Path,
    zip_level: u8,
) -> AppResult<(PathBuf, String, u64, HeaderFlags, Option<PathBuf>)> {
    let meta = std::fs::metadata(path)?;
    if meta.is_dir() {
        let (zip_path, size, zip_name) = crate::zip_util::zip_directory_to_path(path, zip_level)?;
        let flags = HeaderFlags::ZIP | HeaderFlags::DIRECTORY;
        Ok((zip_path.clone(), zip_name, size, flags, Some(zip_path)))
    } else {
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .ok_or_else(|| AppError::config(format!("invalid file path: {}", path.display())))?
            .to_string();
        let size = meta.len();
        Ok((path.to_path_buf(), name, size, HeaderFlags::empty(), None))
    }
}

pub struct RecvOutcome {
    #[allow(dead_code)]
    pub name: String,
    #[allow(dead_code)]
    pub path: PathBuf,
    #[allow(dead_code)]
    pub bytes: u64,
    pub verified: bool,
}

pub async fn receive_one(
    mut socket: tokio::net::TcpStream,
    output_dir: PathBuf,
    require_accept: bool,
    auto_trust: bool,
) -> AppResult<RecvOutcome> {
    let (version, header) = read_header(&mut socket).await?;
    tracing::debug!("incoming v{version} header: {:?}", header);

    if require_accept && !auto_trust {
        write_byte(&mut socket, READY_BYTE).await?;
        let decision = read_byte(&mut socket).await?;
        if decision == DECLINE_BYTE {
            return Err(AppError::Declined);
        }
        if decision != ACCEPT_BYTE {
            return Err(AppError::InvalidHeader(format!(
                "expected accept/decline, got {decision:#x}"
            )));
        }
    }

    let final_path = output_dir.join(&header.name);
    let part = part_path(&final_path);
    let meta_file = meta_path(&final_path);

    #[allow(unused_assignments)]
    let mut offset = header.resume_offset;
    if !header.is_resume() {
        if part.exists() {
            std::fs::remove_file(&part).ok();
        }
        if meta_file.exists() {
            std::fs::remove_file(&meta_file).ok();
        }
        offset = 0;
    } else if part.exists() {
        let actual = part.metadata()?.len();
        if actual < header.total_size {
            offset = actual;
            tracing::info!("resuming '{}' at offset {offset}", header.name);
        } else {
            offset = 0;
        }
    } else {
        offset = 0;
    }

    write_byte(&mut socket, ACCEPT_BYTE).await?;
    write_resume_offset(&mut socket, offset).await?;

    let mut file = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(false)
        .open(&part)
        .await?;
    if offset > 0 {
        file.seek(std::io::SeekFrom::Start(offset)).await?;
    }

    let pb = ProgressBar::new(header.total_size);
    pb.set_style(progress_style());
    pb.set_message(format!("Receiving '{}'", header.name));

    let mut hasher = Sha256::new();
    if offset > 0 {
        let mut existing = File::open(&part).await?;
        let mut buf = vec![0u8; 64 * 1024];
        loop {
            let n = existing.read(&mut buf).await?;
            if n == 0 {
                break;
            }
            hasher.update(&buf[..n]);
        }
    }

    let mut total = offset;
    let remaining = header.total_size.saturating_sub(offset);
    let mut remaining = remaining;
    let mut buf = vec![0u8; 64 * 1024];
    while remaining > 0 {
        let want = (remaining as usize).min(buf.len());
        socket.read_exact(&mut buf[..want]).await?;
        file.write_all(&buf[..want]).await?;
        hasher.update(&buf[..want]);
        total += want as u64;
        remaining -= want as u64;
        pb.inc(want as u64);

        if total % (1024 * 1024) < want as u64 {
            let meta = PartMeta {
                total_size: header.total_size,
                received: total,
            };
            let s = serde_json::to_string(&meta)?;
            std::fs::write(&meta_file, s)?;
        }
    }
    pb.finish_with_message(format!("Received '{}'", header.name));

    let calculated = hasher.finalize();
    let received_hash = read_hash(&mut socket).await?;
    let verified = calculated.as_slice() == received_hash;

    if verified {
        std::fs::rename(&part, &final_path)?;
        std::fs::remove_file(&meta_file).ok();
        tracing::info!("file '{}' verified and saved", header.name);
    } else {
        std::fs::remove_file(&part).ok();
        std::fs::remove_file(&meta_file).ok();
        tracing::warn!(
            "integrity check failed for '{}'; partial file discarded, transfer can be retried",
            header.name
        );
    }

    Ok(RecvOutcome {
        name: header.name,
        path: final_path,
        bytes: total,
        verified,
    })
}

pub async fn send_with_resume(req: SendRequest) -> AppResult<SendOutcome> {
    let (payload_path, name, total_size, flags, temp_path) =
        prepare_payload(&req.path, req.zip_compression_level)?;
    let _cleanup = TempCleanup(temp_path);

    let mut stream = tokio::net::TcpStream::connect(&req.address).await?;

    let header = Header {
        name: name.clone(),
        total_size,
        resume_offset: 0,
        flags: flags | HeaderFlags::RESUME,
    };
    write_header(&mut stream, &header).await?;

    let first = read_byte(&mut stream).await?;
    if first == READY_BYTE {
        let decision = ACCEPT_BYTE;
        write_byte(&mut stream, decision).await?;
        let accept = read_byte(&mut stream).await?;
        if accept != ACCEPT_BYTE {
            return Err(AppError::Declined);
        }
    } else if first == ACCEPT_BYTE {
    } else {
        return Err(AppError::Declined);
    }
    let server_offset = read_resume_offset(&mut stream).await?;
    let offset = if server_offset == RESUME_RESET { 0 } else { server_offset };
    if offset > total_size {
        return Err(AppError::Transfer(format!(
            "server reports offset {offset} > total {total_size}"
        )));
    }
    if offset > 0 {
        tracing::info!("resuming '{name}' at offset {offset}");
    }

    let mut hasher = Sha256::new();
    let mut file = tokio::fs::File::open(&payload_path).await?;
    file.seek(std::io::SeekFrom::Start(offset)).await?;
    let mut hashed: u64 = 0;
    let mut hash_buf = vec![0u8; 64 * 1024];
    while hashed < offset {
        let want = ((offset - hashed) as usize).min(hash_buf.len());
        file.read_exact(&mut hash_buf[..want]).await?;
        hasher.update(&hash_buf[..want]);
        hashed += want as u64;
    }

    let pb = ProgressBar::new(total_size);
    pb.set_style(progress_style());
    pb.set_message(format!("Sending '{name}'"));
    if offset > 0 {
        pb.inc(offset);
    }

    let start = Instant::now();
    let mut buf = vec![0u8; req.chunk_size];
    let mut total = offset;
    while total < total_size {
        let want = ((total_size - total) as usize).min(buf.len());
        let n = file.read(&mut buf[..want]).await?;
        if n == 0 {
            break;
        }
        stream.write_all(&buf[..n]).await?;
        hasher.update(&buf[..n]);
        total += n as u64;
        pb.inc(n as u64);
    }
    pb.finish_with_message(format!("Sent '{name}'"));

    let hash = hasher.finalize();
    let duration = start.elapsed();
    write_hash(&mut stream, &hash).await?;

    let speed = total as f64 / duration.as_secs_f64() / (1024.0 * 1024.0);
    tracing::info!("transfer of '{name}' complete in {duration:?} ({speed:.2} MB/s)", speed = speed);

    Ok(SendOutcome {
        name,
        bytes: total,
        duration,
    })
}

struct TempCleanup(Option<PathBuf>);

impl Drop for TempCleanup {
    fn drop(&mut self) {
        if let Some(p) = self.0.take() {
            let _ = std::fs::remove_file(p);
        }
    }
}
