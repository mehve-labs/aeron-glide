//! IPC throughput of an exclusive publication against a separate media driver
//! (the counterpart of rusteron's `embedded_exclusive_ipc_throughput`; see
//! `embedded_exclusive_ipc_throughput` for the version with an embedded
//! driver, after Aeron's `EmbeddedExclusiveIpcThroughput.java`). A publisher
//! thread sends 32-byte messages flat out on `aeron:ipc`, the main thread
//! counts them and prints the rate about once a second, until Ctrl-C.
//!
//! Each side has its own client, unless `--shared-client` is given: then one
//! client serves both, as in rusteron's example.
//!
//! Needs a running media driver:
//!
//! ```text
//! cargo run --features bin --bin mediadriver
//! cargo run --release --example throughput
//! cargo run --release --example throughput -- --shared-client
//! ```

use aeron_glide::{AeronClient, OfferError};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, Instant};

const STREAM_ID: i32 = 1001;
const MESSAGE_LENGTH: usize = 32;
const BURST_LENGTH: u64 = 1_000_000;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let channel = "aeron:ipc";

    let running = Arc::new(AtomicBool::new(true));
    let running_ctrl = Arc::clone(&running);
    ctrlc::set_handler(move || {
        running_ctrl.store(false, Ordering::SeqCst);
    })?;

    let shared_client = std::env::args().any(|arg| arg == "--shared-client");
    let shared = if shared_client {
        Some(Arc::new(AeronClient::new()?))
    } else {
        None
    };

    println!("IPC Exclusive Throughput Test");
    println!(
        "  message_length={} channel={} clients={}",
        MESSAGE_LENGTH,
        channel,
        if shared_client {
            "shared"
        } else {
            "one per side"
        }
    );
    println!("  Press Ctrl-C to stop\n");

    // --- Publisher thread (its own client, or the shared one) ---
    let running_pub = Arc::clone(&running);
    let pub_channel = channel.to_string();
    let pub_client = shared.clone();
    let pub_thread = thread::spawn(move || {
        // Stops the subscriber loop if the publisher ends early or panics.
        let _stop = StopOnDrop(&running_pub);
        let client = match pub_client {
            Some(client) => client,
            None => Arc::new(AeronClient::new().expect("Failed to create publisher client")),
        };
        let mut publication = client
            .add_exclusive_publication(&pub_channel, STREAM_ID)
            .expect("Failed to add publication");

        // Wait for connection
        let deadline = Instant::now() + Duration::from_secs(5);
        while !publication.is_connected() && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(10));
        }
        if !publication.is_connected() {
            eprintln!("Publication failed to connect");
            return;
        }

        let buffer = [0u8; MESSAGE_LENGTH];
        let mut back_pressure_count: u64 = 0;
        let mut total_messages: u64 = 0;

        while running_pub.load(Ordering::Acquire) {
            while !sent(publication.offer(&buffer)) {
                back_pressure_count += 1;
                if !running_pub.load(Ordering::Acquire) {
                    break;
                }
            }
            total_messages += 1;
        }

        if total_messages > 0 {
            let ratio = back_pressure_count as f64 / total_messages as f64;
            println!("Publisher back pressure ratio: {:.6}", ratio);
        }
    });

    // --- Subscriber (main thread, its own client or the shared one) ---
    let client = match shared {
        Some(client) => client,
        None => Arc::new(AeronClient::new()?),
    };
    let mut subscription = client.add_subscription(channel, STREAM_ID)?;

    let mut message_count: u64 = 0;
    let mut start = Instant::now();
    let mut next_check = BURST_LENGTH;

    while running.load(Ordering::Acquire) {
        subscription.poll(MESSAGE_LENGTH, |_data, _| {
            message_count += 1;
        })?;

        if message_count >= next_check && start.elapsed() >= Duration::from_secs(1) {
            let elapsed = start.elapsed().as_secs_f64();
            let rate = message_count as f64 / elapsed;
            let throughput = rate * MESSAGE_LENGTH as f64;
            println!(
                "Throughput: {:.0} msgs/sec, {:.0} bytes/sec",
                rate, throughput,
            );
            message_count = 0;
            start = Instant::now();
            next_check = BURST_LENGTH;
        }
    }

    pub_thread.join().expect("Publisher thread panicked");
    Ok(())
}

/// Clears the shared `running` flag when dropped, so the publisher thread
/// ending (by an error or a panic) stops the subscriber loop.
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
