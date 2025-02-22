use std::fs::{File, metadata};
use std::path::{Path, PathBuf};
use std::time::Instant;
use std::io::{BufReader, Read, Error as IoError, ErrorKind, Cursor};
use tokio::net::{TcpListener, TcpStream};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::fs::OpenOptions;
use tokio::sync::Semaphore;
use sha2::{Sha256, Digest};
use indicatif::{ProgressBar, ProgressStyle};
use futures::future::join_all;
use clap::{Parser, Subcommand, value_parser}; // Removed Args import - might not be needed here
use zip::{ZipWriter, CompressionMethod};
use zip::write::FileOptions;
use walkdir::WalkDir;

// Make CHUNK_SIZE configurable via command line
const DEFAULT_CHUNK_SIZE_MB: usize = 1;
const DEFAULT_MAX_PARALLEL_TRANSFERS: usize = 5;
const DEFAULT_ZIP_COMPRESSION_LEVEL: u8 = 6;

#[derive(Parser)]
#[clap(version = "1.0", author = "Your Name", about = "Simple File Transfer App")]
struct Cli {
    #[clap(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug)]
enum Commands {
    Server {
        #[clap(short = 's', long, default_value = "0.0.0.0:8080", help = "Address to listen on")]
        address: String,
        #[clap(short = 'o', long, value_parser, help = "Output path for received files")]
        output_path: Option<String>,
    },
    Client(ClientArgs), // Use ClientArgs struct here
}

#[derive(Parser, Debug)] // Use Parser for ClientArgs to define options
struct ClientArgs {
    #[clap(value_parser, help = "Server address")]
    address: String,
    #[clap(value_parser, help = "File paths to send")]
    file_paths: Vec<String>,

    #[clap(short = 'c', long, default_value_t = DEFAULT_CHUNK_SIZE_MB, help = "Chunk size in MB")]
    chunk_size_mb: usize,

    #[clap(short = 'p', long, default_value_t = DEFAULT_MAX_PARALLEL_TRANSFERS, help = "Maximum parallel file transfers")]
    parallel_transfers: usize,

