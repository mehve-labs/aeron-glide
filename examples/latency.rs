//! UDP ping-pong round-trip latency with an HDR histogram, against a separate
//! media driver (the counterpart of rusteron's `embedded_ping_pong`; see
//! `embedded_ping_pong` for the version with an embedded driver, after Aeron's
//! `EmbeddedPingPong.java`). A pong thread echoes each 32-byte ping back on its
//! own client; the main thread sends 100,000 warm-up pings, then measures
//! 1,000,000 round trips and prints the percentiles.
//!
//! Uses UDP ports 20123 and 20124 (`embedded_ping_pong` uses 20125/20126, so
//! both can run at the same time). Needs a running media driver:
//!
//! ```text
//! cargo run --features bin --bin mediadriver
//! cargo run --release --example latency
//! ```

use aeron_glide::{AeronClient, OfferError};
use hdrhistogram::Histogram;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, Instant};

const PING_STREAM_ID: i32 = 1002;
const PONG_STREAM_ID: i32 = 1003;
const WARMUP_MESSAGES: usize = 100_000;
const NUMBER_OF_MESSAGES: usize = 1_000_000;
const MESSAGE_LENGTH: usize = 32;
const FRAGMENT_COUNT_LIMIT: i32 = 10;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let channel = "aeron:udp?endpoint=localhost:20123";
    let pong_channel = "aeron:udp?endpoint=localhost:20124";

    println!("Ping-Pong Latency Test");
    println!("  message_length={}", MESSAGE_LENGTH);
    println!(
        "  warmup={} messages={}",
        WARMUP_MESSAGES, NUMBER_OF_MESSAGES
    );
    println!("  ping={} pong={}\n", channel, pong_channel);

    let running = Arc::new(AtomicBool::new(true));

    // --- Pong thread (own client) ---
    let running_pong = Arc::clone(&running);
    let pong_ch = pong_channel.to_string();
    let ping_ch = channel.to_string();
    let pong_thread = thread::spawn(move || {
        // Stops the ping side if pong fails or panics, instead of leaving it
        // waiting for an echo forever.
        let _stop = StopOnDrop(&running_pong);
        run_pong(&running_pong, &ping_ch, &pong_ch).expect("Pong failed");
    });

    // Give pong time to set up
    thread::sleep(Duration::from_millis(500));

    // --- Ping (main thread) ---
    let hist = run_ping(&running, channel, pong_channel);
    running.store(false, Ordering::SeqCst);
    pong_thread.join().expect("Pong thread panicked");
    let hist = hist?;

    // --- Results ---
    println!("\nmessage length {} bytes\n", MESSAGE_LENGTH);
    println!("Histogram of RTT latencies:");
    println!("# of samples: {}", hist.len());
    println!("min: {:?}", Duration::from_nanos(hist.min()));
    println!(
        "50th percentile: {:?}",
        Duration::from_nanos(hist.value_at_quantile(0.50))
    );
    println!(
        "99th percentile: {:?}",
        Duration::from_nanos(hist.value_at_quantile(0.99))
    );
    println!(
        "99.9th percentile: {:?}",
        Duration::from_nanos(hist.value_at_quantile(0.999))
    );
    println!(
        "99.99th percentile: {:?}",
        Duration::from_nanos(hist.value_at_quantile(0.9999))
    );
    println!("max: {:?}", Duration::from_nanos(hist.max()));
    println!("avg: {:?}", Duration::from_nanos(hist.mean() as u64));

    Ok(())
}

fn run_pong(
    running: &Arc<AtomicBool>,
    ping_channel: &str,
    pong_channel: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let client = AeronClient::new()?;

    // Pong subscribes to pong channel (receives pings) and publishes to ping channel (sends pongs)
    let mut pong_sub = client.add_subscription(pong_channel, PONG_STREAM_ID)?;
    let ping_pub = client.add_publication(ping_channel, PING_STREAM_ID)?;

    let deadline = Instant::now() + Duration::from_secs(5);
    while !ping_pub.is_connected() && Instant::now() < deadline {
        thread::sleep(Duration::from_millis(10));
    }

    println!("Pong ready");

    while running.load(Ordering::Acquire) {
        pong_sub.poll(FRAGMENT_COUNT_LIMIT, |data, _| {
            // Echo back using try_claim for zero-copy
            let result = ping_pub.try_claim(data.len()).map(|mut claim| {
                claim.buffer_mut().copy_from_slice(data);
                claim.commit()
            });
            if !sent(result) {
                // Fallback to offer if claim fails
                while !sent(ping_pub.offer(data)) && running.load(Ordering::Acquire) {}
            }
        })?;
    }
    Ok(())
}

