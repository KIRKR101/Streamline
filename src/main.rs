use std::fs::File;
use std::path::{Path, PathBuf};
use std::env;
use std::time::Instant;
use std::io::{BufReader, Read};
use tokio::net::{TcpListener, TcpStream};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::fs::OpenOptions;
use tokio::sync::Semaphore;
use sha2::{Sha256, Digest};
use indicatif::{ProgressBar, ProgressStyle};
use futures::future::join_all;

const CHUNK_SIZE: usize = 1024 * 1024; // 1 MB
const MAX_PARALLEL_TRANSFERS: usize = 5;

async fn start_server(address: &str, output_path: Option<String>) -> tokio::io::Result<()> {
    let listener = TcpListener::bind(address).await?;
    println!("Server listening on {}", address);

    loop {
        let (socket, _) = listener.accept().await?;
        let output_path = output_path.clone();
        tokio::spawn(async move {
            if let Err(e) = receive_file(socket, output_path).await {
                eprintln!("Error receiving file: {}", e);
            }
        });
    }
}

async fn receive_file(mut socket: TcpStream, output_path: Option<String>) -> tokio::io::Result<()> {
    let mut file_name_buffer = vec![0; 256];
    let file_name_size = socket.read(&mut file_name_buffer).await?;
    let file_name = String::from_utf8_lossy(&file_name_buffer[..file_name_size]);
    let file_name = file_name.trim();

    let output_file_path = if let Some(path) = output_path {
        let mut path_buf = PathBuf::from(path);
        path_buf.push(file_name);
        path_buf
    } else {
        PathBuf::from(file_name)
    };

    let mut file = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(&output_file_path)
        .await?;

    let mut size_buffer = [0u8; 8];
    socket.read_exact(&mut size_buffer).await?;
    let file_size = u64::from_be_bytes(size_buffer);

    let pb = ProgressBar::new(file_size);
    pb.set_style(ProgressStyle::default_bar()
        .template("[{elapsed_precise}] [{bar:40.cyan/blue}] {bytes}/{total_bytes} ({eta})")
        .unwrap()
        .progress_chars("#>-"));

    let start_time = Instant::now();
    let mut total_bytes = 0u64;
    let mut hasher = Sha256::new();

    while total_bytes < file_size {
        let mut buffer = vec![0; CHUNK_SIZE.min((file_size - total_bytes) as usize)];
        let n = socket.read(&mut buffer).await?;
        if n == 0 {
            break;
        }
        file.write_all(&buffer[..n]).await?;
        total_bytes += n as u64;
        pb.set_position(total_bytes);
        hasher.update(&buffer[..n]);
    }

    pb.finish_with_message("Transfer complete");

    let duration = start_time.elapsed();
    let speed = total_bytes as f64 / duration.as_secs_f64() / 1024.0 / 1024.0; // MB/s
    println!("Transfer complete in {:.2?}", duration);
    println!("Average speed: {:.2} MB/s", speed);

    let calculated_hash = hasher.finalize();
    let mut received_hash = [0u8; 32];
    socket.read_exact(&mut received_hash).await?;

    if calculated_hash[..] == received_hash {
        println!("File integrity verified");
    } else {
        println!("Warning: File integrity check failed");
    }

    println!("File received and saved to {:?}", output_file_path);
    Ok(())
}

async fn send_files(address: &str, file_paths: Vec<String>) -> tokio::io::Result<()> {
    let semaphore = std::sync::Arc::new(Semaphore::new(MAX_PARALLEL_TRANSFERS));

    let transfers = file_paths.into_iter().map(|file_path| {
        let semaphore = semaphore.clone();
        let address = address.to_string();
        async move {
            let _permit = semaphore.acquire_owned().await.unwrap();
            send_file(&address, &file_path).await
        }
    });

    let results = join_all(transfers).await;
    
    for result in results {
        if let Err(e) = result {
            eprintln!("Error sending file: {}", e);
        }
    }

    Ok(())
}

async fn send_file(address: &str, file_path: &str) -> tokio::io::Result<()> {
    let mut stream = TcpStream::connect(address).await?;
    let path = Path::new(file_path);

    let file_name = path.file_name().unwrap().to_str().unwrap();
    stream.write_all(file_name.as_bytes()).await?;

    let file = File::open(file_path)?;
    let file_size = file.metadata()?.len();
    stream.write_all(&file_size.to_be_bytes()).await?;

    let mut reader = BufReader::new(file);

    let pb = ProgressBar::new(file_size);
    pb.set_style(ProgressStyle::default_bar()
        .template("[{elapsed_precise}] [{bar:40.cyan/blue}] {bytes}/{total_bytes} ({eta})")
        .unwrap()
        .progress_chars("#>-"));

    let start_time = Instant::now();
    let mut total_bytes = 0u64;
    let mut hasher = Sha256::new();

    loop {
        let mut buffer = vec![0; CHUNK_SIZE];
        let n = reader.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        stream.write_all(&buffer[..n]).await?;
        total_bytes += n as u64;
        pb.set_position(total_bytes);
        hasher.update(&buffer[..n]);
    }

    pb.finish_with_message("Transfer complete");

    let duration = start_time.elapsed();
    let speed = total_bytes as f64 / duration.as_secs_f64() / 1024.0 / 1024.0; // MB/s
    println!("Transfer complete in {:.2?}", duration);
    println!("Average speed: {:.2} MB/s", speed);

    let hash = hasher.finalize();
    stream.write_all(&hash).await?;

    println!("File integrity verified: '{}' sent to {}", file_name, address);
    Ok(())
}

#[tokio::main]
async fn main() {
    let args: Vec<String> = env::args().collect();

    if args.len() < 2 {
        eprintln!("Usage: {} [server|client] [options]", args[0]);
        return;
    }

    match args[1].as_str() {
        "server" => {
            let address = args.get(2).cloned().unwrap_or_else(|| "0.0.0.0:8080".to_string());
            let output_path = args.get(3).cloned();
            if let Err(e) = start_server(&address, output_path).await {
                eprintln!("Server error: {}", e);
            }
        }
        "client" => {
            if args.len() < 4 {
                eprintln!("Usage: {} client <address> <file_path1> [file_path2 ...]", args[0]);
                return;
            }
            let address = args[2].clone();
            let file_paths: Vec<String> = args[3..].to_vec();
            if let Err(e) = send_files(&address, file_paths).await {
                eprintln!("Client error: {}", e);
            }
        }
        _ => {
            eprintln!("Invalid mode. Use 'server' or 'client'.");
        }
    }
}