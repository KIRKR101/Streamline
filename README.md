# Streamline
A Rust-based command-line tool designed to simplify and automate file transfers and data streaming between clients and servers on a local area network. It supports sending and receiving files, saving files to specific directories, and handling multiple files simultaneously.

## Installation

Make sure both [Rust](https://rust-lang.org/tools/install) & Cargo (included with Rust) are installed on your system.

To install Streamline, clone the repository and use Cargo to build and install the executable:

```
git clone https://github.com/KIRKR101/Streamline
cd Streamline
cargo install --path .
```

This will compile the project and place the `streamline` executable in your Cargo bin directory, making it accessible from your command line with the command `streamline`.

## Usage

Streamline can be used in both server and client modes. Depending on the operation, you can specify file locations, directories to save files, multiple files, and network addresses including ports.

```
streamline <mode> <address:port> <directory to save to / file(s) to be sent>
```

#### Server Mode

To start a server that listens for incoming files:

```
streamline server 0.0.0.0:8080 /path/to/directory/
```

This command starts a server on all interfaces (`0.0.0.0`) at port `8080`. Incoming files will be saved to the `/path/to/directory/` directory. You can also use the private IP of the machine receiving the files (which starts the server), usually starting in 192, e.g. `streamline server 192.168.0.31:8080 /path/to/directory/`. When a directory path is not specified, the files will be sent to wherever the terminal is opened, e.g. `C:\Users\user`.

#### Client Mode

To send files to a server:

```
streamline client 192.168.0.31:8080 file1.txt file2.txt [more files...]
```

This command sends `file1.txt` and `file2.txt` to the server running at `192.168.0.31` on port `8080`. The specific address **must** be specified in this command. The source of file1.txt etc. is the location wherein the terminal is open e.g. `C:\Users\user`. If not in that location, you can pass the entire file location, e.g. `C:\Users\user\Documents\example.jpg` instead.

### Example

![server](https://github.com/user-attachments/assets/f5429e27-2187-474a-ba5d-897854751700)

![client](https://github.com/user-attachments/assets/36f88d5d-d475-4aaa-9657-0a99e8c1e8d1)

#### Limitations

- Not optimized for high-throughput scenarios.
- Designed for straightforward TCP-based transfers, not complex file-sharing protocols.
- Lacks encryption and advanced security features. It is recommended to use this in trusted networks or with additional security layers.

I have used this on Windows and Linux—even transferring files between the two. I have no reason to believe it wouldn't work equally on MacOS.
