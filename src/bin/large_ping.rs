use aeron_rs::AeronClient;
use std::time::{Duration, Instant};
use std::thread;

const CHANNEL: &str = "aeron:ipc";
const PING_STREAM_ID: i32 = 20;
const PONG_STREAM_ID: i32 = 21;

/// Size of the message to send — larger than the default IPC MTU (~1KB term appender frame)
/// to force Aeron to fragment it.
const MESSAGE_SIZE: usize = 8192;

fn main() {
    println!("Starting large_ping (message size: {} bytes)...", MESSAGE_SIZE);

    let mut client = AeronClient::new().expect("Failed to start Aeron");
    client.start();

    let mut publ = client.add_publication(CHANNEL, PING_STREAM_ID).unwrap();
    let mut sub = client.add_subscription(CHANNEL, PONG_STREAM_ID).unwrap();

    println!("Waiting for large_pong subscriber...");
    while !publ.is_connected() {
        thread::sleep(Duration::from_millis(10));
    }

    println!("Connected. Sending large pings...");
    let start = Instant::now();

    for i in 0..5u32 {
        // Build a large message: 4-byte sequence number + pattern fill
        let mut msg = vec![0u8; MESSAGE_SIZE];
        msg[..4].copy_from_slice(&i.to_le_bytes());
        for j in 4..MESSAGE_SIZE {
            msg[j] = (j % 256) as u8;
        }

        while publ.offer(&msg) < 0 {
            thread::yield_now();
        }
        println!("Sent ping {} ({} bytes)", i, MESSAGE_SIZE);

        // Wait for the assembled response
        let mut received = false;
        while !received {
            sub.poll_assembled(10, |data| {
                let seq = u32::from_le_bytes(data[..4].try_into().unwrap());
                println!(
                    "  Received assembled pong: seq={}, size={} bytes, intact={}",
                    seq,
                    data.len(),
                    data.len() == MESSAGE_SIZE && data[4..] == msg[4..]
                );
                received = true;
            });
            thread::yield_now();
        }
    }

    println!("5 large ping-pongs completed in {:?}", start.elapsed());
}