fn run_ping(
    running: &Arc<AtomicBool>,
    ping_channel: &str,
    pong_channel: &str,
) -> Result<Histogram<u64>, Box<dyn std::error::Error>> {
    let client = AeronClient::new()?;

    // Ping publishes to pong channel (sends pings) and subscribes to ping channel (receives pongs)
    let mut pong_pub = client.add_publication(pong_channel, PONG_STREAM_ID)?;
    let mut ping_sub = client.add_subscription(ping_channel, PING_STREAM_ID)?;

    let deadline = Instant::now() + Duration::from_secs(5);
    while !pong_pub.is_connected() && Instant::now() < deadline {
        thread::sleep(Duration::from_millis(10));
    }
    if !pong_pub.is_connected() {
        return Err("Pong publisher failed to connect".into());
    }

    println!("Ping ready\n");
    thread::sleep(Duration::from_millis(500));

    let mut histogram: Histogram<u64> = Histogram::new(3)?;
    let mut buffer = [0u8; MESSAGE_LENGTH];

    // Warmup
    print!("Warming up ({} messages)... ", WARMUP_MESSAGES);
    for _ in 0..WARMUP_MESSAGES {
        if !record_rtt(
            running,
            &mut pong_pub,
            &mut ping_sub,
            &mut buffer,
            &mut histogram,
        ) {
            return Err("pong stopped".into());
        }
    }
    println!("done");
    histogram.reset();

    // Measurement
    print!("Measuring ({} messages)... ", NUMBER_OF_MESSAGES);
    let start = Instant::now();
    for i in 0..NUMBER_OF_MESSAGES {
        if !record_rtt(
            running,
            &mut pong_pub,
            &mut ping_sub,
            &mut buffer,
            &mut histogram,
        ) {
            return Err("pong stopped".into());
        }
        if i > 0 && i % 1_000_000 == 0 {
            print!("{}M ", i / 1_000_000);
        }
    }
    let elapsed = start.elapsed();
    println!(
        "done ({:.2}s, {:.0} msgs/sec)",
        elapsed.as_secs_f64(),
        NUMBER_OF_MESSAGES as f64 / elapsed.as_secs_f64()
    );

    Ok(histogram)
}

/// One round trip; `false` if pong stopped before echoing it.
#[inline]
fn record_rtt(
    running: &AtomicBool,
    publication: &mut aeron_glide::Publication,
    subscription: &mut aeron_glide::Subscription,
    buffer: &mut [u8],
    histogram: &mut Histogram<u64>,
) -> bool {
    // Write current timestamp (nanos) into the first 8 bytes
    let now = nanos();
    buffer[..8].copy_from_slice(&now.to_le_bytes());

    // Send
    while !sent(publication.offer(buffer)) {
        if !running.load(Ordering::Acquire) {
            return false;
        }
    }

    // Receive
    let mut received = false;
    while !received {
        if !running.load(Ordering::Acquire) {
            return false;
        }
        subscription
            .poll(FRAGMENT_COUNT_LIMIT, |data, _| {
                let sent_time = i64::from_le_bytes(data[..8].try_into().unwrap());
                let rtt = nanos() - sent_time;
                if rtt >= 0 {
                    let _ = histogram.record(rtt as u64);
                }
                received = true;
            })
            .expect("poll failed");
    }
    true
}

// Epoch anchored at process start — gives stable nanosecond timestamps for RTT.
fn nanos() -> i64 {
    use std::sync::OnceLock;
    static EPOCH: OnceLock<Instant> = OnceLock::new();
    let epoch = EPOCH.get_or_init(Instant::now);
    epoch.elapsed().as_nanos() as i64
}

/// Clears the shared `running` flag when dropped, so the pong thread ending
/// (by an error or a panic) stops the ping side.
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
