use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

use crate::config::PROTOCOL_VERSION;
use crate::error::{AppError, AppResult};

pub const MAGIC_V1: u8 = 0x00;
pub const MAGIC_V2: u8 = PROTOCOL_VERSION;
pub const ACCEPT_BYTE: u8 = 0xAC;
pub const DECLINE_BYTE: u8 = 0xDE;
pub const READY_BYTE: u8 = 0x52;
#[allow(dead_code)]
pub const BUSY_BYTE: u8 = 0x42;

pub const RESUME_RESET: u64 = u64::MAX;

pub const MAX_NAME_LEN: u32 = 4096;
pub const MAX_TOTAL_SIZE: u64 = 1u64 << 50;

bitflags::bitflags! {
    #[derive(Debug, Clone, Copy, Default)]
    pub struct HeaderFlags: u8 {
        const ZIP       = 0b0000_0001;
        const RESUME    = 0b0000_0010;
        const DIRECTORY = 0b0000_0100;
    }
}

#[derive(Debug, Clone)]
pub struct Header {
    pub name: String,
    pub total_size: u64,
    pub resume_offset: u64,
    pub flags: HeaderFlags,
}

impl Header {
    #[allow(dead_code)]
    pub fn is_zip(&self) -> bool {
        self.flags.contains(HeaderFlags::ZIP)
    }

    pub fn is_resume(&self) -> bool {
        self.flags.contains(HeaderFlags::RESUME)
    }

    #[allow(dead_code)]
    pub fn is_directory(&self) -> bool {
        self.flags.contains(HeaderFlags::DIRECTORY)
    }
}

pub async fn detect_version<R: AsyncRead + Unpin>(r: &mut R) -> AppResult<u8> {
    let mut b = [0u8; 1];
    r.read_exact(&mut b)
        .await
        .map_err(|e| AppError::InvalidHeader(format!("could not read version byte: {e}")))?;
    Ok(b[0])
}

pub async fn write_header<W: AsyncWrite + Unpin>(w: &mut W, h: &Header) -> AppResult<()> {
    let name_bytes = h.name.as_bytes();
    if name_bytes.len() as u32 > MAX_NAME_LEN {
        return Err(AppError::InvalidHeader(format!(
            "name too long: {} bytes (max {MAX_NAME_LEN})",
            name_bytes.len()
        )));
    }
    if h.total_size > MAX_TOTAL_SIZE {
        return Err(AppError::InvalidHeader(format!(
            "total size too large: {}",
            h.total_size
        )));
    }
    w.write_u8(MAGIC_V2).await?;
    w.write_u32(name_bytes.len() as u32).await?;
    w.write_all(name_bytes).await?;
    w.write_u64(h.total_size).await?;
    w.write_u64(h.resume_offset).await?;
    w.write_u8(h.flags.bits()).await?;
    Ok(())
}

pub async fn read_header_v2<R: AsyncRead + Unpin>(r: &mut R) -> AppResult<Header> {
    let mut len_buf = [0u8; 4];
    r.read_exact(&mut len_buf)
        .await
        .map_err(|e| AppError::InvalidHeader(format!("name length: {e}")))?;
    let name_len = u32::from_be_bytes(len_buf) as usize;
    if name_len == 0 || name_len as u32 > MAX_NAME_LEN {
        return Err(AppError::InvalidHeader(format!(
            "name length out of range: {name_len}"
        )));
    }
    let mut name_buf = vec![0u8; name_len];
    r.read_exact(&mut name_buf)
        .await
        .map_err(|e| AppError::InvalidHeader(format!("name: {e}")))?;
    let name = std::str::from_utf8(&name_buf)
        .map_err(|e| AppError::Utf8 {
            field: "name",
            source: e,
        })?
        .trim()
        .to_string();

    let mut total_buf = [0u8; 8];
    r.read_exact(&mut total_buf)
        .await
        .map_err(|e| AppError::InvalidHeader(format!("total size: {e}")))?;
    let total_size = u64::from_be_bytes(total_buf);

    let mut resume_buf = [0u8; 8];
    r.read_exact(&mut resume_buf)
        .await
        .map_err(|e| AppError::InvalidHeader(format!("resume offset: {e}")))?;
    let resume_offset = u64::from_be_bytes(resume_buf);

    let mut flags_buf = [0u8; 1];
    r.read_exact(&mut flags_buf)
        .await
        .map_err(|e| AppError::InvalidHeader(format!("flags: {e}")))?;
    let flags = HeaderFlags::from_bits_truncate(flags_buf[0]);

    Ok(Header {
        name,
        total_size,
        resume_offset,
        flags,
    })
}

