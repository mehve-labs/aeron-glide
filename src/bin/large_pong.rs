use aeron_rs::{AeronClient, ControlledAction};
use std::thread;
use std::time::Duration;

const CHANNEL: &str = "aeron:ipc";
const PING_STREAM_ID: i32 = 20;
const PONG_STREAM_ID: i32 = 21;

fn main() {
    println!("Starting large_pong (controlled flow + fragment assembler)...");

    let mut client = AeronClient::new().expect("Failed to start Aeron");
    client.start();

    let mut sub = client.add_subscription(CHANNEL, PING_STREAM_ID).unwrap();
    let mut publ = client.add_publication(CHANNEL, PONG_STREAM_ID).unwrap();

    println!("large_pong waiting for messages...");

    loop {
        // poll_assembled with ControlledAction: if we can't echo back immediately,
        // return Abort so Aeron rewinds and re-delivers the message next poll.
        let _ = sub.poll_assembled(10, |data| -> ControlledAction {
            let seq = u32::from_le_bytes(data[..4].try_into().unwrap());

            if publ.offer(data) < 0 {
                // Back-pressure: can't send right now, tell Aeron to retry
                println!("  seq={}: back-pressure, aborting", seq);
                return ControlledAction::Abort;
            }

            println!("Echoed ping: seq={}, size={} bytes", seq, data.len());
            ControlledAction::Continue
        });

        thread::sleep(Duration::from_millis(1));
    }
}
