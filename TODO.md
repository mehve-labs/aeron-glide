# aeron-rs Implementation Plan

The `aeron-rs` project currently demonstrates basic IPC Publication and Subscription. To provide a fully featured, safe, and idiomatic Rust wrapper over the Aeron C++ API, the following major components need to be bound via `cxx`:

## Planned Features

- [ ] **Aeron Counters (CNC metrics)**
  - Bind `aeron::concurrent::CountersReader` to read the `cnc.dat` command-and-control file.
  - Implement zero-allocation polling of error logs, loss reporters, and real-time statistics.

- [ ] **Aeron Fragment Assemblers**
  - Bind `aeron::FragmentAssembler` to automatically reassemble messages that exceed the MTU (Maximum Transmission Unit) across multiple fragments.
  - Provide a safe Rust closure interface for reassembled message handlers.

- [ ] **Aeron Archive & Image Buffers**
  - Bind `aeron::Image` to expose lower-level control of active streams.
  - Expose the Aeron Archive API (which enables recording streams to disk and replaying them) via the C++ Archive client.

- [ ] **Advanced Media Driver Control**
  - Currently we start the Embedded Driver with defaults. We need to expand the C Shim to map to the heavily configurable `aeron_driver_context_t` (and potentially the C++ Driver wrapper if available) to allow tuning of RingBuffer mapping, buffer sizes, threading modes, and idle strategies.

- [ ] **Aeron Cluster (Consensus & State Machines)**
  - Implement bindings to the Aeron Cluster C++ client, which handles interactions with Raft-based consensus logging and distributed service interactions.