pub async fn read_header_v1<R: AsyncRead + Unpin>(r: &mut R) -> AppResult<Header> {
    let mut len_buf = [0u8; 4];
    r.read_exact(&mut len_buf)
        .await
        .map_err(|e| AppError::InvalidHeader(format!("v1 name length: {e}")))?;
    let name_len = u32::from_be_bytes(len_buf) as usize;
    if name_len == 0 || name_len as u32 > MAX_NAME_LEN {
        return Err(AppError::InvalidHeader(format!(
            "v1 name length out of range: {name_len}"
        )));
    }
    let mut name_buf = vec![0u8; name_len];
    r.read_exact(&mut name_buf)
        .await
        .map_err(|e| AppError::InvalidHeader(format!("v1 name: {e}")))?;
    let name = String::from_utf8_lossy(&name_buf).trim().to_string();

    let mut size_buf = [0u8; 8];
    r.read_exact(&mut size_buf)
        .await
        .map_err(|e| AppError::InvalidHeader(format!("v1 size: {e}")))?;
    let total_size = u64::from_be_bytes(size_buf);

    Ok(Header {
        name,
        total_size,
        resume_offset: 0,
        flags: HeaderFlags::empty(),
    })
}

pub async fn read_header<R: AsyncRead + Unpin>(r: &mut R) -> AppResult<(u8, Header)> {
    let version = detect_version(r).await?;
    let header = if version == MAGIC_V2 {
        read_header_v2(r).await?
    } else if version == MAGIC_V1 {
        read_header_v1(r).await?
    } else {
        return Err(AppError::InvalidHeader(format!(
            "unsupported protocol version: {version}"
        )));
    };
    Ok((version, header))
}

pub async fn write_byte<W: AsyncWrite + Unpin>(w: &mut W, b: u8) -> AppResult<()> {
    w.write_u8(b).await?;
    Ok(())
}

pub async fn read_byte<R: AsyncRead + Unpin>(r: &mut R) -> AppResult<u8> {
    let mut b = [0u8; 1];
    r.read_exact(&mut b).await?;
    Ok(b[0])
}

pub async fn read_resume_offset<R: AsyncRead + Unpin>(r: &mut R) -> AppResult<u64> {
    let mut buf = [0u8; 8];
    r.read_exact(&mut buf).await?;
    Ok(u64::from_be_bytes(buf))
}

pub async fn write_resume_offset<W: AsyncWrite + Unpin>(w: &mut W, offset: u64) -> AppResult<()> {
    w.write_u64(offset).await?;
    Ok(())
}

pub async fn read_hash<R: AsyncRead + Unpin>(r: &mut R) -> AppResult<[u8; 32]> {
    let mut buf = [0u8; 32];
    r.read_exact(&mut buf).await?;
    Ok(buf)
}

pub async fn write_hash<W: AsyncWrite + Unpin>(w: &mut W, hash: &[u8]) -> AppResult<()> {
    if hash.len() != 32 {
        return Err(AppError::InvalidHeader(format!(
            "hash must be 32 bytes, got {}",
            hash.len()
        )));
    }
    w.write_all(hash).await?;
    Ok(())
}
