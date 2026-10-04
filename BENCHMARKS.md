# Benchmarks

How aeron-glide performs, compared with [rusteron](https://github.com/gsrxyz/rusteron),
and how the numbers were measured. In short: aeron-glide and rusteron perform
the same. Both run against the same media driver, and on the hot path both call
Aeron's C client functions (rusteron's bundled Aeron 1.52.2, aeron-glide's
1.53.3); the differences below are within the run-to-run noise.

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
| aeron-glide 0.4.0 | 39.7M msgs/sec | 19.8 µs | 29.6 µs | 43.1 µs | 81.3 µs |
| aeron-glide 0.4.0, one client | 39.9M msgs/sec | | | | |
| rusteron 0.2.10 | 39.9M msgs/sec | 20.1 µs | 29.8 µs | 46.4 µs | 118.7 µs |

macOS cannot pin threads to cores: `taskset` does not exist, Apple Silicon
ignores thread affinity hints, and `taskpolicy` can only lower a process's
priority. The benchmarks ran as ordinary foreground processes. Busy-spinning
threads at that priority run on the performance cores in practice, but the
scheduler decides.

## Results on Linux, pinned

The same benchmarks in a Debian 12 container (arm64) under Docker Desktop on
the same Mac, with 12 virtual CPUs and `--shm-size=2g`. With `taskset`, the
driver (its conductor, sender and receiver threads) runs on CPUs 1, 4 and 5,
and each benchmark process (publisher and subscriber, or ping and pong) on
CPUs 2 and 3, the same for every contender. These are the virtual machine's
CPUs, which macOS still schedules onto physical cores. That is why both
libraries show the same 4 ms p99.99: the virtual machine pausing, not either
library.

| | Throughput | p50 | p99 | p99.9 | p99.99 |
|---|---|---|---|---|---|
| aeron-glide 0.4.0 | 42.3M msgs/sec | 2.8 µs | 5.4 µs | 11.5 µs | 4.0 ms |
| aeron-glide 0.4.0, one client | 42.1M msgs/sec | | | | |
| rusteron 0.2.10 | 42.7M msgs/sec | 2.8 µs | 5.5 µs | 11.6 µs | 4.0 ms |

Pinning matters: with all three driver threads on one CPU, the round trip
was 22.4 µs at p50, for both libraries.

On a Linux host, pin to isolated physical cores (e.g. `isolcpus`) for numbers
that hold for production.

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

On Linux, add `--driver-cpus 1,4,5 --bench-cpus 2,3` to pin with `taskset`
(give the driver a CPU per thread: it runs three in its default mode). In a
container, give it a larger `/dev/shm` (`docker run --shm-size=2g`): Aeron's
log buffers do not fit in the default 64 MB. rusteron's build needs libclang
(`libclang-dev`).

The script prints a table like the ones above, and `--json` writes the
per-round results.

Other benchmarks in the examples: `embedded_ping_pong` and
`embedded_exclusive_ipc_throughput` run the driver inside the process.
