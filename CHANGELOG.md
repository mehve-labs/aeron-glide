# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Changed

- **Breaking:** the archive client follows the C++ API. Connect with
  `archive::Context` (`.aeron(&client)` shares a client; channels, stream ids,
  `message_timeout`, `message_retry_attempts`, `idle_strategy`,
  `delegating_invoker`, `error_handler`, `credentials_supplier`,
  `recording_signal_consumer`, ...) and `connect()` / `connect_async()`,
  instead of `AeronArchive::connect(channel, stream, channel, stream)`.
  `AeronArchive` methods take `&self` (it is `Send + Sync`). `start_replay`
  takes `&ReplayParams` instead of a position and a length.
  `RecordingDescriptor` gains `source_identity` and is `#[non_exhaustive]`.
  `ReplayMerge::new` borrows the archive immutably.
- **Breaking:** `ChannelBuilder` covers every C++ `ChannelUriStringBuilder`
  option and validates like it: `build()` returns `Result<String>`, failing
  with the first invalid setting (e.g. an MTU that is not a multiple of 32, a
  term length that is not a power of two). Parameters are written in the C++
  order, then custom ones; setting one again replaces it. Renamed to the C++
  names: `interface` → `network_interface`, `control` → `control_endpoint`,
  `socket_sndbuf` → `socket_sndbuf_length`, `socket_rcvbuf` →
  `socket_rcvbuf_length`, `receiver_window` → `receiver_window_length`.
  `control_mode` takes a `ControlMode`, `linger` a `Duration`, `mtu` and
  `term_length` a `u32`.
- The C++ shim is compiled as C++17 (was C++14): building needs a C++17
  compiler (GCC 7+, Clang 5+, MSVC 2017+).
- **Breaking:** `AeronClient::start` is removed. It did nothing: the client
  starts when it connects.
- **Breaking:** the cxx bridge module `aeron_glide::ffi` is private. It exposed
  raw, unchecked C++ calls (e.g. committing a forged buffer claim) to safe code.
- **Breaking:** fallible APIs return `aeron_glide::Result<T>` with a typed
  `aeron_glide::Error` (`kind()`, `code()`, `message()`, `is_fatal()`) instead
  of `Box<dyn Error>`. `Error` is `Send + Sync + 'static`. Aeron C++ exceptions
  and media driver errors are mapped to an `ErrorKind` matching the Aeron
  exception class, with the Aeron error code preserved.
- **Breaking:** fragment handlers receive the fragment `Header` as a second
  argument (`|data, header|`, or `|data, _|` to ignore it), matching the C++
  API: `Subscription::poll` / `poll_assembled`, `Image::poll` /
  `poll_assembled` and `ReplayMerge::poll`. `Header` exposes `session_id`,
  `stream_id`, `term_id`, `term_offset`, `initial_term_id`, `position`,
  `position_bits_to_shift`, `frame_length`, `header_type`, `flags` and
  `reserved_value`.
- **Breaking:** `try_claim(length)` returns a `BufferClaim` guard instead of
  taking a closure: write into `buffer_mut()`, optionally set `flags`,
  `header_type` or `reserved_value`, then `commit()` (returns the position) or
  `abort()`; dropping it aborts. This also fixes `try_claim` reporting success
  when the closure aborted the claim.
- **Breaking:** `offer` and `try_claim` on `Publication` and `ExclusivePublication`
  return `Result<i64, OfferError>` instead of a raw position. Aeron errors such
  as an oversized message are returned as `OfferError::Error` instead of
  aborting the process.
- **Breaking:** calls that can fail inside Aeron now return `Result` instead of
  aborting the process: `Subscription::poll` / `poll_assembled`, `Image::poll` /
  `poll_assembled` / `position`, `ReplayMerge::poll` (which previously reported
  errors as 0 fragments), and `CountersReader::get_counter_value` /
  `get_counter_state` / `get_counter_type_id` / `get_counter_label` (e.g. for an
  out-of-range counter id).
- `Image::position` documents that a closed image reports its final position.
- Strings read from Aeron (counter labels, image source identities, recording
  channels) no longer abort on invalid UTF-8; invalid bytes become `U+FFFD`.
