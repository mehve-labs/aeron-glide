# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

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
