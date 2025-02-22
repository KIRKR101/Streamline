# Streamline

Streamline is a simple and efficient command-line tool built with Rust for transferring files and directories over a local network. It supports both server and client modes, directory zipping for efficient transfer, progress bars, and integrity checks using SHA256 hashing.

## Features

*   **File and Directory Transfer:** Send individual files or entire directories. Directories are automatically zipped on-the-fly before sending.
*   **Server and Client Modes:** Operate as a server to receive files or as a client to send files.
*   **Configurable Chunk Size:** Optimize transfer speed by adjusting the chunk size for client transfers.
*   **Parallel Transfers:** Send multiple files concurrently from the client to speed up transfers.
*   **Zip Compression:** Directories are compressed into zip archives before client transfer to reduce bandwidth usage. Compression level is configurable.
*   **Progress Bars:**  Real-time progress bars for both file transfers and directory zipping.
*   **File Integrity Verification:** SHA256 hashing ensures file integrity during transfer.
*   **Cross-Platform Compatibility:** Works on Windows, Linux, and macOS.

## Installation

Ensure you have [Rust](https://rust-lang.org/tools/install) and Cargo installed on your system.

1.  Clone the Streamline repository:

    ```bash
    git clone https://github.com/KIRKR101/Streamline
    cd Streamline
    ```

2.  Install Streamline using Cargo:

    ```bash
    cargo install --path .
    ```

    This command compiles the project and installs the `streamline` executable to your Cargo bin directory (usually `~/.cargo/bin` or `C:\Users\YourUsername\.cargo\bin`), making it available in your command line.

## Usage

Streamline has two main modes: `server` and `client`.

### Server Mode

Start a server to listen for and receive incoming files:

```bash
streamline server [OPTIONS]
```

**Options:**

*   `-s, --address <ADDRESS>`:  Specify the address and port to listen on (e.g., `0.0.0.0:8080`, `192.168.1.100:9999`, `localhost:8081`). Default is `0.0.0.0:8080` (listening on all interfaces).
*   `-o, --output-path <PATH>`:  Specify the directory to save received files. If not specified, files are saved to the current working directory.

**Examples:**

*   Start a server listening on all interfaces at port `8080`, saving files to `/path/to/receive/directory`:

    ```bash
    streamline server --address 0.0.0.0:8080 --output-path /path/to/receive/directory
    ```

*   Start a server listening on IP `192.168.1.105` at port `9999`, saving files to the current directory:

    ```bash
    streamline server --address 192.168.1.105:9999
    ```

*   Start a server with default address and output path:

    ```bash
    streamline server
    ```

### Client Mode

Send files and directories to a Streamline server:

```bash
streamline client <ADDRESS> [OPTIONS] <FILE_PATH> [FILE_PATH]...
```

**Arguments:**

*   `<ADDRESS>`: The address and port of the server to send files to (e.g., `192.168.1.105:8080`).
*   `<FILE_PATH> [FILE_PATH]...`: One or more file or directory paths to send.

**Options:**

*   `-c, --chunk-size-mb <SIZE>`:  Set the chunk size in MB for file transfer (default: `1MB`).
*   `-p, --parallel-transfers <COUNT>`: Set the maximum number of files to transfer in parallel (default: `5`).
*   `-z, --zip-compression-level <LEVEL>`: Set the zip compression level (0-9) for directories (default: `6`). `0` is no compression (faster), `9` is maximum compression (smaller size, slower).

**Examples:**

*   Send `file1.txt` and `directory1` to a server at `192.168.1.105:8080` with default options:

    ```bash
    streamline client 192.168.1.105:8080 file1.txt directory1
    ```

*   Send `file1.txt` and `directory2` to a server at `127.0.0.1:8080` with a 2MB chunk size and maximum zip compression:

    ```bash
    streamline client 127.0.0.1:8080 -c 2 -z 9 file1.txt directory2
    ```

*   Send `file1.txt` and `another_file.mp4` to a server at `localhost:9000` with 3 parallel transfers:

    ```bash
    streamline client localhost:9000 -p 3 file1.txt another_file.mp4
    ```

### Example

![server](https://github.com/user-attachments/assets/f5429e27-2187-474a-ba5d-897854751700)
![client](https://github.com/user-attachments/assets/36f88d5d-d475-4aaa-9657-0a99e8c1e8d1)

### Limitations

*   Not optimized for extremely high-throughput environments.
*   Designed for simple TCP-based transfers, not for complex file-sharing scenarios.
*   Lacks built-in encryption and advanced security features. Use in trusted networks or with additional security measures.

Streamline has been tested on Windows and Linux, including file transfers between them, and is expected to work on macOS as well.