- Closures are passed to C++ through monomorphised trampolines instead of
  thread-local `HashMap` registries: no hashing per poll, no `transmute`, and
  handlers may now poll other subscriptions, images or `ReplayMerge`s (this
  used to abort the process with "already borrowed").
- A panic in a fragment, counter, recording or reserved-value handler now
  unwinds to the caller of `poll` / `for_each` / `list_recordings` / `offer_*`
  once the C++ call has returned, instead of aborting the process. The fragment
  being handled by a controlled poll is aborted (re-delivered on the next poll);
  remaining fragments of a plain poll are consumed without delivery.
- **Breaking:** `CountersReader::for_each` returns `Result<()>`.
- **Breaking:** `AeronClient::add_publication`, `add_exclusive_publication` and
  `add_subscription`, and `Publication::offer` / `try_claim`, take `&self`.
  `AeronClient::start` takes `&self` too.
- **Breaking:** the media driver is configured with `MediaDriver::builder()`
  (chainable setters without the `set_` prefix; the first invalid setting is
  reported by `start()`), or started with defaults via `MediaDriver::launch()`.
  A started `MediaDriver` cannot be reconfigured or started again; previously
  setters after `start()` raced with the driver's threads and a second
  `start()` leaked the first driver. `MediaDriver::new` and its `Default` impl
  are removed. `ThreadingMode::Invoker` is rejected by `start()` until the
  driver duty cycle is exposed. `MediaDriver` is `Send + Sync` and has `dir()`.
- **Breaking:** `ReplayMerge` is now `ReplayMerge<'a>` and keeps the
  `&mut Subscription` passed to `ReplayMerge::new` borrowed while it is alive.
- **Breaking:** `Image` is now `Image<'a>`, borrowing its `Subscription` /
  `ReplayMerge`. `Subscription::image_by_index` and `image_by_session_id` take
  `&self`.
- **Breaking:** `Subscription::image_by_index`, `Subscription::image_by_session_id`
  and `ReplayMerge::image` return `Option<Image>` instead of an error when there
  is no such image.

### Fixed

- Strings with an interior NUL (channels, destinations, directories, client
  name, image rejection reasons, media driver and archive settings) were
  silently cut short at the NUL; they now fail with `IllegalArgument`.
- `archive::NULL_POSITION` / `NULL_LENGTH` were `i64::MIN`; Aeron's null value
  is -1, so replays "from the start" or "to the end" used invalid values.
- `ReplayMerge`'s default progress timeout is the C++ 5 seconds (was 10).
- Works around upstream archive client bugs: reading the context's recording
  events channel when unset crashed, replicating without credentials read
  uninitialised memory, a custom idle strategy was dropped after connecting,
  and some requests never ran an agent invoker client's conductor (hanging).
