use aeron_rs::AeronClient;
use std::time::Duration;
use std::thread;

const PING_CHANNEL: &str = "aeron:ipc";
const PING_STREAM_ID: i32 = 10;
const PONG_STREAM_ID: i32 = 11;

fn main() {
    println!("Starting Aeron Client...");
    // The Media Driver should already be running from the ping process
    let mut client = AeronClient::new().expect("Failed to start Aeron");
    client.start();

    let mut sub = client.add_subscription(PING_CHANNEL, PING_STREAM_ID).unwrap();
    let mut publ = client.add_publication(PING_CHANNEL, PONG_STREAM_ID).unwrap();

    println!("Pong waiting for ping messages...");
    
    // We run endlessly in this example, echoing anything we get
    loop {
        let _ = sub.poll(1, |data| {
            println!("Pong received ping: {:?}", std::str::from_utf8(data).unwrap());
            
            // Re-offer the exact same message bytes back to the other stream
            while publ.offer(data) < 0 {
                // back pressure or unconnected
                thread::yield_now();
            }
        });
        
        thread::sleep(Duration::from_millis(1));
    }
}
