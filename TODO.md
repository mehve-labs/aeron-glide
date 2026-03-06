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

- [x] **Controlled Fragment Handlers**
  - Bind `aeron::ControlledFragmentAssembler` and the `ControlledFragmentHandler` interface.
  - Expose flow-control actions (ABORT, BREAK, COMMIT, CONTINUE) to give consumers back-pressure control over polling.
  - Unified into `poll_assembled` via `PollAction` trait: closures returning `()` map to CONTINUE, closures returning `ControlledAction` get full flow control. Demonstrated in `large_pong`.

- [x] **UDP Channel Support**
  - Channel strings pass through the cxx shim to Aeron unmodified — no C++ changes needed.
  - Added `--channel` CLI flag to ping and pong (default: `aeron:ipc`).
  - UDP usage: `--channel "aeron:udp?endpoint=localhost:20121"`.

- [x] **URI / Channel Builder**
  - Pure Rust `ChannelBuilder` with `ipc()` / `udp()` constructors and typed methods for all common params.
  - Generic `param(key, value)` escape hatch for any Aeron URI parameter.
  - 6 unit tests covering IPC, UDP, multicast, MDC, multi-param, and custom params.

- [x] **Advanced Media Driver Control**
  - Expanded `MediaDriverWrapper` to hold `aeron_driver_context_t*` and `aeron_driver_t*` with proper lifecycle (context init in constructor, driver+context close in destructor).
  - 17 setter methods exposed through cxx: dir, dir_delete_on_start/shutdown, threading_mode, conductor/sender/receiver idle strategies, term/ipc_term buffer lengths, mtu/ipc_mtu lengths, socket so_rcvbuf/so_sndbuf, print_configuration, conductor/sender/receiver CPU affinity.
  - Rust enums: `ThreadingMode` (Dedicated, SharedNetwork, Shared, Invoker) and `IdleStrategy` (Backoff, Spin, Yield, Sleeping, Noop).
  - `MediaDriver::new()` and `start()` now return `Result`.
  - Renamed `aeronmd` binary to `mediadriver`. Config via YAML file (`examples/mediadriver.yaml`) instead of CLI flags.
  
- [x] **Aeron Archive**
  - Expose the Aeron Archive API (recording, replay, listing, position queries, truncation) via the C++ Archive wrapper.
  - Gated behind `archive` Cargo feature flag (requires Java 17+ for SBE codec generation).
  - `AeronArchive::connect()` with configurable control request/response channels.
  - Recording: `start_recording`, `stop_recording`, `stop_recording_by_channel_and_stream`.
  - Replay: `start_replay`, `stop_replay`, `stop_all_replays`.
  - Listing: `list_recordings`, `list_recordings_for_uri`, `find_last_matching_recording` with closure-based `RecordingDescriptor` callback.
  - Position: `get_recording_position`, `get_start_position`, `get_stop_position`, `get_max_recorded_position`.
  - Error polling: `poll_for_error_response`, `check_for_error_response`.

- [x] **Image Buffers**
  - Bind `aeron::Image` to expose per-publisher stream access from a `Subscription`.
  - Metadata: `session_id`, `correlation_id`, `join_position`, `source_identity`.
  - Position tracking: `position`, `set_position`.
  - State: `is_closed`, `is_end_of_stream`, `end_of_stream_position`.
  - Polling: `poll` (raw fragments), `poll_assembled` (assembled + unified flow control via `PollAction` trait).
  - `Subscription` accessors: `image_count`, `image_by_index`, `image_by_session_id`.

- [x] **Replay Merge**
  - Bind `aeron::archive::client::ReplayMerge` to seamlessly combine a recorded stream replay with a live stream for gap-fill scenarios.
  - State machine: REPLAY → CATCHUP → ATTEMPT_LIVE_JOIN → MERGED. UDP only, requires `control-mode=manual` subscription.
  - `ReplayMerge::new()` with subscription, archive, replay/live channels, recording ID, start position.
  - `do_work()` drives the state machine, `poll()` for fragments, `image()` for the merged Image.
  - State queries: `is_merged()`, `has_failed()`, `is_live_added()`.
  - Demo binary: `replay_merge_demo` (requires archive feature + running archive daemon).

- [ ] **Aeron Cluster (Consensus & State Machines)**
  - Implement bindings to the Aeron Cluster C++ client, which handles interactions with Raft-based consensus logging and distributed service interactions.

- [ ] **Async Resource Creation** *(low priority)*
  - Bind `aeron::Aeron::asyncAddPublication`, `asyncAddExclusivePublication`, `asyncAddSubscription`, and `asyncAddCounter`.
  - Provide polling-based resource acquisition for non-blocking startup patterns.
  - Note: the current shim already does the async pattern internally (register + spin on find). The spin completes in microseconds. This would only matter if pipelining many resource creations or integrating into an event loop that can't block at all.
