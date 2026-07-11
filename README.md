# aeron-glide

[![CI](https://github.com/mehve-labs/aeron-glide/actions/workflows/ci.yml/badge.svg)](https://github.com/mehve-labs/aeron-glide/actions/workflows/ci.yml)
[![Crates.io](https://img.shields.io/crates/v/aeron-glide.svg)](https://crates.io/crates/aeron-glide)
[![docs.rs](https://docs.rs/aeron-glide/badge.svg)](https://docs.rs/aeron-glide)
[![License: AGPL-3.0](https://img.shields.io/badge/license-AGPL--3.0-blue.svg)](LICENSE)


A safe, idiomatic Rust wrapper for the [Aeron](https://github.com/real-logic/aeron) C++ API, built using [`cxx`](https://cxx.rs/).

## Why `aeron-glide`?

Previously, the Rust ecosystem relied on projects like [rusteron](https://github.com/mimiquate/rusteron) to interface with Aeron. While `rusteron` successfully bridged the gap to the underlying C API, doing so heavily relied on complex generic code generation, unsafe bindings, and verbose C structs exposed directly to Rust developers. This often led to difficult-to-maintain abstractions and safety boundaries that were hard to enforce.

We decided to build something better.

`aeron-glide` takes a fundamentally different approach. Instead of binding strictly to the Aeron C API using `bindgen`, we bind directly to the **Aeron C++ API** using `cxx`. `cxx` creates a safe, statically verified bridge between Rust and C++, allowing us to eliminate vast amounts of boilerplate. Our C++ shim carefully wraps Aeron's `Context`, `Publication`, and `Subscription` objects, passing closures cleanly through trampolines into safe, idiomatic Rust structures.

The result is a fast, safe, and significantly cleaner Aeron client for Rust.

## Installation

```toml
[dependencies]
aeron-glide = "0.1"
```

## Prerequisites

- **CMake** (for building the Aeron C++ Driver from source)
- **Rust 1.92+** (Cargo)
- **C++14+ compiler**
- **Java JDK 17+** (only required when building with `--features archive`)

*(Note: The `build.rs` script will automatically fetch and compile Aeron `v1.50.2` for you during the initial `cargo build`.)*

## Quick Start

```rust
use aeron_glide::AeronClient;

let mut client = AeronClient::new()?;
client.start();

let mut pub1 = client.add_publication("aeron:ipc", 1001)?;
let mut sub1 = client.add_subscription("aeron:ipc", 1001)?;

// Publish
while pub1.offer(b"hello aeron") < 0 {}

// Subscribe
sub1.poll(10, |data| {
    println!("Received: {}", std::str::from_utf8(data).unwrap());
});
```

## Running the Examples

All examples require a running Aeron Media Driver. You can start one with:

```bash
cargo run --bin mediadriver
```

This launches an embedded C media driver that manages shared memory buffers and handles publication/subscription matching. Keep it running in a dedicated terminal, then use any of the examples below in separate terminals.

You can optionally pass a YAML config file to tune driver settings (threading mode, buffer sizes, idle strategies, etc.):

```bash
cargo run --bin mediadriver -- examples/mediadriver.yaml
```

### Ping / Pong

Basic pub/sub round-trip. Sends 10 `"ping!"` messages and measures total time.

```bash
# Terminal 1                                    # Terminal 2
cargo run --example pong                        cargo run --example ping
```

**Exclusive publication** (single-writer, lower contention):
```bash
cargo run --example pong -- --exclusive
cargo run --example ping -- --exclusive
```

**Zero-copy publish** (writes directly into Aeron's log buffer via `tryClaim`):
```bash
cargo run --example pong
cargo run --example ping -- --zero-copy
```

**Both combined:**
```bash
cargo run --example pong -- --exclusive
cargo run --example ping -- --exclusive --zero-copy
```

**UDP transport** (instead of IPC shared memory):
```bash
cargo run --example pong -- --channel "aeron:udp?endpoint=localhost:20121"
cargo run --example ping -- --channel "aeron:udp?endpoint=localhost:20121"
```

### Large Ping / Pong

Sends 8 KB messages that exceed the MTU and get fragmented by Aeron. Demonstrates `poll_assembled` (automatic fragment reassembly) and `ControlledAction` (back-pressure flow control).

```bash
# Terminal 1                                    # Terminal 2
cargo run --example large_pong                  cargo run --example large_ping
```

`large_pong` uses `ControlledAction::Abort` when it can't echo back immediately, causing Aeron to re-deliver the message on the next poll -- no user-side buffering needed.

### Counters

Reads Aeron's CNC (command-and-control) counters -- real-time stats like bytes sent/received, NAKs, errors, and heartbeats.

```bash
cargo run --example counters
```

The `ping` example also prints counters after its run.

## Benchmarks

See [BENCHMARKS.md](BENCHMARKS.md) for full results. Summary on Apple Silicon:

| Test | Result |
|------|--------|
| IPC Throughput (exclusive, 32B) | ~67.5M msgs/sec |
| UDP Latency p50 (32B, localhost) | ~17.5 us |
| UDP Latency p99 (32B, localhost) | ~28.2 us |

```bash
cargo run --release --example throughput   # IPC throughput
cargo run --release --example latency      # UDP ping-pong latency
```

## Archive Support

The Aeron Archive enables recording streams to disk and replaying them later.

**Important**: The Aeron Archive **server** (the process that actually records and replays streams) is Java-only -- it is not exposed by the C or C++ API. You must run the Java `ArchivingMediaDriver` separately. This crate provides the **client** bindings that connect to and control that server.

Archive support is behind a Cargo feature flag because it requires Java 17+ at build time (for SBE codec generation):

```bash
cargo build --features archive
```

If your default Java is too old, set `JAVA_HOME`:

```bash
JAVA_HOME=/path/to/jdk17+ cargo build --features archive
```

### Running the Archive Server

Start the Java ArchivingMediaDriver (which includes both a media driver and the archive):

```bash
bash scripts/start-archive.sh
```

This finds the `aeron-all` jar built during `cargo build --features archive` and launches the server. Keep it running in a dedicated terminal.

### Record / Replay

With the archive server running:

```bash
# Terminal 2: Record 10 messages to the archive
cargo run --features archive --example record

# Terminal 3: Replay all recorded messages from the beginning
cargo run --features archive --example replay
```

### Archive Client API

The archive client API provides:
- **Recording**: start/stop recording any channel+stream to the archive
- **Replay**: replay recorded streams from any position
- **Listing**: query recording descriptors by ID, channel, or stream
- **Position queries**: get recording/start/stop/max positions
- **Truncation**: truncate stopped recordings

```rust
use aeron_glide::archive::{AeronArchive, SourceLocation};

let mut archive = AeronArchive::connect(
    "aeron:udp?endpoint=localhost:8010", 10,  // control request
    "aeron:udp?endpoint=localhost:0", 20,     // control response
)?;

// Start recording
let sub_id = archive.start_recording("aeron:ipc", 1001, SourceLocation::Local, false)?;

// List recordings
archive.list_recordings(0, 100, |desc| {
    println!("Recording {}: stream={} channel={}", desc.recording_id, desc.stream_id, desc.stripped_channel);
})?;

// Replay
let replay_session = archive.start_replay(0, "aeron:ipc", 1002, 0, i64::MAX)?;
```

## Documentation

Full API documentation is available on [docs.rs](https://docs.rs/aeron-glide).

## Minimum Supported Rust Version

The MSRV is **1.92.0**.

## License

> **Disclaimer:** This project is not officially associated with or endorsed by Adaptive Financial Consulting Ltd. (Adaptive) or the Aeron project.

This project is dual-licensed:

- **Open Source**: [AGPL-3.0-or-later](LICENSE) -- free for personal use, education, and open-source projects
- **Commercial**: A proprietary license is available for businesses that cannot comply with the AGPL copyleft requirements. See [LICENSE-COMMERCIAL.md](LICENSE-COMMERCIAL.md) for details.
