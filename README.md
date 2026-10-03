# aeron-glide

[![CI](https://github.com/mehve-labs/aeron-glide/actions/workflows/ci.yml/badge.svg)](https://github.com/mehve-labs/aeron-glide/actions/workflows/ci.yml)
[![Crates.io](https://img.shields.io/crates/v/aeron-glide.svg)](https://crates.io/crates/aeron-glide)
[![docs.rs](https://docs.rs/aeron-glide/badge.svg)](https://docs.rs/aeron-glide)
[![License: Apache 2.0](https://img.shields.io/badge/license-Apache--2.0-blue.svg)](LICENSE)


A safe, idiomatic Rust wrapper for the [Aeron](https://github.com/real-logic/aeron) C++ API, built using [`cxx`](https://cxx.rs/).

## Why `aeron-glide`?

Previously, the Rust ecosystem relied on projects like [rusteron](https://github.com/mimiquate/rusteron) to interface with Aeron. While `rusteron` successfully bridged the gap to the underlying C API, doing so heavily relied on complex generic code generation, unsafe bindings, and verbose C structs exposed directly to Rust developers. This often led to difficult-to-maintain abstractions and safety boundaries that were hard to enforce.

We decided to build something better.

`aeron-glide` takes a fundamentally different approach. Instead of binding strictly to the Aeron C API using `bindgen`, we bind directly to the **Aeron C++ API** using `cxx`. `cxx` creates a safe, statically verified bridge between Rust and C++, allowing us to eliminate vast amounts of boilerplate. Our C++ shim carefully wraps Aeron's `Context`, `Publication`, and `Subscription` objects, passing closures cleanly through trampolines into safe, idiomatic Rust structures.

The result is a fast, safe, and significantly cleaner Aeron client for Rust.

## Installation

```toml
[dependencies]
aeron-glide = "0.3"
```

## Features

| Feature | Default | What it adds |
|---|---|---|
| `driver` | yes | The embedded C media driver (`MediaDriver`). Without it, run a driver separately and only the client is built. |
| `archive` | no | The Aeron Archive client (needs Java 17+ to build). |
| `bin` | no | The `mediadriver` binary (`cargo install aeron-glide --features bin`). |

## Prerequisites

- **CMake 3.30+** (Aeron 1.53 requires it; older distributions such as Debian 12 ship 3.25 — install a newer one from cmake.org or pip)
- **Rust 1.97+** (Cargo)
- **C++17 compiler** (GCC 7+, Clang 5+, MSVC 2017+)
- **Java JDK 17+** (only required when building with `--features archive`)
- On Linux: `libbsd` and `libuuid` development packages (`libbsd-dev uuid-dev` on Debian/Ubuntu)

The build script downloads Aeron `1.53.3` from GitHub (checking its SHA-256) and
compiles it on the first `cargo build`. Environment variables:

| Variable | Effect |
|---|---|
| `AERON_SOURCE_DIR` | Build from this Aeron source tree instead of downloading (offline builds) |
| `AERON_VERSION` | Another Aeron release (the generated bindings target 1.53.3) |
| `AERON_SHA256` | The expected SHA-256 of the downloaded tarball (overrides the built-in one) |

## Quick Start

```rust
use aeron_glide::AeronClient;

let client = AeronClient::new()?;

let pub1 = client.add_publication("aeron:ipc", 1001)?;
let mut sub1 = client.add_subscription("aeron:ipc", 1001)?;

// Publish (retry while not connected / back pressured)
while let Err(e) = pub1.offer(b"hello aeron") {
    if !e.is_retryable() {
        return Err(e.into());
    }
}

// Subscribe
sub1.poll(10, |data, _| {
    println!("Received: {}", String::from_utf8_lossy(data));
})?;
```

## Running the Examples

All examples require a running Aeron Media Driver. You can start one with:

```bash
cargo run --features bin --bin mediadriver
```

This launches an embedded C media driver that manages shared memory buffers and handles publication/subscription matching. Keep it running in a dedicated terminal, then use any of the examples below in separate terminals.

You can optionally pass a YAML config file to tune driver settings (threading mode, buffer sizes, idle strategies, etc.):

```bash
cargo run --features bin --bin mediadriver -- examples/mediadriver.yaml
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

The archive client covers the C++ `AeronArchive` API:
- **Connecting**: `archive::Context` (shared client, channels, timeouts, idle
  strategy, credentials, recording signals), blocking or asynchronous
- **Recording**: recorded publications, start/stop/extend recordings, purge,
  truncate, update channels, segment management
- **Replay**: `replay` / `start_replay` with `ReplayParams` (positions, lengths,
  bounded replays), `ReplayMerge`, and `PersistentSubscription` (replay, then
  follow the live stream)
- **Queries**: recording descriptors, recording subscriptions, positions,
  recording position counters (`archive::recording_pos`)
- **Replication** between archives, and typed `ArchiveErrorCode`s

```rust
use aeron_glide::AeronClient;
use aeron_glide::archive::{self, ReplayParams, SourceLocation};

let client = AeronClient::new()?;
let archive = archive::Context::new()
    .aeron(&client)
    .control_request_channel("aeron:udp?endpoint=localhost:8010")
    .control_response_channel("aeron:udp?endpoint=localhost:0")
    .connect()?;

// Start recording
let sub_id = archive.start_recording("aeron:ipc", 1001, SourceLocation::Local, false)?;

// List recordings
archive.list_recordings(0, 100, |desc| {
    println!("Recording {}: stream={} channel={}", desc.recording_id, desc.stream_id, desc.stripped_channel);
})?;

// Replay recording 0 from its start into a new subscription
let mut replay = archive.replay(0, "aeron:ipc", 1002, &ReplayParams::new().position(0))?;
replay.poll(10, |data, _| println!("{} bytes", data.len()))?;
```

The archive tests (`cargo test --features archive`) start a Java
`ArchivingMediaDriver` per test and are skipped when Java or the jar is not
available (set `AERON_GLIDE_REQUIRE_ARCHIVE=1` to fail instead).

## Documentation

Full API documentation is available on [docs.rs](https://docs.rs/aeron-glide).

## Minimum Supported Rust Version

The MSRV is **1.97.0**. CI also tests against the latest stable Rust.

## License

> **Disclaimer:** This project is not officially associated with or endorsed by Adaptive Financial Consulting Ltd. (Adaptive) or the Aeron project.

This project is licensed under the [Apache License 2.0](LICENSE) — free for everyone, any purpose (including proprietary and closed-source use), subject only to the attribution and notice terms of the license. See [NOTICE](NOTICE) for attribution details.

Unless you explicitly state otherwise, any contribution you submit for inclusion in aeron-glide shall be licensed under the Apache License 2.0, without any additional terms or conditions. See [CONTRIBUTING.md](CONTRIBUTING.md).
