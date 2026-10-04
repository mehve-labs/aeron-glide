//! Print every message received on a channel and stream until Ctrl-C, like
//! Aeron's `basic_subscriber.c` / `BasicSubscriber.java`: image handlers report
//! publishers joining and leaving, and the poll loop backs off with an idle
//! strategy when there is nothing to read.
//!
//! Needs a media driver, and `basic_publisher` to send messages:
//!
//! ```text
//! cargo run --features bin --bin mediadriver
//! cargo run --example basic_subscriber
//! cargo run --example basic_publisher
//! ```
//!
//! Options: `--channel`, `--stream-id` and `--dir` (the Aeron directory).

use aeron_glide::concurrent::{IdleStrategy, SleepingIdleStrategy};
use aeron_glide::{AeronClient, Context};
use clap::Parser;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

#[derive(Parser)]
#[command(about = "Print received messages (Aeron's BasicSubscriber)")]
struct Args {
    /// The channel to subscribe to.
    #[arg(short, long, default_value = "aeron:udp?endpoint=localhost:20121")]
    channel: String,
    /// The stream ID to subscribe to.
    #[arg(short, long, default_value_t = 1001)]
    stream_id: i32,
    /// The Aeron directory of the media driver (its default if omitted).
    #[arg(short = 'p', long)]
    dir: Option<String>,
}

const FRAGMENT_COUNT_LIMIT: usize = 10;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();
    let running = Arc::new(AtomicBool::new(true));
    let flag = running.clone();
    ctrlc::set_handler(move || flag.store(false, Ordering::Release))?;

    println!(
        "Subscribing to channel {} on Stream ID {}",
        args.channel, args.stream_id
    );
    let mut context = Context::new();
    if let Some(dir) = &args.dir {
        context = context.aeron_dir(dir);
    }
    let client = AeronClient::connect(context)?;

    // The handlers run on the client's conductor thread.
    let mut subscription = client.add_subscription_with_image_handlers(
        &args.channel,
        args.stream_id,
        |image| {
            println!(
                "Available image on session {} from {} (join position {})",
                image.session_id, image.source_identity, image.join_position
            )
        },
        |image| {
            println!(
                "Unavailable image on session {} at position {}",
                image.session_id, image.position
            )
        },
    )?;

    let mut idle = SleepingIdleStrategy::new(Duration::from_millis(1));
    let mut received = 0u64;
    while running.load(Ordering::Acquire) {
        let fragments = subscription.poll(FRAGMENT_COUNT_LIMIT, |data, header| {
            received += 1;
            println!(
                "Message to stream {} from session {} ({} bytes) <<{}>>",
                header.stream_id(),
                header.session_id(),
                data.len(),
                String::from_utf8_lossy(data)
            );
        })?;
        idle.idle(fragments);
    }

    println!("Shutting down after {received} messages...");
    Ok(())
}
