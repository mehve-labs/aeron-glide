use aeron_rs::AeronClient;
use std::time::Duration;
use std::thread;

const CHANNEL: &str = "aeron:ipc";
const PING_STREAM_ID: i32 = 20;
const PONG_STREAM_ID: i32 = 21;

fn main() {
    println!("Starting large_pong (fragment assembler)...");

    let mut client = AeronClient::new().expect("Failed to start Aeron");
    client.start();

    let mut sub = client.add_subscription(CHANNEL, PING_STREAM_ID).unwrap();
    let mut publ = client.add_publication(CHANNEL, PONG_STREAM_ID).unwrap();

    println!("large_pong waiting for messages...");

    loop {
        let _ = sub.poll_assembled(10, |data| {
            let seq = u32::from_le_bytes(data[..4].try_into().unwrap());
            println!("Received assembled ping: seq={}, size={} bytes", seq, data.len());

            // Echo the full reassembled message back
            while publ.offer(data) < 0 {
                thread::yield_now();
            }
        });

        thread::sleep(Duration::from_millis(1));
    }
}
