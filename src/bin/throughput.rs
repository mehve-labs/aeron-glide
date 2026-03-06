use aeron_rs::AeronClient;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

const STREAM_ID: i32 = 1001;
const MESSAGE_LENGTH: usize = 32;
const BURST_LENGTH: u64 = 1_000_000;

/// IPC exclusive-publication throughput test.
/// Equivalent to rusteron's embedded_exclusive_ipc_throughput example.
///
/// Requires a running media driver: cargo run --bin mediadriver
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let channel = "aeron:ipc";

    let running = Arc::new(AtomicBool::new(true));
    let running_ctrl = Arc::clone(&running);
    ctrlc::set_handler(move || {
        running_ctrl.store(false, Ordering::SeqCst);
    })?;

    println!("IPC Exclusive Throughput Test");
    println!("  message_length={} channel={}", MESSAGE_LENGTH, channel);
    println!("  Press Ctrl-C to stop\n");

    // --- Publisher thread (own client) ---
    let running_pub = Arc::clone(&running);
    let pub_channel = channel.to_string();
    let pub_thread = thread::spawn(move || {
        let mut client = AeronClient::new().expect("Failed to create publisher client");
        client.start();
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
            while publication.offer(&buffer) < 0 {
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

    // --- Subscriber (main thread, own client) ---
    let mut client = AeronClient::new()?;
    client.start();
    let mut subscription = client.add_subscription(channel, STREAM_ID)?;

    let mut message_count: u64 = 0;
    let mut start = Instant::now();
    let mut next_check = BURST_LENGTH;

    while running.load(Ordering::Acquire) {
        subscription.poll(MESSAGE_LENGTH as i32, |_data| {
            message_count += 1;
        });

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
