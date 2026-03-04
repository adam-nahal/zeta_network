# Hole Punching (zeta9)

A Rust-based **NAT hole punching** tool that enables direct peer-to-peer TCP connections between devices behind NATs (Network Address Translation), using a relay server for peer discovery.

## Overview

This project implements TCP hole punching, a technique that allows two peers behind different NATs to establish a direct connection without requiring a permanent relay server for data transfer. The relay server is only used for initial peer discovery.

## Features

- **Three operational modes**: Relay, Listen, and Dial
- **NAT type detection** using STUN protocol
- **Async networking** with Tokio runtime
- **Socket reuse** (`SO_REUSEADDR`) for efficient binding
- **Automatic public IP detection** for relay servers

## Architecture

```
┌─────────────┐         ┌─────────────┐
│  Dial Peer  │────────▶│   Relay     │
│ (initiator) │         │   Server    │
└─────────────┘         └──────┬──────┘
       ▲                       │
       │                       │
       └───────────────────────┘
               │
       (peer discovery)
               │
       ┌───────▼───────┐
       │  Listen Peer  │
       │   (receiver)  │
       └───────────────┘

After discovery: Direct P2P connection (hole punch)
```

## Installation

### Prerequisites

- Rust 1.70+ (Edition 2021)
- Cargo package manager

### Build from Source

```bash
git clone https://github.com/adam-nahal/hole_punching.git
cd hole_punching
cargo build --release
```

The binary will be available at `target/release/zeta9`.

## Usage

### Running as Relay Server

The relay server facilitates peer discovery:

```bash
./target/release/zeta9 --mode relay
```

The relay will:
- Bind to port `12345` on your public IP
- Accept connections from Dial and Listen peers
- Forward messages between connected peers

### Running as Listen Peer (Receiver)

```bash
./target/release/zeta9 \
    --mode listen \
    --relay-ip <RELAY_IP> \
    --relay-port 12345
```

### Running as Dial Peer (Initiator)

```bash
./target/release/zeta9 \
    --mode dial \
    --relay-ip <RELAY_IP> \
    --relay-port 12345 \
    --remote-peer-ip <LISTEN_PEER_IP> \
    --remote-peer-port <LISTEN_PEER_PORT>
```

### Command-Line Options

| Option | Required For | Description |
|--------|--------------|-------------|
| `--mode` | Always | Operation mode: `relay`, `listen`, or `dial` |
| `--relay-ip` | listen, dial | IP address of the relay server |
| `--relay-port` | listen, dial | Port of the relay server (default: 12345) |
| `--remote-peer-ip` | dial | IP address of the target peer |
| `--remote-peer-port` | dial | Port of the target peer |

## How It Works

### 1. NAT Detection

Each peer detects its NAT type using STUN servers:
- Open Internet
- Full Cone
- Restricted Cone
- Port Restricted Cone
- Symmetric
- Symmetric UDP Firewall
- UDP Blocked

### 2. Peer Discovery

1. **Listen peer** connects to relay and announces readiness
2. **Dial peer** connects to relay and requests connection to Listen peer
3. **Relay** forwards addresses between peers

### 3. Hole Punching

1. Both peers bind to their local addresses with `SO_REUSEADDR`
2. Both peers attempt simultaneous connection to each other
3. NAT tables are "punched" creating a direct path
4. Direct P2P connection established

## Protocol Messages

| Message | Direction | Purpose |
|---------|-----------|---------|
| `DIAL_REQUEST:<ip>:<port>` | Dial → Relay | Request connection to Listen peer |
| `LISTEN_READY:<address>` | Listen → Relay | Announce readiness to receive |

## Project Structure

```
hole_punching/
├── Cargo.toml              # Package manifest
├── README.md               # This file
└── src/
    ├── main.rs             # Entry point, CLI parsing
    ├── client.rs           # Listen/Dial mode logic
    ├── relay.rs            # Relay server implementation
    └── nat_detector/
        ├── mod.rs          # NAT detection entry point
        ├── util.rs         # STUN-based NAT detection
        └── valid_ipv4s.txt # STUN server list
```

## Dependencies

| Dependency | Purpose |
|------------|---------|
| `tokio` | Async runtime |
| `clap` | CLI argument parsing |
| `reqwest` | HTTP client (public IP detection) |
| `stun_codec_blazh` | STUN protocol encoding/decoding |
| `bytecodec` | Binary codec for STUN |
| `log` / `simple_logger` | Logging |
| `rand` | Random STUN server selection |

## Limitations

- Hole punching may not work with all NAT types (especially Symmetric NAT)
- Requires both peers to coordinate timing for simultaneous connection
- Relay server must be publicly accessible

## License

This project is open source. See the repository for license information.

## Contributing

Contributions are welcome! Please feel free to submit issues and pull requests.
