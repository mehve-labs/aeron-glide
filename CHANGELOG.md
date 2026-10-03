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
- **Breaking:** `Subscription::image_by_index`, `Subscription::image_by_session_id`
  and `ReplayMerge::image` return `Option<Image>` instead of an error when there
  is no such image.

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
