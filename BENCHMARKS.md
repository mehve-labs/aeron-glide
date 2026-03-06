# Benchmarks

Performance results for aeron-glide on Apple Silicon (M-series, macOS).

## IPC Throughput

Exclusive publication, 32-byte messages, single publisher + single subscriber.

```
cargo run --release --example throughput
```

| Metric | aeron-glide | rusteron |
|--------|----------|----------|
| Throughput (msgs/sec) | ~67.5M | ~37M |
| Throughput (bytes/sec) | ~2.16 GB/s | ~1.18 GB/s |

## UDP Latency (Ping-Pong)

32-byte messages, 1M samples after 100K warmup, localhost UDP round-trip.

```
cargo run --release --example latency
```

| Percentile | RTT |
|------------|-----|
| min | 8.5 us |
| p50 | 17.5 us |
| p99 | 28.2 us |
| p99.9 | 58.1 us |
| p99.99 | 171.3 us |
| max | 3.3 ms |
| avg | 19.3 us |

## Reproducing

1. Start a media driver in a separate terminal:
   ```
   cargo run --release --bin mediadriver
   ```

2. Run the benchmark:
   ```
   cargo run --release --example throughput   # IPC throughput
   cargo run --release --example latency      # UDP latency
   ```

Results vary by hardware. The numbers above were measured on Apple M-series silicon with Aeron 1.50.2.
