# Benchmarks

How aeron-glide performs, compared with [rusteron](https://github.com/gsrxyz/rusteron)
and with aeron-glide 0.3.1, and how the numbers were measured. In short:
aeron-glide and rusteron perform the same, since both spend their time in the
same Aeron C code. aeron-glide's latency tail was slightly lower in these runs.

## Method

- **One media driver for everyone.** Every contender connects to the same
  running media driver: this crate's `mediadriver` binary, Aeron 1.53.3, with
  its default settings. Only the client libraries differ.
- **The same work on both sides.** aeron-glide runs its `throughput` and
  `latency` examples, and rusteron its `embedded_exclusive_ipc_throughput` and
  `embedded_ping_pong` examples. Despite their names, these also connect to an
  external driver through `AERON_DIR`. The pairs do the same thing:
  - **Throughput:** 32-byte messages on an exclusive IPC publication. A
    publisher thread offers as fast as it can, and a subscriber thread polls
    up to 32 fragments at a time. We take the median of the per-second rates
    over 12 seconds, after 2 seconds of warm-up.
  - **Latency:** a UDP ping-pong on `localhost:20123/20124` with 32-byte
    messages and a fragment limit of 10, measuring 1M round trips after 100K of
    warm-up. rusteron's example measures 10M; we changed that constant to 1M to
    match.
- **One client or two.** rusteron's throughput example uses one client for
  both ends. aeron-glide's uses one client per end by default, and
  `--shared-client` uses one for both, so both setups are measured.
- **Rounds.** The contenders run one after another, in alternating order each
  round, with nothing else running on the machine. Each table gives the median
  of three rounds.
- **Builds.**
  - aeron-glide 0.4.0: release build, Rust 1.99.
  - rusteron 0.2.10: release build with its `static` feature, as its README
    recommends, using its pinned Rust 1.95 and its bundled Aeron 1.52.2
    client.
- **Script.** [`scripts/benchmark.py`](scripts/benchmark.py) runs all of this;
  see [Reproducing](#reproducing).

## Results on macOS

Apple M4 Pro (8 performance and 4 efficiency cores, 48 GB), macOS 27.

| | Throughput | p50 | p99 | p99.9 | p99.99 |
|---|---|---|---|---|---|
| aeron-glide 0.4 | 39.7M msgs/sec | 19.8 µs | 29.6 µs | 43.1 µs | 81.3 µs |
| aeron-glide 0.4, one client | 39.9M msgs/sec | | | | |
| rusteron 0.2.10 | 39.9M msgs/sec | 20.1 µs | 29.8 µs | 46.4 µs | 118.7 µs |

macOS cannot pin threads to cores: `taskset` does not exist, Apple Silicon
ignores thread affinity hints, and `taskpolicy` can only lower a process's
priority. The benchmarks ran as ordinary foreground processes. Busy-spinning
threads at that priority run on the performance cores in practice, but the
scheduler decides.

## Results on Linux, pinned

The same benchmarks in a Debian 12 container (arm64) under Docker Desktop on
the same Mac, with 12 virtual CPUs and `--shm-size=2g`. With `taskset`, the
driver runs on CPU 1, and each benchmark process (publisher and subscriber, or
ping and pong) runs on CPUs 2 and 3, the same for every contender. These are
the virtual machine's CPUs, which macOS still schedules onto physical cores.
That is why both libraries show the same 4 ms p99.99: the virtual machine
pausing, not either library.

| | Throughput | p50 | p99 | p99.9 | p99.99 |
|---|---|---|---|---|---|
| aeron-glide 0.4 | 41.5M msgs/sec | 22.4 µs | 33.2 µs | 62.8 µs | 4.0 ms |
| aeron-glide 0.4, one client | 41.4M msgs/sec | | | | |
| rusteron 0.2.10 | 41.8M msgs/sec | 22.4 µs | 33.2 µs | 83.1 µs | 4.0 ms |

On a Linux host, pin to isolated physical cores (e.g. `isolcpus`) for numbers
that hold for production.

## Compared with aeron-glide 0.3.1

Measured on macOS, against the 0.3.1 release built with the same Aeron 1.53.3
(its examples are equivalent).

| | Throughput, per round | p50 | p99 | p99.9 |
|---|---|---|---|---|
| 0.4 | 39.7M, 39.6M, 39.4M | 20.0 µs | 29.4 µs | 42.0 µs |
| 0.3.1 | 79.3M, 49.5M, 40.4M | 20.3 µs | 29.8 µs | 43.8 µs |

Latency is the same. 0.3.1's throughput is not higher, it is unstable.

- Back pressure is close to zero in both versions, so the publisher sets the
  pace, and the rate depends on how its thread and the subscriber's share cache
  lines.
- 0.4's subscriber does less work per poll: 0.3.1 looked its handler up in a
  thread-local registry each time. So it follows the publisher closely and
  reads each cache line just after it is written, and the publisher waits for
  those lines. In a profile, the copy into the log buffer takes 38% of the
  publisher's time in 0.4, and 23% in 0.3.1.
- 0.3.1's slower subscriber sometimes stays far enough behind to avoid that,
  and sometimes not, hence 40M to 79M from one run to the next.
- Making 0.4's subscriber wait 200 ns after each poll gives about 65M messages
  per second.

A throughput number like this measures how two threads share a cache more than
how fast a library is. Measure your own message sizes, threads and core
placement.

## Reproducing

On macOS or Linux:

```bash
cargo build --release --features bin --bin mediadriver --examples
python3 scripts/benchmark.py --glide target/release --rounds 3
```

To include rusteron, build its examples from a checkout (with its Aeron
submodules) and pass their directory:

```bash
git clone https://github.com/gsrxyz/rusteron && cd rusteron
git submodule update --init --depth 1 rusteron-client/aeron rusteron-media-driver/aeron
# optional: in rusteron-client/examples/embedded_ping_pong.rs,
# set NUMBER_OF_MESSAGES to 1_000_000 to match aeron-glide's latency example
cargo build --release -p rusteron-client --features "examples static" \
    --example embedded_exclusive_ipc_throughput --example embedded_ping_pong
cd ../aeron-glide
python3 scripts/benchmark.py --glide target/release \
    --rusteron ../rusteron/target/release/examples
```

On Linux, add `--driver-cpus 1 --bench-cpus 2,3` to pin with `taskset`. In a
container, give it a larger `/dev/shm` (`docker run --shm-size=2g`): Aeron's
log buffers do not fit in the default 64 MB. rusteron's build needs libclang
(`libclang-dev`).

The script prints a table like the ones above, and `--json` writes the
per-round results.

Other benchmarks in the examples: `embedded_ping_pong` and
`embedded_exclusive_ipc_throughput` run the driver inside the process.
