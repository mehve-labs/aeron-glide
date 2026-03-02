use aeron_rs::AeronClient;
use std::time::{Duration, Instant};
use std::thread;

const PING_CHANNEL: &str = "aeron:ipc";
const PING_STREAM_ID: i32 = 10;
const PONG_STREAM_ID: i32 = 11;

fn main() {
    println!("Starting Aeron Client...");
    let mut client = AeronClient::new().expect("Failed to start Aeron");
    client.start();

    let mut publ = client.add_publication(PING_CHANNEL, PING_STREAM_ID).unwrap();
    let mut sub = client.add_subscription(PING_CHANNEL, PONG_STREAM_ID).unwrap();

    println!("Waiting for pong subscriber...");
    while !publ.is_connected() {
        thread::sleep(Duration::from_millis(10));
    }

    println!("Connected. Sending pings...");
    let msg = b"ping!";
    let start = Instant::now();

    for i in 0..10 {
        // Offer a message
        while publ.offer(msg) < 0 {
            thread::yield_now();
        }

        // Wait for pong
        let mut received = false;
        while !received {
            sub.poll(1, |data| {
                println!("Ping received response: {:?}", std::str::from_utf8(data).unwrap());
                received = true;
            });
            thread::yield_now();
        }
        
        println!("Completed roundtrip {}", i);
    }
    
    println!("10 ping-pongs completed in {:?}", start.elapsed());
}
