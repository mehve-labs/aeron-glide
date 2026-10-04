//! IPC throughput of an exclusive publication with an embedded media driver,
//! like Aeron's `EmbeddedExclusiveIpcThroughput.java` (and, with `--claim`,
//! `EmbeddedExclusiveBufferClaimIpcThroughput.java`): a publisher thread
//! sends small messages flat out, a subscriber thread counts them, and the
//! main thread reports the rate once a second.
//!
//! Self-contained (no separate media driver). Runs for `--seconds` (default
//! 5) or until Ctrl-C:
//!
//! ```text
//! cargo run --release --example embedded_exclusive_ipc_throughput
//! cargo run --release --example embedded_exclusive_ipc_throughput -- --claim --seconds 3
//! ```

use aeron_glide::concurrent::{BusySpinIdleStrategy, IdleStrategy};
use aeron_glide::{AeronClient, Context, MediaDriver, OfferError, ThreadingMode};
use clap::Parser;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::thread;
use std::time::{Duration, Instant};

const CHANNEL: &str = "aeron:ipc";
const STREAM_ID: i32 = 1001;
const FRAGMENT_COUNT_LIMIT: usize = 256;

#[derive(Parser)]
#[command(about = "Exclusive publication IPC throughput (EmbeddedExclusiveIpcThroughput)")]
struct Args {
    /// Message length in bytes.
    #[arg(short, long, default_value_t = 32)]
    length: usize,
    /// How long to run, in seconds.
    #[arg(short, long, default_value_t = 5)]
    seconds: u64,
    /// Publish with zero-copy `try_claim` instead of `offer`.
    #[arg(short, long)]
    claim: bool,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();
    let running = Arc::new(AtomicBool::new(true));
    let flag = running.clone();
    ctrlc::set_handler(move || flag.store(false, Ordering::Release))?;

    let dir = std::env::temp_dir().join(format!("aeron-glide-ipc-tput-{}", std::process::id()));
    let driver = MediaDriver::builder()
        .dir(&dir.to_string_lossy())
        .dir_delete_on_start(true)
        .dir_delete_on_shutdown(true)
        .threading_mode(ThreadingMode::Shared)
        .start()?;
    let client = AeronClient::connect(Context::new().aeron_dir(driver.dir()))?;
    let mut subscription = client.add_subscription(CHANNEL, STREAM_ID)?;
    let mut publication = client.add_exclusive_publication(CHANNEL, STREAM_ID)?;
    println!(
        "Streaming {}-byte messages on {CHANNEL} with {} for {} s",
        args.length,
        if args.claim { "try_claim" } else { "offer" },
        args.seconds
    );

    let messages = AtomicU64::new(0);
    thread::scope(|scope| {
        // Subscriber: count every message.
        // Each thread clears `running` when it ends, even by panicking, to stop the others.
        scope.spawn(|| {
            let _stop = StopOnDrop(&running);
            let mut idle = BusySpinIdleStrategy;
            while running.load(Ordering::Acquire) {
                let fragments = subscription
                    .poll(FRAGMENT_COUNT_LIMIT, |_, _| {
                        messages.fetch_add(1, Ordering::Relaxed);
                    })
                    .expect("poll");
                idle.idle(fragments);
            }
        });

        // Publisher: send flat out, counting the offers that had to be retried.
        scope.spawn(|| {
            let _stop = StopOnDrop(&running);
            let message = vec![0u8; args.length];
            let (mut sent_count, mut back_pressure) = (0u64, 0u64);
            'publish: while running.load(Ordering::Acquire) {
                loop {
                    let result = if args.claim {
                        publication.try_claim(args.length).map(|mut claim| {
                            claim.buffer_mut().copy_from_slice(&message);
                            claim.commit()
                        })
                    } else {
                        publication.offer(&message)
                    };
                    if sent(result) {
                        break;
                    }
                    back_pressure += 1;
                    if !running.load(Ordering::Acquire) {
                        break 'publish;
                    }
                }
                sent_count += 1;
            }
            println!(
                "Publisher back pressure ratio: {:.6}",
                back_pressure as f64 / sent_count.max(1) as f64
            );
        });

        // Rate reporter.
        let _stop = StopOnDrop(&running);
        let deadline = Instant::now() + Duration::from_secs(args.seconds);
        let (mut last, mut last_time) = (0, Instant::now());
        while running.load(Ordering::Acquire) && Instant::now() < deadline {
            thread::sleep(Duration::from_secs(1));
            let (now, total) = (Instant::now(), messages.load(Ordering::Relaxed));
            let rate = (total - last) as f64 / (now - last_time).as_secs_f64();
            println!(
                "{:.3e} msgs/sec, {:.3e} bytes/sec, totals {} messages {} MB",
                rate,
                rate * args.length as f64,
                total,
                total * args.length as u64 / (1024 * 1024)
            );
            (last, last_time) = (total, now);
        }
    });
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
