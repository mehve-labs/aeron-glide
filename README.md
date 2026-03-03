# aeron-rs

A safe, idiomatic Rust wrapper for the [Aeron](https://github.com/real-logic/aeron) C++ API, built using [`cxx`](https://cxx.rs/).

## Why `aeron-rs`?

Previously, the Rust ecosystem relied on projects like [rusteron](https://github.com/mimiquate/rusteron) to interface with Aeron. While `rusteron` successfully bridged the gap to the underlying C API, doing so heavily relied on complex generic code generation, unsafe bindings, and verbose C structs exposed directly to Rust developers. This often led to difficult-to-maintain abstractions and safety boundaries that were hard to enforce.

We decided to build something better.

`aeron-rs` takes a fundamentally different approach. Instead of binding strictly to the Aeron C API using `bindgen`, we bind directly to the **Aeron C++ API** using `cxx`. `cxx` creates a safe, statically verified bridge between Rust and C++, allowing us to eliminate vast amounts of boilerplate. Our C++ shim carefully wraps Aeron's `Context`, `Publication`, and `Subscription` objects, passing closures cleanly through trampolines into safe, idiomatic Rust structures.

The result is a fast, safe, and significantly cleaner Aeron client for Rust.

## Prerequisites

- **CMake** (for building the Aeron C++ Driver from source)
- **Rust** (Cargo)
- **C++14+ compiler**
- **Java JDK 17+** (only required when building with `--features archive`)

*(Note: The `build.rs` script will automatically fetch and compile Aeron `v1.50.2` for you during the initial `cargo build`.)*

## Running the Examples

All examples require a running Aeron Media Driver. You can start one with:

```bash
cargo run --bin mediadriver
```

This launches an embedded C media driver that manages shared memory buffers and handles publication/subscription matching. Keep it running in a dedicated terminal, then use any of the examples below in separate terminals.

You can optionally pass a YAML config file to tune driver settings (threading mode, buffer sizes, idle strategies, etc.):

```bash
cargo run --bin mediadriver -- example_configs/mediadriver.yaml
```

### Ping / Pong

Basic pub/sub round-trip. Sends 10 `"ping!"` messages and measures total time.

```bash
# Terminal 1                          # Terminal 2
cargo run --bin pong                  cargo run --bin ping
```

**Exclusive publication** (single-writer, lower contention):
```bash
cargo run --bin pong -- --exclusive
cargo run --bin ping -- --exclusive
```

**Zero-copy publish** (writes directly into Aeron's log buffer via `tryClaim`):
```bash
cargo run --bin pong
cargo run --bin ping -- --zero-copy
```

**Both combined:**
```bash
cargo run --bin pong -- --exclusive
cargo run --bin ping -- --exclusive --zero-copy
```

**UDP transport** (instead of IPC shared memory):
```bash
cargo run --bin pong -- --channel "aeron:udp?endpoint=localhost:20121"
cargo run --bin ping -- --channel "aeron:udp?endpoint=localhost:20121"
```

### Large Ping / Pong

Sends 8 KB messages that exceed the MTU and get fragmented by Aeron. Demonstrates `poll_assembled` (automatic fragment reassembly) and `ControlledAction` (back-pressure flow control).

```bash
# Terminal 1                          # Terminal 2
cargo run --bin large_pong            cargo run --bin large_ping
```

`large_pong` uses `ControlledAction::Abort` when it can't echo back immediately, causing Aeron to re-deliver the message on the next poll — no user-side buffering needed.

### Counters

Reads Aeron's CNC (command-and-control) counters — real-time stats like bytes sent/received, NAKs, errors, and heartbeats.

```bash
cargo run --bin counters
```

The `ping` binary also prints counters after its run.

## Archive Support

The Aeron Archive enables recording streams to disk and replaying them later.

**Important**: The Aeron Archive **server** (the process that actually records and replays streams) is Java-only — it is not exposed by the C or C++ API. You must run the Java `ArchivingMediaDriver` separately. This crate provides the **client** bindings that connect to and control that server.

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
cargo run --features archive --bin record

# Terminal 3: Replay all recorded messages from the beginning
cargo run --features archive --bin replay
```

### Archive Client API

The archive client API provides:
- **Recording**: start/stop recording any channel+stream to the archive
- **Replay**: replay recorded streams from any position
- **Listing**: query recording descriptors by ID, channel, or stream
- **Position queries**: get recording/start/stop/max positions
- **Truncation**: truncate stopped recordings

```rust
use aeron_rs::archive::{AeronArchive, SourceLocation};

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
