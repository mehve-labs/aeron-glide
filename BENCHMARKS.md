# Benchmarks

Measured on an Apple M4 Pro (12 cores, 48 GB), macOS 27, Rust 1.99, Aeron
1.53.3, with release builds. The driver is the `mediadriver` binary with its
default settings, shared by every run. Each benchmark ran three times,
alternating with the same examples built from the previous release (0.3.1,
also on Aeron 1.53.3), and the tables give the median.

## UDP latency (ping-pong)

32-byte messages over localhost UDP, 1M round trips after 100K warm-up.

```
cargo run --release --example latency
```

| Round trip | 0.4 | 0.3.1 |
|---|---|---|
| min | 10.4 µs | 10.3 µs |
| p50 | 20.8 µs | 20.8 µs |
| p99 | 30.9 µs | 29.9 µs |
| p99.9 | 43.8 µs | 43.0 µs |
| p99.99 | 71.0 µs | 71.6 µs |
| mean | 21.0 µs | 21.0 µs |

Latency is unchanged: the round trip is dominated by the kernel's UDP path,
not by the bindings.

## IPC throughput

32-byte messages on an exclusive IPC publication, one publisher thread and one
subscriber thread, each with its own client.

```
cargo run --release --example throughput
```

| | 0.4 | 0.3.1 |
|---|---|---|
| Messages per second | 35.5M | 58.0M |

(Measured before `offer` called Aeron's C function directly, which raised 0.4
to about 41M in the comparison below.)

This number measures the publisher and subscriber racing over the same cache
lines more than it measures the bindings, so read it with care:

- The publisher is the bottleneck in both versions: back pressure is almost
  zero (about one offer in a million in 0.4).
- 0.4's subscriber does less work per poll (0.3.1 looked its handler up in a
  thread-local registry on every poll), so it follows the publisher
  more closely and reads each cache line right after it is written. The
  publisher's copy into the log buffer then waits for those lines to come
  back: in a profile, `memmove` takes 38% of the publisher's time in 0.4 and
  23% in 0.3.1, with the same code around it.
- Slowing 0.4's subscriber down by a 200 ns spin after each poll raises the
  rate to about 65M messages per second, above 0.3.1.
- Run to run, 0.3.1 ranged from 40M to 58M on the same machine.

For comparable numbers, measure your own message sizes and threading, ideally
with both ends on separate cores.

## Other benchmarks

`embedded_ping_pong` and `embedded_exclusive_ipc_throughput` run the same kind
of tests with the media driver inside the process.

## rusteron

Measured in a separate session (the absolute latency differs from the tables
above, so compare only within this table). Both clients ran against the same
`mediadriver` (Aeron 1.53.3); rusteron 0.2.10 builds Aeron 1.52.2 and was linked
statically, as its README recommends. aeron-glide ran its `throughput` and
`latency` examples, rusteron its `embedded_exclusive_ipc_throughput` and
`embedded_ping_pong` examples, which do the same work: 32-byte messages on an
exclusive IPC publication with a poll limit of 32, and a UDP ping-pong on the
same ports with a fragment limit of 10 (its 10M round trips changed to 1M, like ours). Three
alternating rounds, medians:

| | aeron-glide 0.4 | rusteron 0.2.10 |
|---|---|---|
| IPC throughput | 41.0M msgs/sec | 39.9M msgs/sec |
| UDP round trip p50 | 50.7 µs | 51.7 µs |
| UDP round trip p99 | 85.4 µs | 87.6 µs |
| UDP round trip p99.9 | 137.1 µs | 143.2 µs |

The two are equivalent: both spend their time in the same Aeron C code. One
difference in the harnesses: rusteron's throughput example uses one client for
both ends, ours one client per end. The throughput caveats above apply to both.

## Reproducing

```bash
cargo build --release --features bin --bin mediadriver --examples
target/release/mediadriver &       # one shared driver
target/release/examples/throughput # Ctrl-C after ~10 s
target/release/examples/latency
```
