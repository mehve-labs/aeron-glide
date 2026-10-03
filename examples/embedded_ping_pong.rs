//! Round-trip latency with an embedded media driver, like Aeron's
//! `EmbeddedPingPong.java`: a pong thread echoes every ping back, the main
//! thread sends pings stamped with the send time, waits for each echo and
//! records the round trip in a histogram. A warm-up run comes first.
//!
//! Self-contained (no separate media driver):
//!
//! ```text
//! cargo run --release --example embedded_ping_pong
//! cargo run --release --example embedded_ping_pong -- --messages 10000 --warmup 1000
//! ```

use aeron_glide::concurrent::{BusySpinIdleStrategy, IdleStrategy};
use aeron_glide::{AeronClient, Context, MediaDriver, OfferError, ThreadingMode};
use clap::Parser;
use hdrhistogram::Histogram;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, Instant};

const PING_CHANNEL: &str = "aeron:udp?endpoint=localhost:20123";
const PONG_CHANNEL: &str = "aeron:udp?endpoint=localhost:20124";
const PING_STREAM_ID: i32 = 1002;
const PONG_STREAM_ID: i32 = 1003;
const FRAGMENT_COUNT_LIMIT: i32 = 10;

#[derive(Parser)]
#[command(about = "Ping-pong latency with an embedded media driver (EmbeddedPingPong)")]
struct Args {
    /// Pings to measure.
    #[arg(short, long, default_value_t = 100_000)]
    messages: usize,
    /// Pings sent before measuring.
    #[arg(short, long, default_value_t = 10_000)]
    warmup: usize,
    /// Message length in bytes (at least 8, for the timestamp).
    #[arg(short, long, default_value_t = 32)]
    length: usize,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();
    assert!(args.length >= 8, "messages carry an 8-byte timestamp");
    let dir = std::env::temp_dir().join(format!("aeron-glide-pingpong-{}", std::process::id()));
    let driver = MediaDriver::builder()
        .dir(&dir.to_string_lossy())
        .dir_delete_on_start(true)
        .dir_delete_on_shutdown(true)
        .threading_mode(ThreadingMode::Dedicated)
        .start()?;
    // One client, shared by both threads.
    let client = AeronClient::connect(Context::new().aeron_dir(driver.dir()))?;
    println!("Publishing Ping at {PING_CHANNEL} on stream id {PING_STREAM_ID}");
    println!("Subscribing Pong at {PONG_CHANNEL} on stream id {PONG_STREAM_ID}");
    println!("Message length of {} bytes", args.length);

    let running = AtomicBool::new(true);
    thread::scope(|scope| -> Result<(), Box<dyn std::error::Error>> {
        scope.spawn(|| pong(&client, &running));
        let result = ping(&client, &args);
        running.store(false, Ordering::Release);
        result
    })
}

/// Echo each ping back on the pong stream until `running` is cleared.
fn pong(client: &AeronClient, running: &AtomicBool) {
    let mut pings = client
        .add_subscription(PING_CHANNEL, PING_STREAM_ID)
        .expect("ping subscription");
    let mut pongs = client
        .add_exclusive_publication(PONG_CHANNEL, PONG_STREAM_ID)
        .expect("pong publication");
    let mut idle = BusySpinIdleStrategy;
    while running.load(Ordering::Acquire) {
        let fragments = pings
            .poll(FRAGMENT_COUNT_LIMIT, |data, _| {
                while !sent(pongs.offer(data)) {
                    std::hint::spin_loop();
                }
            })
            .expect("poll pings");
        idle.idle(fragments);
    }
}

fn ping(client: &AeronClient, args: &Args) -> Result<(), Box<dyn std::error::Error>> {
    let mut pings = client.add_exclusive_publication(PING_CHANNEL, PING_STREAM_ID)?;
    let mut pongs = client.add_subscription(PONG_CHANNEL, PONG_STREAM_ID)?;
    println!("Waiting for new image from Pong...");
    let deadline = Instant::now() + Duration::from_secs(10);
    while !(pings.is_connected() && pongs.is_connected()) {
        assert!(Instant::now() < deadline, "ping and pong did not connect");
        thread::sleep(Duration::from_millis(1));
    }

    let epoch = Instant::now();
    let mut message = vec![0u8; args.length];
    let mut histogram = Histogram::<u64>::new_with_bounds(1, 10_000_000_000, 3)?;
    for (phase, count) in [("Warming up", args.warmup), ("Pinging", args.messages)] {
        println!("{phase}: {count} messages");
        histogram.reset();
        for _ in 0..count {
            let sent_at = epoch.elapsed().as_nanos() as u64;
            message[..8].copy_from_slice(&sent_at.to_le_bytes());
            while !sent(pings.offer(&message)) {
                std::hint::spin_loop();
            }
            // Busy-poll until the echo is back.
            let mut echoed = 0;
            while echoed == 0 {
                echoed = pongs.poll(FRAGMENT_COUNT_LIMIT, |data, _| {
                    let sent_at = u64::from_le_bytes(data[..8].try_into().unwrap());
                    let rtt = epoch.elapsed().as_nanos() as u64 - sent_at;
                    histogram.saturating_record(rtt);
                })?;
            }
        }
    }

    println!(
        "Round trip latency (microseconds) over {} pings:",
        histogram.len()
    );
    for percentile in [50.0, 90.0, 99.0, 99.9, 99.99] {
        let nanos = histogram.value_at_percentile(percentile);
        println!("  p{percentile:<6} {:>10.2}", nanos as f64 / 1000.0);
    }
    println!("  max     {:>10.2}", histogram.max() as f64 / 1000.0);
    Ok(())
}

/// `true` once offered, `false` to retry (back pressure, not connected yet, ...).
/// Errors that retrying cannot fix (e.g. a message too long) end the example.
fn sent(result: Result<i64, OfferError>) -> bool {
    match result {
        Ok(_) => true,
        Err(e) if e.is_retryable() => false,
        Err(e) => panic!("offer failed: {e}"),
    }
}
