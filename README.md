# Streamline

Resumable, bidirectional file transfer with a system-tray UI, mDNS discovery, and an address book for named peers.

## What's new in v0.2

* **System tray** — keep the server running quietly with a tray icon and menu (Send file..., Open output folder, Quit).
* **Send with Streamline** — register an OS shell entry so right-click → Send just works.
* **Address book** — `streamline pair <name> <host:port>` saves peers; `streamline client <name> ...` resolves them.
* **mDNS discovery** — `streamline discover` lists servers on the LAN with zero config.
* **Bidirectional transfers** — `require-accept` mode plus trusted peers, desktop notifications on incoming.
* **Resume on disconnect** — server keeps `<name>.part` + `<name>.meta.json`; clients continue from the last offset.
* **Multi-file queue** — persistent `~/.streamline/queue.json`; tray surface for active transfers.
* **Refactored** — single 370-line `main.rs` split into 12 modules with a typed `AppError`, idiomatic Rust, and `clippy -D warnings` clean.

## Features

* File and directory transfer (directories auto-zipped with configurable compression).
* Server and client modes over plain TCP.
* Configurable chunk size and parallel transfers.
* SHA-256 integrity verification.
* v1 protocol kept for back-compat; v2 adds resume + flags.
* Cross-platform: Windows, Linux, macOS (tray + discovery on each).

## Installation

```bash
git clone https://github.com/KIRKR101/Streamline
cd Streamline
cargo install --path .
```

## Usage

```
streamline <COMMAND>

Commands:
  server          Run a server (with tray + mDNS by default)
  client          Send files/dirs to a server
  pair            Save a peer by name and address
  peers           List, remove, or update trust
  queue           Show or clear the transfer queue
  discover        Browse for peers on the LAN
  install         Register "Send with Streamline" in the OS shell
  uninstall       Remove the shell entry
  send-via-shell  Internal: invoked by the OS shell entry
```

### Server

```bash
streamline server -s 0.0.0.0:8080 -o ./received
streamline server --require-accept            # prompt for each incoming transfer
streamline server --no-advertise              # disable mDNS
```

The server runs in the foreground and shows a tray icon. Quit via the tray menu or Ctrl-C.

### Client

```bash
streamline client 192.168.1.105:8080 file.txt dir/
streamline client my-laptop file.txt           # by saved peer name
streamline client 192.168.1.105:8080 -c 2 -p 8 -z 9 file.txt
```

### Pairing

```bash
streamline pair my-laptop 192.168.1.10:8080 --trust
streamline peers list
streamline peers trust my-laptop              # mark trusted (auto-accept)
streamline peers remove my-laptop
```

### Discovery

```bash
streamline discover --timeout 5
# my-laptop._streamline._tcp.local. -> 192.168.1.10:8080
```

### Shell integration

```bash
streamline install --peer my-laptop
# right-click any file -> "Send with Streamline"
streamline uninstall
```

The server address book and config live in `~/.streamline/` (or `%APPDATA%\Streamline\Streamline\` on Windows).

## Protocol

v2 framed binary protocol (v1 is also accepted):

```
[1B version=2][4B name_len][name][8B total_size][8B resume_offset]
[1B flags: zip|resume|directory][payload...][32B sha256]
```

After the client sends the header, the server replies with `[1B accept/decline][8B resume_offset]`. `RESUME_RESET` (u64::MAX) tells the client to start from the beginning.

## Limitations

* No TLS / auth — use on a trusted network.
* mDNS / tray require the `discovery` and `tray` features (on by default).
* Resume is per-target filename; renaming a partial file on the receiver breaks resume.
* macOS shell integration is unimplemented (use the tray or CLI for now).
