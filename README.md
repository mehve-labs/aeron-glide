# aeron-glide

[![CI](https://github.com/mehve-labs/aeron-glide/actions/workflows/ci.yml/badge.svg)](https://github.com/mehve-labs/aeron-glide/actions/workflows/ci.yml)
[![Crates.io](https://img.shields.io/crates/v/aeron-glide.svg)](https://crates.io/crates/aeron-glide)
[![docs.rs](https://docs.rs/aeron-glide/badge.svg)](https://docs.rs/aeron-glide)
[![License: Apache 2.0](https://img.shields.io/badge/license-Apache--2.0-blue.svg)](LICENSE)

A safe, idiomatic Rust API for [Aeron](https://github.com/aeron-io/aeron): the
client, the embedded C media driver and the archive client, built on Aeron's
C++ API with [`cxx`](https://cxx.rs/). It is built against Aeron **1.53.3**.

```rust
use aeron_glide::AeronClient;

let client = AeronClient::new()?; // connects to a running media driver
let publication = client.add_publication("aeron:ipc", 1001)?;
let mut subscription = client.add_subscription("aeron:ipc", 1001)?;

// Retry while not connected or back pressured; other errors are real.
while let Err(e) = publication.offer(b"hello aeron") {
    if !e.is_retryable() {
        return Err(e.into());
    }
}

subscription.poll(10, |data, _header| {
    println!("received {}", String::from_utf8_lossy(data));
})?;
```

## What it covers

- **Client**: publications (concurrent and exclusive), `offer`, zero-copy
  `try_claim`, vectored offers, block offers and padding (exclusive
  publications), subscriptions with fragment reassembly and
  controlled polling, images, synchronous and asynchronous adds, destinations
  (MDC/MDS), response channels, and the client's lifecycle handlers.
- **Channels**: `ChannelBuilder` builds and validates channel URIs (every C++
  `ChannelUriStringBuilder` parameter); `ChannelUri` parses them.
- **Counters**: your own counters, the driver's counters (`CountersReader`),
  and the driver's CnC file without a client (`CncFile`: counters, error log,
  loss report, driver liveness).
- **Embedded media driver** (`MediaDriver`, `driver` feature): every
  `aeronmd.h` setting, threaded or invoker mode (you run its duty cycle),
  termination validators and hooks. Also a `mediadriver` binary (`bin`
  feature) configured from YAML.
- **Agents** (`concurrent`): Aeron's idle strategies, and `AgentRunner` /
  `AgentInvoker` to run a client or driver duty cycle (or your own) on a
  thread you control.
- **Archive client** (`archive` feature): recording, replay (including
  bounded replays), queries, replication, `ReplayMerge`,
  `PersistentSubscription`, recording signals and typed archive error codes.

## How it differs from rusteron

[rusteron](https://github.com/gsrxyz/rusteron) is the other complete Aeron
binding for Rust. It is generated from Aeron's C API, exposes nearly every C
function with little abstraction, and is used in production. Its README
is upfront that the API "operates in an `unsafe` context": misuse, such as
using a publication after its client is closed, is undefined behaviour.

aeron-glide takes the opposite approach: a hand-written API where that misuse
cannot be expressed in safe code. Performance is the same (both spend their
time in the same Aeron C code; see [Benchmarks](#benchmarks)), so the
difference is in what the API guarantees.

**Where aeron-glide is stronger**

- **Safe by construction.** Publications, subscriptions and counters keep
  their client alive; borrowed objects (images, `ReplayMerge`, buffer claims)
  carry lifetimes; closing is dropping, in any order.
- **Thread safety in the types.** `Send` and `Sync` follow Aeron's documented
  rules out of the box: a client and a concurrent publication are `Sync`,
  subscriptions and exclusive publications are `Send` only. rusteron makes its
  handles `Sync` only with its `multi-threaded` feature.
- **Handlers that can't take the process down.** A panic in a handler is
  caught and resumed after the poll, or reported; in rusteron, callbacks are
  `extern "C"`, so a panic aborts. Handlers may drop what they own, including
  the client, and calls Aeron cannot make from a handler fail with an error
  instead of deadlocking.
- **Hardened against Aeron's own bugs.** Writing the bindings turned up bugs
  in Aeron's C++ wrapper, C client and archive client: a counter
  use-after-free, a `compareAndSet` that can succeed without writing on ARM,
  timeouts that overflow, an archive connect that can hang forever, readers
  that trust lengths from corrupt files, and more. The shim works around them
  and the tests check each workaround. Several are in Aeron's C client (the
  timeouts, the archive hang, the file readers), so code calling it directly,
  rusteron included, is exposed to those.
- **Rust-shaped API.** `&str` channels, builders that validate, one `Error`
  type with Aeron's error codes, idle strategies and agents, and documentation
  on every public item.

**Where rusteron is stronger**

- **Breadth.** Being generated, it covers nearly all of the C API, including
  the driver's pluggable flow control, congestion control and interceptors,
  which aeron-glide does not expose yet. If you need a C function we don't
  wrap, rusteron probably has it (and please open an issue).

| | aeron-glide | rusteron |
|---|---|---|
| Binds | Aeron's C++ API through `cxx`, and the C API where C++ lacks a call or has a bug | Aeron's C API, generated with `bindgen` and its own code generator |
| Misuse | Rejected by the compiler or returned as an error | Undefined behaviour, as its README documents |
| Panics in callbacks | Caught, then resumed or reported | Abort the process |
| Errors | `Error` with an `ErrorKind` and Aeron's error code, `OfferError`, typed archive error codes | `AeronCError` with an `AeronErrorType`, `AeronOfferError` |
| API surface | Curated and documented; gaps added by hand | Nearly all of the C API |
| Performance | Same | Same |

## Installation

```toml
[dependencies]
aeron-glide = "0.4"
```

| Feature | Default | What it adds |
|---|---|---|
| `driver` | yes | The embedded C media driver (`MediaDriver`). Without it only the client is built; run a driver separately. |
| `archive` | no | The Aeron Archive client (needs Java 17+ to build). |
| `bin` | no | The `mediadriver` binary (`cargo install aeron-glide --features bin`). |

### Prerequisites

- **Rust 1.97+**
- **CMake 3.30+** (Aeron 1.53 requires it; Debian 12 ships 3.25, so install a
  newer one from cmake.org or pip)
- **A C++17 compiler**
- **Java 17+**, only with the `archive` feature
- On Linux, `libbsd` and `libuuid` headers are recommended (`libbsd-dev
  uuid-dev` on Debian/Ubuntu): Aeron uses them when CMake finds them

The build script downloads the Aeron source release from GitHub, checks its
SHA-256 and compiles it on the first build. With the `archive` feature, Aeron's
build also runs its Gradle wrapper, which downloads Gradle and Java
dependencies that the source checksum does not cover (and writes to
`~/.gradle`); build with a warm Gradle cache or a mirror if that matters to
you. Environment variables:

| Variable | Effect |
|---|---|
| `AERON_SOURCE_DIR` | Build from this Aeron source tree instead of downloading (offline builds). With `archive`, Aeron's Gradle build of the jar runs in that tree and needs the network unless Gradle's cache is warm |
| `AERON_VERSION` | Another Aeron release (the generated bindings target 1.53.3) |
| `AERON_SHA256` | The expected SHA-256 of the downloaded tarball (overrides the built-in one) |
| `AERON_GLIDE_SANITIZER` | Build Aeron and the shims with `-fsanitize=<value>`, e.g. `address` |

CI tests Linux (x86_64 and arm64) and macOS. Windows builds are experimental.

## Thread safety

| Type | `Send` | `Sync` |
|---|---|---|
| `AeronClient`, `Publication`, `Counter`, `CountersReader`, `CncFile`, `MediaDriver`, `AeronArchive` | yes | yes |
| `ExclusivePublication`, `Subscription`, `PersistentSubscription`, `ReplayMerge` | yes | no |
| `Image` (borrows its `Subscription`) | no | no |

Share one client per process (`Arc<AeronClient>`) and add resources from any
thread. A concurrent `Publication` can be offered to from several threads;
move exclusive publications and subscriptions to the thread that uses them.

A client in agent invoker mode (`Context::use_conductor_agent_invoker`) has no
conductor thread: call `AeronClient::invoke` (or run a `ClientAgent` with an
`AgentRunner`) to do its work. Calls from other threads are serialised with
it.

## Handlers and panics

Closures run on Aeron's threads or inside Aeron calls, so a panic must not
unwind through C++:

- **Poll handlers** (`poll`, `poll_assembled`, `controlled_poll`, ...): the
  panic is caught and resumed once Aeron returns from the poll. The remaining
  fragments of that poll are consumed without being delivered; a controlled
  handler's fragment is aborted instead, so the next poll delivers it again.
- **Reserved value suppliers** (`offer_with_reserved_value`): the message is
  still published, and the panic is resumed afterwards.
- **Client, driver and archive handlers** (error handlers, image and counter
  handlers, termination hooks, recording signals, ...): the panic is caught
  and printed to stderr, since they run on Aeron's conductor threads.

Handlers may drop the objects they own, including the client: the drop is
moved off Aeron's thread when it would otherwise join or free the thread it
runs on. Calls Aeron cannot make from inside a handler (for example adding or
removing client handlers, waiting for an asynchronous add, or a blocking
archive request from an archive handler) fail with
an error instead of deadlocking.

## Examples

The `embedded_*`, `streaming_rate`, `file_transfer`, `multi_destination`,
`non_blocking_publisher` and `response_channel` examples start their own media
driver. The archive examples use the archive server's driver. The others need a
media driver; start one in its own terminal:

```bash
cargo run --features bin --bin mediadriver                              # defaults
cargo run --features bin --bin mediadriver -- examples/mediadriver.yaml # from a config
```

| Example | Shows |
|---|---|
| `ping` / `pong` | Round trips over IPC or UDP; `--exclusive` and `--zero-copy` (`try_claim`) |
| `large_ping` / `large_pong` | Messages larger than the MTU: reassembly and controlled polling |
| `throughput` / `latency` | IPC throughput and UDP latency benchmarks |
| `counters` | The driver's counters |
| `image_demo` | Images and their positions |
| `response_channel` | Request/response over response channels |
| `basic_publisher` / `basic_subscriber` | Aeron's basic samples: publish once a second, print what arrives |
| `non_blocking_publisher` | Asynchronous adds polled from your own loop, and a publisher `Agent` on an `AgentRunner` |
| `driver_stats` | A driver's CnC file without a client: counters, liveness, the distinct error log and the loss report |
| `embedded_ping_pong` / `embedded_exclusive_ipc_throughput` | Latency and throughput with the driver in-process |
| `streaming_rate` | Streaming as fast as possible, with messages larger than the MTU |
| `file_transfer` | A file sent in `try_claim` chunks, reassembled and checksummed |
| `multi_destination` | Multi-destination cast (dynamic and manual) and a multi-destination subscription |
| `record` / `replay` / `replay_merge_demo` | Archive client (`--features archive`, archive server below) |
| `persistent_subscription` | Replay, join the live stream, fall back to replay when live is lost, rejoin |
| `recording_replication` / `recording_throughput` | Replicating a recording; recording rate and the catalog |
| `archive_error_handling` | Typed archive error codes, error responses, connect retries, closed clients |

```bash
cargo run --example pong              # terminal 2
cargo run --example ping              # terminal 3
cargo run --example ping -- --exclusive --zero-copy

# Over UDP: both sides on the same channel
cargo run --example pong -- --channel "aeron:udp?endpoint=localhost:20121"
cargo run --example ping -- --channel "aeron:udp?endpoint=localhost:20121"
```

## Archive

The archive **server** is Java only (Aeron's C and C++ APIs only include the
client), so run the Java `ArchivingMediaDriver` next to your application.
`--features archive` builds Aeron's `aeron-all` jar along with the client, and
`scripts/start-archive.sh` in this repository starts the server from it:

```bash
cargo build --features archive        # JAVA_HOME=/path/to/jdk17+ if needed
bash scripts/start-archive.sh         # terminal 1
cargo run --features archive --example record
cargo run --features archive --example replay
```

```rust
use aeron_glide::archive::{self, ReplayParams, SourceLocation};

let archive = archive::Context::new()
    .control_request_channel("aeron:udp?endpoint=localhost:8010")
    .control_response_channel("aeron:udp?endpoint=localhost:0")
    .connect()?;

let subscription_id =
    archive.start_recording("aeron:ipc", 1001, SourceLocation::Local, false)?;
archive.list_recordings(0, 100, |recording| {
    println!("{}: {}", recording.recording_id, recording.stripped_channel);
})?;
let mut replay = archive.replay(0, "aeron:ipc", 1002, &ReplayParams::new().position(0))?;
replay.poll(10, |data, _| println!("{} bytes", data.len()))?;
```

The archive tests start a Java `ArchivingMediaDriver` per test. They are
skipped when Java or the jar is missing, unless
`AERON_GLIDE_REQUIRE_ARCHIVE=1` is set (`AERON_ALL_JAR` points them to another
jar).

## Benchmarks

aeron-glide performs the same as rusteron: both spend their time in the same
Aeron C code. On an Apple M4 Pro, against one shared media driver (Aeron
1.53.3), median of three alternating rounds:

| | IPC throughput | UDP round trip p50 | p99 | p99.9 |
|---|---|---|---|---|
| aeron-glide 0.4.0 | 39.7M msgs/sec | 19.8 µs | 29.6 µs | 43.1 µs |
| rusteron 0.2.10 | 39.9M msgs/sec | 20.1 µs | 29.8 µs | 46.4 µs |

**[BENCHMARKS.md](BENCHMARKS.md)** has the method, a run pinned with
`taskset` on Linux, and how to reproduce them with
[`scripts/benchmark.py`](scripts/benchmark.py).

## Built with AI

This project would not exist without AI. aeron-glide is developed with
AI coding agents doing most of the writing, and humans deciding what to build,
setting the bar and checking the results. We say so plainly because it shaped
the project.

A safe binding for Aeron means reading, line by line, Aeron's Java, C and C++
clients and media driver; checking every wrapper against them; and asking at
each call what happens on another thread, inside a handler, after the driver
dies, or with a corrupt file. AI agents made that amount of work possible:

- they compared each API with Aeron's own code and tests, phase by phase;
- separate agents reviewed every phase adversarially, looking for ways to break
  it, and those findings were fixed and turned into regression tests;
- they ran the suite on Linux x86_64 and arm64 and under AddressSanitizer, and
  ran the benchmarks against rusteron;
- along the way they found and reproduced the Aeron bugs the shim works around.

Nothing is trusted because an AI wrote it: changes land only when the tests,
the sanitizer and the reviews agree.

If you don't want AI-developed code in your stack, this crate is not for you,
and that's fine. If you do use it, the same tools can help you read, extend or
fork it.

## Documentation

API documentation is on [docs.rs](https://docs.rs/aeron-glide), and
[CHANGELOG.md](CHANGELOG.md) lists the changes in each release.

## Minimum supported Rust version

1.97. CI also tests the latest stable Rust.

## License

> **Disclaimer:** This project is not officially associated with or endorsed
> by Adaptive Financial Consulting Ltd. (Adaptive) or the Aeron project.

Licensed under the [Apache License 2.0](LICENSE): free for any purpose,
including proprietary and closed-source use, subject to the license's
attribution and notice terms. See [NOTICE](NOTICE) for attribution details.

Unless you explicitly state otherwise, any contribution you submit for
inclusion in aeron-glide is licensed under the Apache License 2.0, without
any additional terms or conditions. See [CONTRIBUTING.md](CONTRIBUTING.md).
