#!/usr/bin/env python3
"""Throughput and latency of aeron-glide (and optionally rusteron) on one machine.

Every contender connects to the same media driver (this crate's `mediadriver`
binary), so only the client libraries differ. The contenders run in turn, in
alternating order each round, and the report gives the median of the rounds.

    cargo build --release --features bin --bin mediadriver --examples
    python3 scripts/benchmark.py --glide target/release --rounds 3

With a rusteron checkout built with
`cargo build --release -p rusteron-client --features "examples static"
--example embedded_exclusive_ipc_throughput --example embedded_ping_pong`:

    python3 scripts/benchmark.py --glide target/release \\
        --rusteron ../rusteron/target/release/examples

On Linux, `--driver-cpus` and `--bench-cpus` pin the driver and the benchmark
processes with `taskset` (e.g. `--driver-cpus 1 --bench-cpus 2,3`).
"""

import argparse
import json
import os
import re
import signal
import statistics
import subprocess
import sys
import tempfile
import time

LATENCY_KEYS = {
    "min": "min",
    "50th percentile": "p50",
    "99th percentile": "p99",
    "99.9th percentile": "p99.9",
    "99.99th percentile": "p99.99",
    "max": "max",
    "avg": "mean",
}
UNITS_US = {"ns": 1e-3, "µs": 1.0, "us": 1.0, "ms": 1e3, "s": 1e6}


def micros(text):
    """Microseconds from a Rust `Duration` debug string such as `20.8µs`."""
    m = re.match(r"([\d.]+)(ns|µs|us|ms|s)$", text.strip())
    return float(m.group(1)) * UNITS_US[m.group(2)] if m else None


def pinned(command, cpus):
    return ["taskset", "-c", cpus] + command if cpus else command


def run(command, env, seconds):
    """Run `command`; after `seconds`, stop it with Ctrl-C. Returns its output."""
    process = subprocess.Popen(command, env=env, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True)
    try:
        output, _ = process.communicate(timeout=seconds)
    except subprocess.TimeoutExpired:
        process.send_signal(signal.SIGINT)
        try:
            output, _ = process.communicate(timeout=20)
        except subprocess.TimeoutExpired:
            process.kill()
            output, _ = process.communicate()
    return output


def throughput(output, warmup):
    """Median of the per-second rates, skipping the first `warmup` reports."""
    rates = [float(r.replace(",", "")) for r in re.findall(r"Throughput: ([\d,]+) msgs/sec", output)]
    rates = rates[warmup:]
    return statistics.median(rates) if rates else None


def latency(output):
    result = {}
    for line in output.splitlines():
        key, _, value = line.partition(":")
        if key.strip() in LATENCY_KEYS:
            result[LATENCY_KEYS[key.strip()]] = micros(value)
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--glide", required=True, help="aeron-glide target dir (e.g. target/release)")
    parser.add_argument("--rusteron", help="rusteron examples dir (optional)")
    parser.add_argument("--driver", help="media driver binary (default: <glide>/mediadriver)")
    parser.add_argument("--rounds", type=int, default=3)
    parser.add_argument("--seconds", type=int, default=12, help="length of each throughput run")
    parser.add_argument("--warmup", type=int, default=2, help="throughput reports to skip")
    parser.add_argument("--driver-cpus", help="taskset CPU list for the driver (Linux)")
    parser.add_argument("--bench-cpus", help="taskset CPU list for the benchmarks (Linux)")
    parser.add_argument("--no-latency", action="store_true")
    parser.add_argument("--json", help="also write the raw results here")
    args = parser.parse_args()

    examples = os.path.join(args.glide, "examples")
    contenders = [
        ("aeron-glide", [os.path.join(examples, "throughput")], [os.path.join(examples, "latency")]),
        ("aeron-glide, one client", [os.path.join(examples, "throughput"), "--shared-client"], None),
    ]
    if args.rusteron:
        contenders.append(
            (
                "rusteron",
                [os.path.join(args.rusteron, "embedded_exclusive_ipc_throughput")],
                [os.path.join(args.rusteron, "embedded_ping_pong")],
            )
        )

    shm = "/dev/shm" if os.path.isdir("/dev/shm") else None
    aeron_dir = tempfile.mkdtemp(prefix="aeron-glide-bench-", dir=shm)
    env = dict(os.environ, AERON_DIR=aeron_dir)
    env.pop("SDKROOT", None)
    driver = subprocess.Popen(
        pinned([args.driver or os.path.join(args.glide, "mediadriver")], args.driver_cpus),
        env=env,
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
    )
    time.sleep(2)

    results = {name: {"throughput": [], "latency": []} for name, _, _ in contenders}
    try:
        for round_ in range(args.rounds):
            order = contenders if round_ % 2 == 0 else list(reversed(contenders))
            for name, tp_command, lat_command in order:
                rate = throughput(run(pinned(tp_command, args.bench_cpus), env, args.seconds), args.warmup)
                results[name]["throughput"].append(rate)
                line = f"round {round_ + 1} {name}: {rate:,.0f} msgs/sec" if rate else f"round {round_ + 1} {name}: no result"
                if lat_command and not args.no_latency:
                    lat = latency(run(pinned(lat_command, args.bench_cpus), env, 600))
                    results[name]["latency"].append(lat)
                    line += f", p50 {lat.get('p50')} µs"
                print(line, flush=True)
    finally:
        driver.send_signal(signal.SIGINT)
        driver.wait(timeout=20)

    def median(values):
        values = [v for v in values if v is not None]
        return statistics.median(values) if values else None

    print("\n| | Throughput (msgs/sec) | p50 (µs) | p99 (µs) | p99.9 (µs) | p99.99 (µs) |")
    print("|---|---|---|---|---|---|")
    for name, _, _ in contenders:
        r = results[name]
        tp = median(r["throughput"])
        cells = [f"{tp / 1e6:.1f}M" if tp else "-"]
        for key in ("p50", "p99", "p99.9", "p99.99"):
            value = median([lat.get(key) for lat in r["latency"]])
            cells.append(f"{value:.1f}" if value else "-")
        print(f"| {name} | " + " | ".join(cells) + " |")
    if args.json:
        with open(args.json, "w") as f:
            json.dump(results, f, indent=1)
    return 0


if __name__ == "__main__":
    sys.exit(main())
