//! Publish a numbered "Hello World!" message once a second, like Aeron's
//! `basic_publisher.c` / `BasicPublisher.java`. Each message is offered once:
//! a transient failure (no subscriber yet, back pressure) is reported and the
//! message is dropped, as in the sample; any other failure ends the program.
//!
//! Needs a media driver, and `basic_subscriber` to see the messages:
//!
//! ```text
//! cargo run --features bin --bin mediadriver
//! cargo run --example basic_subscriber
//! cargo run --example basic_publisher -- --messages 10
//! ```
//!
//! Options: `--channel`, `--stream-id`, `--messages`, `--linger` (seconds to
//! keep the publication open at the end) and `--dir` (the Aeron directory).

use aeron_glide::{AeronClient, Context, OfferError};
use clap::Parser;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::Duration;

#[derive(Parser)]
#[command(about = "Publish a message a second (Aeron's BasicPublisher)")]
struct Args {
    /// The channel to publish on.
    #[arg(short, long, default_value = "aeron:udp?endpoint=localhost:20121")]
    channel: String,
    /// The stream ID to publish on.
    #[arg(short, long, default_value_t = 1001)]
    stream_id: i32,
    /// How many messages to send.
    #[arg(short, long, default_value_t = 10)]
    messages: u64,
    /// Seconds to keep the publication open after the last message.
    #[arg(short, long, default_value_t = 0)]
    linger: u64,
    /// The Aeron directory of the media driver (its default if omitted).
    #[arg(short = 'p', long)]
    dir: Option<String>,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();
    let running = Arc::new(AtomicBool::new(true));
    let flag = running.clone();
    ctrlc::set_handler(move || flag.store(false, Ordering::Release))?;

    println!(
        "Publishing to channel {} on Stream ID {}",
        args.channel, args.stream_id
    );
    let mut context = Context::new();
    if let Some(dir) = &args.dir {
        context = context.aeron_dir(dir);
    }
    let client = AeronClient::connect(context)?;
    let publication = client.add_publication(&args.channel, args.stream_id)?;
    println!(
        "Publication added: session ID {}, max payload {} bytes",
        publication.session_id(),
        publication.max_payload_length()
    );

    for i in 0..args.messages {
        if !running.load(Ordering::Acquire) {
            break;
        }
        let message = format!("Hello World! {i}");
        print!("offering {}/{} - ", i + 1, args.messages);
        match publication.offer(message.as_bytes()) {
            Ok(position) => println!("yay! (position {position})"),
            Err(OfferError::NotConnected) => {
                println!("offer failed because publisher is not connected to a subscriber")
            }
            Err(OfferError::BackPressured) => println!("offer failed due to back pressure"),
            Err(OfferError::AdminAction) => {
                println!("offer failed because of an administration action in the system")
            }
            // Closed, max position exceeded or an Aeron error: retrying cannot help.
            Err(e) => panic!("offer failed: {e}"),
        }
        if !publication.is_connected() {
            println!("No active subscribers detected");
        }
        thread::sleep(Duration::from_secs(1));
    }

    println!("Done sending.");
    if args.linger > 0 {
        println!("Lingering for {} seconds...", args.linger);
        thread::sleep(Duration::from_secs(args.linger));
    }
    Ok(())
}
