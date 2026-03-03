use aeron_rs::{AeronClient, ExclusivePublication, Publication};
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
    fn offer(&mut self, buf: &[u8]) -> i64 {
        match self {
            Pub::Regular(p) => p.offer(buf),
            Pub::Exclusive(p) => p.offer(buf),
        }
    }
}

fn main() {
    let args = Args::parse();

    println!("Starting Aeron Client (channel: {})...", args.channel);
    // The Media Driver should already be running from the ping process
    let mut client = AeronClient::new().expect("Failed to start Aeron");
    client.start();

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
        let _ = sub.poll_assembled(1, |data| {
            println!(
                "Pong received ping: {:?}",
                std::str::from_utf8(data).unwrap()
            );

            // Re-offer the exact same message bytes back to the other stream
            while publ.offer(data) < 0 {
                // back pressure or unconnected
                thread::yield_now();
            }
        });

        thread::sleep(Duration::from_millis(1));
    }
}