- `Counter::compare_and_set` could report success without writing on ARM when
  the value changed away and back to the expected one (an upstream bug in
  Aeron's GCC atomics, worked around in the shim).
- Synchronous adds on a closed client (e.g. after a driver timeout) waited for
  the whole driver timeout; they now fail at once with `IllegalState`.
- The embedded driver's version labels (e.g. the "Aeron software" counter)
  carried the git commit of whatever repository the crate was built in.
- Agent invoker mode was not thread-safe: the C client runs conductor work inline
  on whichever thread adds or closes a resource, which raced with `invoke()`
  (crashes). A per-client conductor lock now serialises all of it in invoker
  mode (no cost in threaded mode).
- Handlers could crash or hang the client: adding or removing a handler, or a
  synchronous add, from inside a handler now fails with `ErrorKind::Reentrant`
  (as does a nested `invoke()`), and a handler released on the conductor thread
  (e.g. a subscription's image handler) may own the last client.
- A dropped or timed-out `PendingAdd` left its resource alive until the client
  closed; the client now closes it once the driver has created it. Polling a
  pending add again after it failed reports `IllegalState`.
- Polling an image from inside a handler that is already polling the same
  image through another handle now fails with `ErrorKind::Reentrant`; it used
  to re-deliver fragments and could release the term the outer handler was
  reading.
- Synchronous `add_publication` / `add_exclusive_publication` /
  `add_subscription` spun forever if the media driver never answered; they now
  fail with `ErrorKind::Timeout` after the client's driver timeout.
- Reassembly state is shared by a subscription and all of its `Image` handles
  (one C++ `ControlledFragmentAssembler` per subscription), so a message whose
  fragments are polled through different handles is no longer lost. A nested
  assembled poll on the same subscription now fails with
  `ErrorKind::Reentrant` instead of corrupting the buffer the outer handler is
  reading.
- `ExclusivePublication::channel_status` failed to link (Aeron 1.53.3 declares
  the C++ method but never defines it), and `local_socket_addresses` on
  publications read an uninitialised buffer for IPC or inactive channels (an
  upstream C++ wrapper bug). Both now call the C functions directly.
- `ControlledAction` used the wrong values (0–3 instead of Aeron's 1–4), so
  every action returned from a `poll_assembled` handler did the next one's job:
  `Abort` continued, `Break` aborted, `Commit` broke off and `Continue`
  committed. A panicking assembled handler therefore did not abort its fragment
  either.
- Neither the client nor the archive client calls `exit()` on asynchronous
  errors such as a media driver timeout or shutdown any more (the default Aeron
  error handlers did). Errors go to `Context::error_handler`, or are printed to
  stderr; fatal ones close the client. The archive now connects through its own
  client with a non-exiting handler (honouring `AERON_DIR`).
- Client errors reported with positive Aeron client error codes are classified:
  "MediaDriver has been shutdown" and other timeouts as `DriverTimeout` /
  `ClientTimeout` / `ConductorServiceTimeout`, buffer-full errors as
  `IllegalState`, instead of the generic `Aeron` kind.
- `try_claim` truncated lengths above `i32::MAX` to 32 bits (e.g. a claim of
  4 GiB + 16 bytes committed a 16-byte message); such lengths are now rejected
  with `ErrorKind::IllegalArgument`.
- Use-after-free: an `Image` could outlive its subscription and client, and
  then crash on `position()`, `poll()` or drop. The C++ image wrapper now keeps
  its subscription (and through it the client) alive, and `Image` borrows the
  `Subscription` or `ReplayMerge` it came from.

### Added

- Publication accessors on `Publication` and `ExclusivePublication`: `channel`,
  `stream_id`, `session_id`, `initial_term_id`, `registration_id`,
  `original_registration_id`, `is_original` (concurrent only), `max_message_length`,
  `max_payload_length`, `term_buffer_length`, `position_bits_to_shift`,
  `is_closed`, `max_possible_position`, `position`, `publication_limit`,
  `publication_limit_id`, `available_window`, `channel_status` (new
  `ChannelStatus` enum, with `NoStatus` for IPC and closed channels),
  `channel_status_id`, `local_socket_addresses`.
- Vectored and reserved-value offers on both publication types:
  `offer_vectored` (several buffers as one message, no copy),
  `offer_with_reserved_value` and `offer_vectored_with_reserved_value` (a
  supplier sets each fragment header's reserved value).
- `Subscription` accessors and polling: `channel`, `stream_id`,
  `registration_id`, `channel_status`, `channel_status_id`, `is_closed`,
  `local_socket_addresses`, `resolved_endpoint`,
  `try_resolve_channel_endpoint_port`, `controlled_poll` (flow-controlled,
  without reassembly), `block_poll`, `images` and `for_each_image`.
- `Image` accessors and polling: `initial_term_id`, `term_buffer_length`,
  `position_bits_to_shift`, `subscriber_position_id`,
  `subscription_registration_id`, `is_publication_revoked`,
  `active_transport_count`, `reject`, `controlled_poll` (without reassembly),
  `bounded_poll`, `bounded_controlled_poll` and `block_poll`.
- `Context::default_aeron_path` and `Context::request_driver_termination`.
- Agent invoker mode: `Context::use_conductor_agent_invoker` runs the client
  conductor inside `AeronClient::invoke()` on your own thread instead of a
  dedicated one (`AeronClient::uses_agent_invoker`). Synchronous adds invoke
  the conductor while they wait.
- Lifecycle handlers, run on the client conductor thread: `Context` gains
  `on_available_image`, `on_unavailable_image`, `on_new_publication`,
  `on_new_exclusive_publication`, `on_new_subscription`,
  `on_available_counter`, `on_unavailable_counter`, `on_close_client` and
  `on_publication_error_frame` (events `ImageEvent`, `NewPublication`,
  `NewSubscription`, `CounterEvent`, `PublicationErrorFrame`); `AeronClient`
  gains `add_/remove_available_counter_handler`,
  `add_/remove_unavailable_counter_handler`, `add_/remove_close_client_handler`
  and `add_subscription_with_image_handlers` (plus `_async`). Handlers are
  owned by the C++ client, may drop the client, and have their panics caught.
- `Image::bounded_poll_assembled` and `ReplayMerge::poll_assembled`.
- Asynchronous adds: `AeronClient::add_publication_async`,
  `add_exclusive_publication_async` and `add_subscription_async` return a
  `PendingAdd` to `poll()` (or `wait()`) for the resource. Also
  `AeronClient::client_id`, `next_correlation_id`, `aeron_dir`,
  `cnc_file_name`, `driver_timeout`, `client_name` and
  `idle_sleep_duration`.
- Multi-destination support: `add_destination`, `remove_destination` and
  `find_destination_response` on `Publication`, `ExclusivePublication` and
  `Subscription`, plus `remove_destination_by_id` on publications.
- `ExclusivePublication::revoke` (consumes the publication) and
  `revoke_on_close`: end the stream for subscribers without lingering.
- Every scalar media driver setting is available on `MediaDriverBuilder`
  (96 setters, e.g. `publication_linger_timeout_ns`, `sender_wildcard_port_range`,
  `receiver_group_tag`) with the matching getters on a started `MediaDriver`
  (95, e.g. `dir()`), generated from Aeron's `aeronmd.h`. New enums
  `ThreadNaming` and `InferableBoolean`. Settings that take function pointers or
  driver-internal structs are not exposed yet. Strings passed to driver
  settings are owned by the builder (some C setters keep the pointer), and
  setting an idle strategy's `*_init_args` reloads that strategy, so their order
  doesn't matter.
- `Context` client configuration (`aeron_dir`, `client_name`, `driver_timeout`,
  `resource_linger_timeout`, `idle_sleep_duration`, `pre_touch_mapped_memory`,
  `error_handler`) and `AeronClient::connect(context)`. The client now honours
  `AERON_DIR`, so it can reach a `MediaDriver` started with a custom directory.
- Thread-safety markers matching Aeron: `AeronClient`, `Publication` and
  `CountersReader` are `Send + Sync`; `ExclusivePublication` and `Subscription`
  are `Send`. Share one client per process instead of one per thread.
- `OfferError` (`NotConnected`, `BackPressured`, `AdminAction`, `Closed`,
  `MaxPositionExceeded`, `Error`) with `is_retryable()` and `is_back_pressured()`.

- Counters: `AeronClient::add_counter` and `add_static_counter` (plus `_async`
  forms returning a `PendingAdd<Counter>`) allocate counters in the media
  driver. `Counter` has `id`, `registration_id`, `state`, `label`, `is_closed`
  and the atomic counter operations (`get`, `get_weak`, `set`, `set_ordered`,
  `set_weak`, `increment`, `increment_ordered`, `get_and_add`,
  `get_and_add_ordered`, `get_and_set`, `compare_and_set`); it is
  `Send + Sync`; its weak and ordered operations use relaxed/release atomics
  instead of the C++ plain accesses, so sharing it between threads is not a
  data race. `CountersReader::counter` (`unsafe`: writing a counter the client
  relies on, such as a subscriber position, can make it read out of bounds)
  gives a writable handle on an existing counter. Keys and labels longer than `CountersReader::MAX_KEY_LENGTH` /
  `MAX_LABEL_LENGTH` are rejected. Works around an upstream C++ bug: a counter
  holding the last reference to its client was closed after the client had
  freed it.
- `CountersReader` lookups: `find_by_registration_id` and
  `find_by_type_id_and_registration_id` (returning `Option<i32>`),
  `get_counter_registration_id`, `get_counter_owner_id`,
  `get_free_for_reuse_deadline` and `get_counter_key`.
- `ChannelBuilder` options: `prefix`, `media`, `tags`, `alias`, `group_tag`,
  `initial_term_id`, `term_id`, `term_offset`, `initial_position`,
  `session_id_tagged`, `eos`, `group`, `spies_simulate_connection`,
  `media_receive_timestamp_offset`, `channel_receive_timestamp_offset`,
  `channel_send_timestamp_offset`, `response_correlation_id`, `nak_delay`,
  `untethered_window_limit_timeout`, `untethered_resting_timeout`,
  `max_resend`, plus `remove` and `clear`.
- `ChannelBuilder::build` also rejects empty values (a trailing one such as
  `tags=` crashes the media driver's URI parser) and URIs longer than
  `channel::MAX_URI_LENGTH`. Channel errors carry `EINVAL` as their code.
- `ChannelUri` (C++ `ChannelUri`): `parse` / `FromStr`, `prefix`, `media`,
  `scheme`, `get`, `put`, `remove`, `contains_key`, `params`,
  `has_control_mode_response`, `Display`, and `add_session_id` /
  `add_alias_if_absent`. Parameters keep their order.
- Response channels (`control-mode=response`) are documented in the `channel`
  module, with the `response_channel` example and an end-to-end test.
- The `channel` module with the C++ URI parameter name constants
  (`ENDPOINT_PARAM_NAME`, ...).
- Archive client (C++ `AeronArchive` parity): `add_recorded_publication`,
  `add_recorded_exclusive_publication`, `extend_recording`,
  `try_stop_recording`, `try_stop_recording_by_channel_and_stream`,
  `try_stop_recording_by_identity`, `stop_recording_publication`,
  `stop_recording_exclusive_publication`, `purge_recording`, `update_channel`,
  `list_recording`, `list_recording_subscriptions`
  (`RecordingSubscriptionDescriptor`), `poll_for_recording_signals`
  (`RecordingSignal`, `RecordingSignalCode`), `replay` (returns a
  `Subscription`), `ReplayParams` (bounded replays, file I/O length, replay
  token, subscription registration id), `replicate` / `stop_replication` /
  `try_stop_replication` with `ReplicationParams`, segment operations
  (`detach_segments`, `delete_detached_segments`, `purge_segments`,
  `attach_segments`, `migrate_segments`, `segment_file_base_position`),
  `context()` (`ContextInfo`), `AsyncConnect`, `archive::recording_pos`
  (recording position counters), `PersistentSubscription` /
  `PersistentSubscriptionBuilder`, `ReplayMerge::with_progress_timeout`, and
  `ArchiveErrorCode` (`ArchiveErrorCode::of(&error)`).
- `Debug` for every public type (clients, publications, subscriptions, images,
  counters, contexts, the media driver and archive types); `ChannelBuilder`
  is also `Clone`.
- `counter_types`: the counter type IDs of the media driver, archive and
  cluster (C++ `AeronCounters`), generated from the Aeron headers.
- `CncFile`: map a driver's CnC file without a client
  (`CncFile::map_existing`, or `map_existing_with_timeout`), read its counters
  (`counters_reader`, read-only), its distinct-error log (`read_error_log`,
  with an `ErrorLogEntry` per error), its `constants` (`CncConstants`: PID,
  start time, buffer lengths, ...) and the driver's liveness
  (`to_driver_heartbeat`, `is_driver_active`). A corrupt CnC file whose layout does not fit
  the file is rejected, and errors the driver is still recording are skipped.
- `heartbeat_timestamp` (C++ `HeartbeatTimestamp`): `CLIENT_HEARTBEAT_TYPE_ID`,
  `find_counter_id_by_registration_id` and `is_active`, to check whether a
  client is alive.
- `CountersReader` constants: `RECORD_UNUSED`, `RECORD_ALLOCATED`,
  `RECORD_RECLAIMED`, `DEFAULT_REGISTRATION_ID`, `NOT_FREE_TO_REUSE`,
  `MAX_LABEL_LENGTH`, `MAX_KEY_LENGTH`.

## [0.3.1] - 2026-10-03

### Added

- `Subscription::delete_session_buffer` to free the reassembly buffer held for
  a publisher session. Backed by `deleteSessionBuffer`, which was a no-op in
  the Aeron C++ wrapper before 1.53.3.

### Changed

- Bump bundled Aeron to 1.53.3 (from 1.53.0).
- CI now also builds and tests on the MSRV (Rust 1.97) alongside latest stable.

## [0.3.0] - 2026-09-03

### Changed

- Bump bundled Aeron to 1.53.0 (from 1.52.2).

### Removed

- **Breaking:** `Image::set_position`. Aeron 1.53.0 removed the underlying
  `aeron_image_set_position` / `aeron::Image::position(int64_t)` APIs with no
  replacement, so the subscriber position can no longer be moved manually.
  `Image::position()` (the getter) is unaffected.

## [0.2.0] - 2026-08-01

### Changed

- Bump bundled Aeron to 1.52.2 (from 1.52.0).
- **Relicensed to the Apache License 2.0.** aeron-glide is no longer
  dual-licensed under AGPL-3.0-or-later plus a commercial license; it is now
  offered solely under the permissive Apache-2.0 license, which permits
  proprietary and closed-source use subject only to attribution and notice
  terms. Contributions are now accepted under the standard Apache "inbound =
  outbound" model.

### Removed

- `LICENSE-COMMERCIAL.md` — the commercial license option is obsolete under
  the permissive Apache-2.0 license.

## [0.1.3] - 2026-07-12

### Changed

- Clarify dual-licensing: aeron-glide is offered under **either** AGPL-3.0-or-later
  **or** a commercial license, at the user's choice.

### Added

- `CONTRIBUTING.md` with a lightweight Contributor License Agreement so
  contributions can be offered under both the open-source and commercial licenses.

## [0.1.2] - 2026-07-11

### Changed

- Update repository and homepage URLs to the mehve-labs organization

## [0.1.1] - 2026-07-11

### Changed

- Bump bundled Aeron to 1.52.0 (from 1.51.0)

## [0.1.0] - 2026-03-06

### Added

- **Aeron Client** (`AeronClient`): connect to the media driver, create publications and subscriptions
- **Publication** and **ExclusivePublication**: concurrent and single-writer message publishing
- **Zero-copy publish** via `try_claim` (writes directly into Aeron's log buffer)
- **Subscription**: poll for messages with fragment-level or assembled-message delivery
- **Fragment reassembly** via `poll_assembled` with `ControlledAction` flow control (Abort, Break, Commit, Continue)
- **Image** API: per-session stream access with position tracking, end-of-stream detection
- **CountersReader**: read real-time CNC counters (bytes sent/received, NAKs, errors, heartbeats)
- **Embedded Media Driver** (`MediaDriver`): full C media driver with YAML configuration support
  - Threading modes: Dedicated, SharedNetwork, Shared, Invoker
  - Idle strategies: Backoff, Spin, Yield, Sleeping, Noop
  - Buffer sizes, MTU, CPU affinity, and more
- **ChannelBuilder**: type-safe builder for `aeron:ipc` and `aeron:udp` channel URIs
- **Archive client** (behind `archive` feature flag):
  - Recording: start/stop recording channels to the archive
  - Replay: replay recorded streams from any position
  - Listing: query recording descriptors by ID, channel, or stream
  - Position queries: recording/start/stop/max positions
  - Truncation: truncate stopped recordings
  - Error polling and archive metadata
- **ReplayMerge**: seamless transition from archived replay to live stream (REPLAY -> CATCHUP -> MERGED)
- Examples: ping/pong, large message fragmentation, counters, image demo, throughput benchmark, latency benchmark, archive record/replay, replay merge
