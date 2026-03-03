# aeron-rs Implementation Plan

The `aeron-rs` project currently demonstrates basic IPC Publication and Subscription. To provide a fully featured, safe, and idiomatic Rust wrapper over the Aeron C++ API, the following major components need to be bound via `cxx`:

## Planned Features

- [x] **Aeron Counters (CNC metrics)**
  - Bind `aeron::concurrent::CountersReader` to read the `cnc.dat` command-and-control file.
  - Implement zero-allocation polling of error logs, loss reporters, and real-time statistics.
  - ~~TODO: Add a command to read the counters or integrate it in the ping/pong example.~~ Done: standalone `counters` binary + integrated in ping.

- [x] **Exclusive Publications**
  - Bind `aeron::ExclusivePublication` for session-specific, higher-throughput publishing.
  - Exclusive publications avoid contention by guaranteeing a single writer per session, critical for low-latency paths.
  - Integrated in ping/pong examples via `--exclusive` CLI flag (clap).

- [x] **Buffer Claims (Zero-Copy Offer)**
  - Bind `aeron::BufferClaim` / `tryClaim()` to allow writing directly into the log buffer.
  - Avoids a memcpy on the offer path — essential for performance-sensitive messaging.
  - Callback-based API: closure receives `&mut [u8]` into shared memory, returns bool (commit/abort).
  - Integrated in ping example via `--zero-copy` CLI flag.

- [x] **Aeron Fragment Assemblers**
  - Bind `aeron::FragmentAssembler` to automatically reassemble messages that exceed the MTU (Maximum Transmission Unit) across multiple fragments.
  - Provide a safe Rust closure interface for reassembled message handlers.
  - `Subscription::poll_assembled()` method reuses existing handler trampoline. Used by default in ping/pong.

- [ ] **Controlled Fragment Handlers**
  - Bind `aeron::ControlledFragmentAssembler` and the `ControlledFragmentHandler` interface.
  - Expose flow-control actions (ABORT, BREAK, COMMIT, CONTINUE) to give consumers back-pressure control over polling.

- [ ] **Async Resource Creation**
  - Bind `aeron::Aeron::asyncAddPublication`, `asyncAddExclusivePublication`, `asyncAddSubscription`, and `asyncAddCounter`.
  - Provide polling-based resource acquisition for non-blocking startup patterns.

- [ ] **UDP Channel Support**
  - Extend examples and testing beyond IPC to cover UDP unicast and multicast channels.
  - Ensure `aeron:udp?endpoint=...` channels work end-to-end through the cxx shim.

- [ ] **URI / Channel Builder**
  - Provide a Rust-side builder for constructing Aeron channel URIs programmatically (endpoint, control, interface, MTU, term length, etc.).
  - Eliminates error-prone manual string construction.

- [ ] **Aeron Archive & Image Buffers**
  - Bind `aeron::Image` to expose lower-level control of active streams.
  - Expose the Aeron Archive API (which enables recording streams to disk and replaying them) via the C++ Archive client.

- [ ] **Replay Merge**
  - Bind the Aeron Archive replay-merge functionality to seamlessly combine a recorded stream replay with a live stream for gap-fill scenarios.
  - Distinct from basic Archive replay — requires coordinating replay position with live subscription.

- [ ] **Advanced Media Driver Control**
  - Currently we start the Embedded Driver with defaults. We need to expand the C Shim to map to the heavily configurable `aeron_driver_context_t` (and potentially the C++ Driver wrapper if available) to allow tuning of RingBuffer mapping, buffer sizes, threading modes, and idle strategies.

- [ ] **Aeron Cluster (Consensus & State Machines)**
  - Implement bindings to the Aeron Cluster C++ client, which handles interactions with Raft-based consensus logging and distributed service interactions.
