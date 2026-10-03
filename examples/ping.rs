use aeron_glide::{AeronClient, ExclusivePublication, OfferError, Publication};
use clap::Parser;
use std::thread;
use std::time::{Duration, Instant};

const DEFAULT_CHANNEL: &str = "aeron:ipc";
const PING_STREAM_ID: i32 = 10;
const PONG_STREAM_ID: i32 = 11;

#[derive(Parser)]
#[command(name = "ping", about = "Aeron ping client")]
struct Args {
    /// Aeron channel URI (e.g. "aeron:ipc", "aeron:udp?endpoint=localhost:20121")
    #[arg(long, default_value = DEFAULT_CHANNEL)]
    channel: String,

    /// Use ExclusivePublication instead of Publication
    #[arg(long)]
    exclusive: bool,

    /// Use zero-copy tryClaim instead of offer
    #[arg(long)]
    zero_copy: bool,
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
    /// Zero-copy publish: claim space in the log buffer and write into it.
    fn send_claimed(&mut self, message: &[u8]) -> Result<i64, OfferError> {
        let mut claim = match self {
            Pub::Regular(p) => p.try_claim(message.len())?,
            Pub::Exclusive(p) => p.try_claim(message.len())?,
        };
        claim.buffer_mut().copy_from_slice(message);
        Ok(claim.commit())
    }
    fn is_connected(&self) -> bool {
        match self {
            Pub::Regular(p) => p.is_connected(),
            Pub::Exclusive(p) => p.is_connected(),
        }
    }
}

fn main() {
    let args = Args::parse();

    println!("Starting Aeron Client (channel: {})...", args.channel);
    let client = AeronClient::new().expect("Failed to start Aeron");
    client.start();

    let mut publ = if args.exclusive {
        println!("Using ExclusivePublication");
        Pub::Exclusive(
            client
                .add_exclusive_publication(&args.channel, PING_STREAM_ID)
                .unwrap(),
        )
    } else {
        Pub::Regular(
            client
                .add_publication(&args.channel, PING_STREAM_ID)
                .unwrap(),
        )
    };
    let mut sub = client
        .add_subscription(&args.channel, PONG_STREAM_ID)
        .unwrap();

    println!("Waiting for pong subscriber...");
    while !publ.is_connected() {
        thread::sleep(Duration::from_millis(10));
    }

    if args.zero_copy {
        println!("Using zero-copy tryClaim");
    }

    println!("Connected. Sending pings...");
    let start = Instant::now();

    for i in 0..10u32 {
        if args.zero_copy {
            while publ.send_claimed(b"ping!").is_err() {
                thread::yield_now();
            }
        } else {
            let msg = b"ping!";
            while publ.offer(msg).is_err() {
                thread::yield_now();
            }
        }

        let mut received = false;
        while !received {
            sub.poll_assembled(1, |data| {
                println!(
                    "Ping received response: {:?}",
                    std::str::from_utf8(data).unwrap()
                );
                received = true;
            })
            .expect("poll failed");
            thread::yield_now();
        }

        println!("Completed roundtrip {}", i);
    }

    println!("10 ping-pongs completed in {:?}", start.elapsed());

    // Print Aeron counters after the benchmark
    let reader = client.counters_reader();
    println!("\n--- AERON COUNTERS ---");
    reader
        .for_each(|id, _type_id, _key_buffer, label| {
            let value = reader.get_counter_value(id).unwrap_or(0);
            if value != 0 {
                println!("  {:>3}: {} = {}", id, label, value);
            }
        })
        .expect("failed to read counters");
    println!("----------------------");
}
