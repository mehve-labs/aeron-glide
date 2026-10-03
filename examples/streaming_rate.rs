//! A publisher streaming messages flat out and a subscriber reporting the
//! receive rate once a second, like Aeron's `streaming_publisher.c` +
//! `rate_subscriber.c` (`StreamingPublisher.java` + `RateSubscriber.java`),
//! in one process with an embedded media driver over UDP on localhost.
//!
//! The publisher retries back pressure with a busy-spin idle strategy and
//! reports its back pressure ratio; the subscriber reassembles fragmented
//! messages, so `--length` may exceed the MTU.
//!
//! Self-contained:
//!
//! ```text
//! cargo run --release --example streaming_rate
//! cargo run --release --example streaming_rate -- --messages 1000000 --length 4096
//! ```

use aeron_glide::concurrent::{BusySpinIdleStrategy, IdleStrategy, YieldingIdleStrategy};
use aeron_glide::{AeronClient, Context, MediaDriver, OfferError, ThreadingMode};
use clap::Parser;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::thread;
use std::time::{Duration, Instant};

#[derive(Parser)]
#[command(
    about = "Streaming publisher + rate subscriber (streaming_publisher.c + rate_subscriber.c)"
)]
struct Args {
    /// The channel to stream on.
    #[arg(short, long, default_value = "aeron:udp?endpoint=localhost:20161")]
    channel: String,
    /// The stream ID.
    #[arg(short, long, default_value_t = 1001)]
    stream_id: i32,
    /// How many messages to send.
    #[arg(short, long, default_value_t = 2_000_000)]
    messages: u64,
    /// Message length in bytes.
    #[arg(short = 'L', long, default_value_t = 32)]
    length: usize,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();
    let dir = std::env::temp_dir().join(format!("aeron-glide-streaming-{}", std::process::id()));
    let driver = MediaDriver::builder()
        .dir(&dir.to_string_lossy())
        .dir_delete_on_start(true)
        .dir_delete_on_shutdown(true)
        .threading_mode(ThreadingMode::Dedicated)
        .start()?;
    let client = AeronClient::connect(Context::new().aeron_dir(driver.dir()))?;
    let mut subscription = client.add_subscription(&args.channel, args.stream_id)?;
    let publication = client.add_publication(&args.channel, args.stream_id)?;
    println!(
        "Streaming {} messages of payload length {} bytes to {} on stream id {}",
        args.messages, args.length, args.channel, args.stream_id
    );

    let (messages, bytes) = (AtomicU64::new(0), AtomicU64::new(0));
    // Cleared once the subscriber is done, or when any thread fails or panics.
    let running = AtomicBool::new(true);
    let start = Instant::now();
    thread::scope(|scope| {
        // Rate subscriber: reassemble and count messages until all have arrived.
        scope.spawn(|| {
            let _stop = StopOnDrop(&running);
            let mut idle = YieldingIdleStrategy;
            while running.load(Ordering::Acquire)
                && messages.load(Ordering::Relaxed) < args.messages
            {
                let fragments = subscription
                    .poll_assembled(10, |data, _| {
                        messages.fetch_add(1, Ordering::Relaxed);
                        bytes.fetch_add(data.len() as u64, Ordering::Relaxed);
                    })
                    .expect("poll");
                idle.idle(fragments);
            }
        });

        // Rate reporter, once a second.
        scope.spawn(|| {
            let _stop = StopOnDrop(&running);
            let (mut last_messages, mut last_bytes, mut last_time) = (0, 0, Instant::now());
            while running.load(Ordering::Acquire) {
                thread::sleep(Duration::from_secs(1));
                let now = Instant::now();
                let (m, b) = (
                    messages.load(Ordering::Relaxed),
                    bytes.load(Ordering::Relaxed),
                );
                let seconds = (now - last_time).as_secs_f64();
                println!(
                    "{:.4e} msgs/sec, {:.4e} payload bytes/sec, totals {} messages {} MB payloads",
                    (m - last_messages) as f64 / seconds,
                    (b - last_bytes) as f64 / seconds,
                    m,
                    b / (1024 * 1024)
                );
                (last_messages, last_bytes, last_time) = (m, b, now);
            }
        });

        // Streaming publisher. Its guard only matters if it panics: once done it
        // is disarmed, and the subscriber goes on until every message arrived.
        let stop = StopOnDrop(&running);
        let message = vec![0u8; args.length];
        let mut idle = BusySpinIdleStrategy;
        let mut back_pressure = 0u64;
        'publish: for _ in 0..args.messages {
            idle.reset();
            while !sent(publication.offer(&message)) {
                if !running.load(Ordering::Acquire) {
                    break 'publish; // the subscriber failed
                }
                back_pressure += 1;
                idle.idle_now();
            }
        }
        std::mem::forget(stop);
        println!(
            "Done sending. Publisher back pressure ratio {:.6}",
            back_pressure as f64 / args.messages.max(1) as f64
        );
    });

    let elapsed = start.elapsed().as_secs_f64();
    println!(
        "Received {} messages in {elapsed:.2} s: {:.4e} msgs/sec, {:.1} MB/sec",
        messages.load(Ordering::Relaxed),
        args.messages as f64 / elapsed,
        bytes.load(Ordering::Relaxed) as f64 / elapsed / 1e6
    );
    Ok(())
}

/// Clears the shared `running` flag when dropped, so a thread that ends or
/// panics stops the others instead of leaving them spinning (and
/// `thread::scope` waiting for them forever).
struct StopOnDrop<'a>(&'a AtomicBool);

impl Drop for StopOnDrop<'_> {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
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
