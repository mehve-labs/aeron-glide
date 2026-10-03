//! The pong side of a two-process ping-pong, in the spirit of Aeron's
//! `Pong.java`: echoes every message received on stream 10 back on stream 11,
//! until Ctrl-C. The channel defaults to `aeron:ipc`.
//!
//! Needs a running media driver (neither `ping` nor `pong` embeds one):
//!
//! ```text
//! cargo run --features bin --bin mediadriver
//! cargo run --example pong
//! cargo run --example ping
//! ```
//!
//! Options: `--channel` (must match `ping`'s) and `--exclusive` (an exclusive
//! publication).

use aeron_glide::{AeronClient, ExclusivePublication, OfferError, Publication};
use clap::Parser;
use std::thread;
use std::time::Duration;

const DEFAULT_CHANNEL: &str = "aeron:ipc";
const PING_STREAM_ID: i32 = 10;
const PONG_STREAM_ID: i32 = 11;

#[derive(Parser)]
#[command(name = "pong", about = "Aeron pong responder")]
struct Args {
    /// Aeron channel URI (e.g. "aeron:ipc", "aeron:udp?endpoint=localhost:20121")
    #[arg(long, default_value = DEFAULT_CHANNEL)]
    channel: String,

    /// Use ExclusivePublication instead of Publication
    #[arg(long)]
    exclusive: bool,
}

enum Pub {
    Regular(Publication),
    Exclusive(ExclusivePublication),
}

impl Pub {
    fn offer(&mut self, buf: &[u8]) -> Result<i64, OfferError> {
        match self {
            Pub::Regular(p) => p.offer(buf),
            Pub::Exclusive(p) => p.offer(buf),
        }
    }
}

fn main() {
    let args = Args::parse();

    println!("Starting Aeron Client (channel: {})...", args.channel);
    // Needs a media driver running separately (e.g. the `mediadriver` binary).
    let client = AeronClient::new().expect("Failed to start Aeron");

    let mut sub = client
        .add_subscription(&args.channel, PING_STREAM_ID)
        .unwrap();
    let mut publ = if args.exclusive {
        println!("Using ExclusivePublication");
        Pub::Exclusive(
            client
                .add_exclusive_publication(&args.channel, PONG_STREAM_ID)
                .unwrap(),
        )
    } else {
        Pub::Regular(
            client
                .add_publication(&args.channel, PONG_STREAM_ID)
                .unwrap(),
        )
    };

    println!("Pong waiting for ping messages...");

    // We run endlessly in this example, echoing anything we get
    loop {
        sub.poll_assembled(1, |data, _| {
            println!(
                "Pong received ping: {:?}",
                std::str::from_utf8(data).unwrap()
            );

            // Re-offer the exact same message bytes back to the other stream
            while !sent(publ.offer(data)) {
                // back pressure or unconnected
                thread::yield_now();
            }
        })
        .expect("poll failed");

        thread::sleep(Duration::from_millis(1));
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
