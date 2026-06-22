use directories::ProjectDirs;
use std::path::PathBuf;

const QUALIFIER: &str = "com";
const ORG: &str = "Streamline";
const APP: &str = "Streamline";

pub fn project_dirs() -> ProjectDirs {
    ProjectDirs::from(QUALIFIER, ORG, APP).expect("home directory is available")
}

pub fn config_dir() -> PathBuf {
    project_dirs().config_dir().to_path_buf()
}

pub fn data_dir() -> PathBuf {
    project_dirs().data_dir().to_path_buf()
}

pub fn ensure_dirs() -> std::io::Result<()> {
    std::fs::create_dir_all(config_dir())?;
    std::fs::create_dir_all(data_dir())?;
    Ok(())
}

pub fn config_file() -> PathBuf {
    config_dir().join("config.toml")
}

pub fn peers_file() -> PathBuf {
    config_dir().join("peers.toml")
}

pub fn queue_file() -> PathBuf {
    data_dir().join("queue.json")
}

pub fn log_file() -> PathBuf {
    data_dir().join("streamline.log")
}
