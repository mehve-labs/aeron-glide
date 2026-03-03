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
