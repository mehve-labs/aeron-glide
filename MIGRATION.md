# Migration guide

- [From aeron-glide 0.3 to 0.4](#from-aeron-glide-03-to-04)
- [From rusteron](#from-rusteron)

[CHANGELOG.md](CHANGELOG.md) lists every change; this guide shows how to
update code.

## From aeron-glide 0.3 to 0.4

### Cargo features

The embedded media driver is behind the `driver` feature, which is on by
default. The `mediadriver` binary needs the `bin` feature:

```bash
cargo install aeron-glide --features bin   # was: cargo install aeron-glide
```

Client-only applications can use `default-features = false`, which skips
building the C driver.

The build needs CMake 3.30+ and a C++17 compiler. It no longer needs OpenSSL.

### Errors

Fallible calls return `aeron_glide::Result<T>`, with an `aeron_glide::Error`
instead of `Box<dyn Error>`. `Error` converts into `Box<dyn Error>`, so `?` in
functions returning `Box<dyn Error>` keeps working. Match on `e.kind()`
(`ErrorKind::DriverTimeout`, `ErrorKind::IllegalArgument`, ...) and read
Aeron's error code with `e.code()`.

Calls that used to abort the process on an Aeron error now return it:
`poll` and `poll_assembled` (subscriptions, images, `ReplayMerge`),
`Image::position`, `CountersReader::for_each` and the `get_counter_*` readers.

### Publishing

`offer` returns `Result<i64, OfferError>` instead of a position that is
negative on failure. Retry the retryable errors, and handle the others:

```rust
// 0.3
while publication.offer(message) < 0 {}

// 0.4
while let Err(e) = publication.offer(message) {
    if !e.is_retryable() {
        return Err(e.into()); // e.g. OfferError::Closed, or a message too long
    }
}
```

`try_claim` returns a `BufferClaim` guard instead of taking a closure.
Dropping the guard aborts the claim:

```rust
// 0.3
publication.try_claim(message.len(), |buffer| {
    buffer.copy_from_slice(message);
    true // commit
});

// 0.4
let mut claim = publication.try_claim(message.len())?;
claim.buffer_mut().copy_from_slice(message);
claim.commit();
```

`AeronClient::add_*`, `offer` and `try_claim` take `&self`, so a client and a
concurrent `Publication` can be shared through an `Arc`. `AeronClient::start`
is removed: the client starts when it connects.

### Subscribing

Fragment handlers get the fragment's `Header` as a second argument, and polls
return a `Result`:

```rust
// 0.3
let fragments = subscription.poll(10, |data| println!("{data:?}"));

// 0.4
let fragments = subscription.poll(10, |data, _header| println!("{data:?}"))?;
```

`image_by_index` and `image_by_session_id` return `Option<Image>`. An `Image`
borrows its `Subscription`, so poll it before using the subscription mutably
again.

### Channel URIs

`ChannelBuilder::build` validates the URI and returns `Result<String>`. Some
setters are renamed to their C++ names:

| 0.3 | 0.4 |
|---|---|
| `interface` | `network_interface` |
| `control` | `control_endpoint` |
| `socket_sndbuf` | `socket_sndbuf_length` |
| `socket_rcvbuf` | `socket_rcvbuf_length` |
| `receiver_window` | `receiver_window_length` |

`control_mode` takes a `ControlMode`, `linger` a `Duration`, and `mtu` and
`term_length` a `u32`.

### Embedded media driver

Configure the driver with a builder; it can't be reconfigured once started:

```rust
// 0.3
let mut driver = MediaDriver::new()?;
driver.set_dir("/dev/shm/my-app")?;
driver.set_threading_mode(ThreadingMode::Shared)?;
driver.start()?;

// 0.4
let driver = MediaDriver::builder()
    .dir("/dev/shm/my-app")
    .threading_mode(ThreadingMode::Shared)
    .start()?; // reports the first invalid setting
```

`MediaDriver::launch()` starts one with the defaults. The idle strategy enum
for the driver's threads is renamed `DriverIdleStrategy` (`IdleStrategy` is now
the trait in `aeron_glide::concurrent`).

### Archive client

Connect with an `archive::Context`. `AeronArchive` methods take `&self`, and
`start_replay` takes `ReplayParams`:

```rust
// 0.3
let mut archive = AeronArchive::connect(
    "aeron:udp?endpoint=localhost:8010", 10,
    "aeron:udp?endpoint=localhost:0", 20,
)?;
archive.start_replay(recording_id, "aeron:ipc", 1002, position, length)?;

// 0.4
let archive = archive::Context::new()
    .control_request_channel("aeron:udp?endpoint=localhost:8010")
    .control_response_channel("aeron:udp?endpoint=localhost:0")
    .connect()?;
archive.start_replay(
    recording_id,
    "aeron:ipc",
    1002,
    &ReplayParams::new().position(position).length(length),
)?;
```

`ReplayMerge::new` borrows the subscription and the archive client mutably for
as long as the merge lives. Drop the merge before using them again.

## From rusteron

[rusteron](https://github.com/gsrxyz/rusteron) binds Aeron's C API; aeron-glide
wraps the C++ API. The concepts are the same, so most code maps one to one.

| rusteron | aeron-glide |
|---|---|
| `AeronContext` + `Aeron::new(&ctx)` + `aeron.start()` | `AeronClient::connect(Context::new()...)`, or `AeronClient::new()` |
| `async_add_publication(..)?.poll_blocking(timeout)` | `add_publication(channel, stream_id)` (blocking), or `add_publication_async` and `PendingAdd::poll` |
| `publication.offer(..)` returning `AeronOfferError` | `publication.offer(..)` returning `OfferError` |
| `try_claim` with an `AeronBufferClaim` | `try_claim(length)` returning a `BufferClaim` guard |
| `subscription.poll_fn(handler, limit)` | `subscription.poll(limit, handler)` |
| `AeronFragmentAssembler` / `AeronControlledFragmentAssembler` | `poll_assembled` / `controlled_poll` |
| `AeronCountersReader` | `CountersReader` (`AeronClient::counters_reader`), or `CncFile` without a client |
| `AeronDriver::launch_embedded(..)` with an `AeronDriverContext` | `MediaDriver::builder()...start()` |
| `AeronArchiveContext` + `AeronArchiveAsyncConnect` | `archive::Context` + `connect()` / `connect_async()` |
| C strings (`c"aeron:ipc"`) | `&str` |

Differences to keep in mind:

- **Ownership:** resources keep the client alive, and closing them is
  dropping them. There is no `close()` to call, and nothing is undefined
  behaviour if you drop things in another order.
- **Threads:** `Send`/`Sync` follow Aeron's thread-safety rules without a
  feature flag (`AeronClient` and `Publication` are `Sync`; `Subscription`
  and `ExclusivePublication` are `Send` only).
- **Handlers:** closures are plain Rust closures. A panic is caught and
  resumed after the poll, or reported for handlers on Aeron's threads,
  instead of aborting the process.
- **Coverage:** aeron-glide exposes a curated API. If you rely on a C function
  it doesn't wrap, please open an issue.
