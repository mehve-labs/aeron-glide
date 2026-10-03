# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Changed

- **Breaking:** fallible APIs return `aeron_glide::Result<T>` with a typed
  `aeron_glide::Error` (`kind()`, `code()`, `message()`, `is_fatal()`) instead
  of `Box<dyn Error>`. `Error` is `Send + Sync + 'static`. Aeron C++ exceptions
  and media driver errors are mapped to an `ErrorKind` matching the Aeron
  exception class, with the Aeron error code preserved.
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
- A panic in a fragment, claim, counter or recording handler now unwinds to the
  caller of `poll` / `try_claim` / `for_each` / `list_recordings` once the C++
  call has returned, instead of aborting the process. The fragment being
  handled by a controlled poll is aborted (re-delivered on the next poll);
  remaining fragments of a plain poll are consumed without delivery; a
  panicking claim is aborted.
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
  `ChannelStatus` enum), `channel_status_id`, `local_socket_addresses`.
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
