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

*(Note: The `build.rs` script will automatically fetch and compile Aeron `v1.44.1` for you during the initial `cargo build`.)*

## Running the Examples

This repository includes a classic `ping-pong` pub/sub IPC example that demonstrates how to construct the Aeron client, launch an Embedded Media Driver, and send/receive messages asynchronously.

To run the examples and see Aeron IPC in action, you'll need two terminal windows open side-by-side.

### 1. Start the Ping node (Producer)
In the first terminal, run:
```bash
cargo run --bin ping
```

You should see:
```text
Starting Media Driver...
Starting Aeron Client...
Waiting for pong subscriber...
```
*The ping process will launch the embedded media driver and pause, waiting until it detects a subscriber on the other end.*

### 2. Start the Pong node (Consumer)
In the second terminal, run:
```bash
cargo run --bin pong
```

You should see it immediately connect:
```text
Starting Aeron Client...
Pong waiting for ping messages...
Pong received ping: "ping!"
Pong received ping: "ping!"
...
```

### 3. Watch the Messages Flow!
Back in the **first terminal**, you will see the round-trip acknowledgements streaming in as the Pong node echoes your messages back:
```text
Connected. Sending pings...
Ping received response: "ping!"
Completed roundtrip 0
Ping received response: "ping!"
Completed roundtrip 1
...
10 ping-pongs completed in 164.5ms
```

*(Note: The `ping` binary sends 10 messages and exits, which stops the embedded media driver. The `pong` binary will log errors once the media driver directory is deleted, which you can stop with `Ctrl+C`)*