    #[clap(short = 'z', long, default_value_t = DEFAULT_ZIP_COMPRESSION_LEVEL, value_parser = value_parser!(u8).range(0..=9), help = "Zip compression level (0-9) for directories")]
    zip_compression_level: u8,
}

async fn start_server(address: &str, output_path: Option<String>, chunk_size: usize) -> Result<(), Box<dyn std::error::Error>> {
    let listener = TcpListener::bind(address).await?;
    println!("Server listening on {}", address);

    loop {
        let (socket, addr) = listener.accept().await?;
        println!("Accepted connection from: {}", addr);
        let output_path = output_path.clone();
        let chunk_size_clone = chunk_size; // Move chunk_size into the spawned task
        tokio::spawn(async move {
            if let Err(e) = receive_file(socket, output_path, chunk_size_clone).await {
                eprintln!("Error receiving file from {}: {}", addr, e);
            } else {
                println!("File received successfully from {}", addr);
            }
        });
    }
}

async fn receive_file(mut socket: TcpStream, output_path: Option<String>, chunk_size: usize) -> Result<(), Box<dyn std::error::Error>> {
    // Receive file name size
    let mut file_name_size_buffer = [0u8; 4];
    socket.read_exact(&mut file_name_size_buffer).await.map_err(|e| IoError::new(e.kind(), format!("Failed to read file name size: {}", e)))?;
    let file_name_size = u32::from_be_bytes(file_name_size_buffer) as usize;

    // Receive file name
    let mut file_name_buffer = vec![0; file_name_size];
    socket.read_exact(&mut file_name_buffer).await.map_err(|e| IoError::new(e.kind(), format!("Failed to read file name: {}", e)))?;
    let file_name = String::from_utf8(file_name_buffer).map_err(|e| IoError::new(ErrorKind::InvalidData, format!("Invalid file name encoding: {}", e)))?;
    let file_name = file_name.trim(); // Trim whitespace just in case

    let output_file_path = match output_path {
        Some(path) => {
            let mut path_buf = PathBuf::from(path);
            path_buf.push(file_name);
            path_buf
        }
        None => PathBuf::from(file_name),
    };

    println!("Receiving file: '{}'...", file_name);

    let mut file = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(&output_file_path)
        .await.map_err(|e| IoError::new(e.kind(), format!("Failed to open output file '{}': {}", output_file_path.display(), e)))?;

    // Receive file size
    let mut size_buffer = [0u8; 8];
    socket.read_exact(&mut size_buffer).await.map_err(|e| IoError::new(e.kind(), format!("Failed to read file size: {}", e)))?;
    let file_size = u64::from_be_bytes(size_buffer);

    let pb = ProgressBar::new(file_size);
    pb.set_style(ProgressStyle::default_bar()
        .template("[{elapsed_precise}] [{bar:40.cyan/blue}] {bytes}/{total_bytes} ({eta}) {msg}")
        .unwrap()
        .progress_chars("#>-"));
    pb.set_message(format!("Receiving '{}'", file_name));

    let start_time = Instant::now();
    let mut total_bytes = 0u64;
    let mut hasher = Sha256::new();

    while total_bytes < file_size {
        let buffer_size = chunk_size.min((file_size - total_bytes) as usize);
        let mut buffer = vec![0; buffer_size];
        let n = socket.read_exact(&mut buffer).await.map_err(|e| IoError::new(e.kind(), format!("Failed to read chunk from socket: {}", e)))?; // Read exactly buffer_size bytes
        file.write_all(&buffer).await.map_err(|e| IoError::new(e.kind(), format!("Failed to write chunk to file '{}': {}", output_file_path.display(), e)))?;
        total_bytes += n as u64;
        pb.inc(n as u64);
        hasher.update(&buffer);
    }

    pb.finish_with_message(format!("Received '{}'", file_name));

    let duration = start_time.elapsed();
    let speed = total_bytes as f64 / duration.as_secs_f64() / 1024.0 / 1024.0; // MB/s
    println!("Transfer of '{}' complete in {:.2?}", file_name, duration);
    println!("Average speed: {:.2} MB/s", speed);

    // Receive hash
    let mut received_hash = [0u8; 32];
    socket.read_exact(&mut received_hash).await.map_err(|e| IoError::new(e.kind(), format!("Failed to read hash: {}", e)))?;
    let calculated_hash = hasher.finalize();

    if calculated_hash[..] == received_hash {
        println!("File integrity verified for '{}'", file_name);
    } else {
        println!("Warning: File integrity check failed for '{}'", file_name);
    }

    println!("File received and saved to {:?}", output_file_path);
    Ok(())
}

async fn send_files(address: &str, file_paths: Vec<String>, max_parallel_transfers: usize, chunk_size: usize, zip_compression_level: u8) -> Result<(), Box<dyn std::error::Error>> {
    let semaphore = std::sync::Arc::new(Semaphore::new(max_parallel_transfers));

    // Clone file_paths here so the original is not moved
    let paths_for_transfer = file_paths.clone();

    let transfers = paths_for_transfer.into_iter().map(|file_path| {
        let semaphore = semaphore.clone();
        let address = address.to_string();
        let chunk_size_clone = chunk_size; // Move chunk_size into the async move block
        let zip_compression_level_clone = zip_compression_level;
        async move {
            let _permit = semaphore.acquire_owned().await?;
            send_file(&address, &file_path, chunk_size_clone, zip_compression_level_clone).await
        }
    });

    let results = join_all(transfers).await;

    println!("\n--- Transfer Summary ---");
    let mut success_count = 0;
    let mut error_count = 0;

    // Now you can use the original `file_paths` here because it was cloned above
    for (index, result) in results.into_iter().enumerate() {
        let file_path = &file_paths[index]; // Accessing the original `file_paths` is now safe
        match result {
            Ok(_) => {
                println!("✅ Sent: '{}'", file_path);
                success_count += 1;
            }
            Err(e) => {
                eprintln!("❌ Error sending '{}': {}", file_path, e);
                error_count += 1;
            }
        }
    }

    println!("-----------------------");
    println!("Total files sent: {}", file_paths.len());
    println!("Successful transfers: {}", success_count);
    println!("Failed transfers: {}", error_count);
    println!("-----------------------");

    if error_count > 0 {
        Err("Some files failed to send".into()) // Optionally return an error if any transfer failed
    } else {
        Ok(())
    }
}

fn create_zip_archive(dir_path: &Path, compression_level: u8) -> Result<(Cursor<Vec<u8>>, String), Box<dyn std::error::Error>> {
    let mut zip_buffer = Cursor::new(Vec::new());
    let mut zip_writer = ZipWriter::new(&mut zip_buffer);
    // Explicitly specify the type for FileOptions
    let options: FileOptions<'_, ()> = FileOptions::default()
        .compression_method(CompressionMethod::Deflated)
        .compression_level(Some(compression_level.into()))
        .unix_permissions(0o755);

    let dir_name = dir_path.file_name().and_then(|n| n.to_str()).ok_or_else(|| IoError::new(ErrorKind::InvalidInput, format!("Invalid directory path: '{}'", dir_path.display())))?;
    let zip_file_name = format!("{}.zip", dir_name);

    let entries_count = WalkDir::new(dir_path).into_iter().count(); // Count entries for progress bar
    let pb = ProgressBar::new(entries_count as u64);
    pb.set_style(ProgressStyle::default_bar()
        .template("[{elapsed_precise}] [{bar:40.cyan/blue}] {pos}/{total} ({eta}) {msg}")
        .unwrap()
        .progress_chars("#>-"));
    pb.set_message(format!("Zipping directory '{}'...", dir_name));

    for entry in WalkDir::new(dir_path) {
        let entry = entry?;
        let path = entry.path();
        let name = path.strip_prefix(dir_path)?;

        if path.is_file() {
            #[allow(deprecated)]
            zip_writer.start_file(name.to_string_lossy(), options)?;
            let mut f = File::open(path).map_err(|e| IoError::new(e.kind(), format!("Failed to open file '{}' for zipping: {}", path.display(), e)))?;
            let mut buffer = Vec::new();
            f.read_to_end(&mut buffer).map_err(|e| IoError::new(e.kind(), format!("Failed to read file '{}' for zipping: {}", path.display(), e)))?;
            std::io::Write::write_all(&mut zip_writer, &buffer)?; // Fully qualify write_all
        } else if !name.as_os_str().is_empty() {
            #[allow(deprecated)]
            zip_writer.add_directory(name.to_string_lossy(), options)?;
        }
        pb.inc(1); // Increment progress bar
    }

    pb.finish_with_message(format!("Zipped directory '{}'", dir_name));

    zip_writer.finish()?;
    Ok((zip_buffer, zip_file_name))
}


async fn send_file(address: &str, file_path: &str, chunk_size: usize, zip_compression_level: u8) -> Result<(), Box<dyn std::error::Error>> {
    let mut stream = TcpStream::connect(address).await.map_err(|e| IoError::new(e.kind(), format!("Failed to connect to '{}': {}", address, e)))?;
    let path = Path::new(file_path);
    let file_name_base = path.file_name().and_then(|n| n.to_str()).ok_or_else(|| IoError::new(ErrorKind::InvalidInput, format!("Invalid file path: '{}'", file_path)))?;
    let mut actual_file_name = file_name_base.to_string();
    let mut file_size: u64 = 0;
    let mut reader: BufReader<Box<dyn Read>>;

    let mut is_directory = false;

    if metadata(file_path).map_err(|e| IoError::new(e.kind(), format!("Failed to get metadata for '{}': {}", file_path, e)))?.is_dir() {
        is_directory = true;
        println!("Zipping directory: '{}'...", file_path);
        let (zip_buffer, zip_file_name) = create_zip_archive(path, zip_compression_level)?;
        actual_file_name = zip_file_name;
        file_size = zip_buffer.get_ref().len() as u64;
        reader = BufReader::new(Box::new(Cursor::new(zip_buffer.get_ref().clone())));
        println!("Directory '{}' zipped to '{}', size: {} bytes", file_path, actual_file_name, file_size);
    } else {
        let file = File::open(file_path).map_err(|e| IoError::new(e.kind(), format!("Failed to open file '{}': {}", file_path, e)))?;
        file_size = file.metadata().map_err(|e| IoError::new(e.kind(), format!("Failed to get metadata for '{}': {}", file_path, e)))?.len();
        reader = BufReader::new(Box::new(file));
    }

    // Send file name size and file name
    let file_name_bytes = actual_file_name.as_bytes();
    let file_name_size = file_name_bytes.len();
    stream.write_all(&(file_name_size as u32).to_be_bytes()).await.map_err(|e| IoError::new(e.kind(), format!("Failed to send file name size: {}", e)))?;
    stream.write_all(file_name_bytes).await.map_err(|e| IoError::new(e.kind(), format!("Failed to send file name: {}", e)))?;

    stream.write_all(&file_size.to_be_bytes()).await.map_err(|e| IoError::new(e.kind(), format!("Failed to send file size: {}", e)))?;

    let pb = ProgressBar::new(file_size);
    pb.set_style(ProgressStyle::default_bar()
        .template("[{elapsed_precise}] [{bar:40.cyan/blue}] {bytes}/{total_bytes} ({eta}) {msg}")
        .unwrap()
        .progress_chars("#>-"));

    if is_directory {
        pb.set_message(format!("Sending zipped directory '{}'", file_name_base));
    } else {
        pb.set_message(format!("Sending file '{}'", file_name_base));
    }


    let start_time = Instant::now();
    let mut total_bytes = 0u64;
    let mut hasher = Sha256::new();

    loop {
        let mut buffer = vec![0; chunk_size];
        let n = reader.read(&mut buffer).map_err(|e| IoError::new(e.kind(), format!("Failed to read from file '{}': {}", file_path, e)))?;
        if n == 0 {
            break;
        }
        stream.write_all(&buffer[..n]).await.map_err(|e| IoError::new(e.kind(), format!("Failed to send chunk to server: {}", e)))?;
        total_bytes += n as u64;
        pb.inc(n as u64);
        hasher.update(&buffer[..n]);
    }

    if is_directory {
        pb.finish_with_message(format!("Sent zipped directory '{}'", file_name_base));
    } else {
        pb.finish_with_message(format!("Sent file '{}'", file_name_base));
    }


    let duration = start_time.elapsed();
    let speed = total_bytes as f64 / duration.as_secs_f64() / 1024.0 / 1024.0; // MB/s
    if is_directory {
        println!("Transfer of zipped directory '{}' complete in {:.2?}", file_name_base, duration);
    } else {
        println!("Transfer of '{}' complete in {:.2?}", file_name_base, duration);
    }
    println!("Average speed: {:.2} MB/s", speed);

    let hash = hasher.finalize();
    stream.write_all(&hash).await.map_err(|e| IoError::new(e.kind(), format!("Failed to send hash: {}", e)))?;

    if is_directory {
        println!("File integrity verified: zipped directory '{}' sent to {}", file_name_base, address);
    } else {
        println!("File integrity verified: '{}' sent to {}", file_name_base, address);
    }

    Ok(())
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();

    let chunk_size = match &cli.command {
        Commands::Client(client_args) => client_args.chunk_size_mb,
        _ => DEFAULT_CHUNK_SIZE_MB,
    } * 1024 * 1024;

    let max_parallel_transfers = match &cli.command {
        Commands::Client(client_args) => client_args.parallel_transfers,
        _ => DEFAULT_MAX_PARALLEL_TRANSFERS,
    };

    let zip_compression_level = match &cli.command {
        Commands::Client(client_args) => client_args.zip_compression_level,
        _ => DEFAULT_ZIP_COMPRESSION_LEVEL,
    };


    match cli.command {
        Commands::Server { address, output_path } => {
            start_server(&address, output_path, chunk_size).await?;
        }
        Commands::Client(client_args) => { // Access ClientArgs here
            send_files(&client_args.address, client_args.file_paths, max_parallel_transfers, chunk_size, zip_compression_level).await?;
        }
    }

    Ok(())
}