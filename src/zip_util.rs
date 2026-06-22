use std::fs::File;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use walkdir::WalkDir;
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipWriter};

use crate::error::{AppError, AppResult};

pub fn zip_directory_to_path(
    dir_path: &Path,
    compression_level: u8,
) -> AppResult<(PathBuf, u64, String)> {
    let dir_name = dir_path
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| {
            AppError::config(format!("invalid directory path: {}", dir_path.display()))
        })?;
    let zip_file_name = format!("{dir_name}.zip");
    let mut tmp = std::env::temp_dir();
    tmp.push(format!("streamline-{}.zip", uuid::Uuid::new_v4()));
    let file = File::create(&tmp)?;
    let mut zip_writer = ZipWriter::new(file);

    let options = SimpleFileOptions::default()
        .compression_method(CompressionMethod::Deflated)
        .compression_level(Some(compression_level.into()))
        .unix_permissions(0o755);

    let entries: Vec<_> = WalkDir::new(dir_path).into_iter().collect();
    for entry in entries {
        let entry = entry.map_err(|e| AppError::Other(format!("walkdir: {e}")))?;
        let path = entry.path();
        let name = path
            .strip_prefix(dir_path)
            .map_err(|e| AppError::Other(format!("strip_prefix: {e}")))?;
        let name_str = name.to_string_lossy().replace('\\', "/");

        if path.is_file() {
            zip_writer.start_file(&name_str, options)?;
            let mut f = File::open(path)?;
            let mut buffer = Vec::new();
            f.read_to_end(&mut buffer)?;
            zip_writer.write_all(&buffer)?;
        } else if !name.as_os_str().is_empty() {
            zip_writer.add_directory(&name_str, options)?;
        }
    }
    zip_writer.finish()?;
    let size = std::fs::metadata(&tmp)?.len();
    Ok((tmp, size, zip_file_name))
}
