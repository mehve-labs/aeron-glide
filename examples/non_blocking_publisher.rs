//! Publishing without ever blocking the calling thread:
//!
//! - the publication and subscription are added asynchronously
//!   ([`add_publication_async`](AeronClient::add_publication_async), as
//!   `basic_publisher.c` does with `aeron_async_add_publication`): the
//!   [`PendingAdd`](aeron_glide::PendingAdd)s are polled from a loop that can
//!   do other work meanwhile;
//! - the publisher is an [`Agent`] whose duty cycle offers at most one message
//!   and never spins on back pressure: it reports no work, and the
//!   [`AgentRunner`]'s idle strategy backs off before the next attempt. It stops
//!   itself with [`Error::agent_termination`] once everything is sent.
//!
//! Self-contained (an embedded media driver):
//!
//! ```text
//! cargo run --example non_blocking_publisher
//! cargo run --example non_blocking_publisher -- --messages 100000
//! ```

use aeron_glide::concurrent::{Agent, AgentRunner, BackoffIdleStrategy};
use aeron_glide::{AeronClient, Context, Error, MediaDriver, OfferError, Publication, Result};
use clap::Parser;
use std::time::{Duration, Instant};

const CHANNEL: &str = "aeron:ipc";
const STREAM_ID: i32 = 1001;

#[derive(Parser)]
#[command(about = "Non-blocking adds and an offering agent")]
struct Args {
    /// How many messages to publish.
    #[arg(short, long, default_value_t = 10_000)]
    messages: u64,
}

/// Offers `total` messages, at most one per duty cycle.
struct PublisherAgent {
    publication: Publication,
    next: u64,
    total: u64,
    retries: u64,
}

impl Agent for PublisherAgent {
    fn do_work(&mut self) -> Result<usize> {
        if self.next == self.total {
            return Err(Error::agent_termination()); // done: stop the runner
        }
        let message = format!("message {}", self.next);
        if sent(self.publication.offer(message.as_bytes())) {
            self.next += 1;
            Ok(1)
        } else {
            // Back pressured or not connected yet: let the idle strategy back off.
            self.retries += 1;
            Ok(0)
        }
    }

    fn on_close(&mut self) -> Result<()> {
        println!(
            "publisher agent closed after {} messages ({} retried offers)",
            self.next, self.retries
        );
        Ok(())
    }
}

fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();
    let dir = std::env::temp_dir().join(format!("aeron-glide-nonblocking-{}", std::process::id()));
    let driver = MediaDriver::builder()
        .dir(&dir.to_string_lossy())
        .dir_delete_on_start(true)
        .dir_delete_on_shutdown(true)
        .start()?;
    let client = AeronClient::connect(Context::new().aeron_dir(driver.dir()))?;

    // Start both adds, then poll them from our own loop.
    let mut pending_publication = client.add_publication_async(CHANNEL, STREAM_ID)?;
    let mut pending_subscription = client.add_subscription_async(CHANNEL, STREAM_ID)?;
    let (mut publication, mut subscription) = (None, None);
    let mut other_work = 0u64;
    let deadline = Instant::now() + Duration::from_secs(10);
    while publication.is_none() || subscription.is_none() {
        assert!(Instant::now() < deadline, "the media driver did not answer");
        if publication.is_none() {
            publication = pending_publication.poll()?;
        }
        if subscription.is_none() {
            subscription = pending_subscription.poll()?;
        }
        other_work += 1; // the thread stays free for anything else
        std::thread::yield_now();
    }
    let (publication, mut subscription) = (publication.unwrap(), subscription.unwrap());
    println!(
        "publication {} and subscription {} added ({other_work} loop iterations meanwhile)",
        publication.registration_id(),
        subscription.registration_id()
    );

    let agent = PublisherAgent {
        publication,
        next: 0,
        total: args.messages,
        retries: 0,
    };
    let runner = AgentRunner::start("publisher", agent, BackoffIdleStrategy::default(), |e| {
        eprintln!("publisher: {e}")
    })?;

    let mut received = 0;
    let mut last = String::new();
    let deadline = Instant::now() + Duration::from_secs(30);
    while received < args.messages {
        assert!(
            Instant::now() < deadline,
            "timed out after {received} messages"
        );
        let fragments = subscription.poll(100, |data, _| {
            received += 1;
            last = String::from_utf8_lossy(data).into_owned();
        })?;
        if fragments == 0 {
            std::thread::yield_now();
        }
    }
    println!("received {received} messages, the last one {last:?}");

    let agent = runner.close();
    assert_eq!(agent.next, args.messages);
    Ok(())
}

/// `true` once offered, `false` to retry (back pressure, not connected yet, ...).
/// Errors that retrying cannot fix (e.g. a message too long) end the example.
fn sent(result: std::result::Result<i64, OfferError>) -> bool {
    match result {
        Ok(_) => true,
        Err(e) if e.is_retryable() => false,
        Err(e) => panic!("offer failed: {e}"),
    }
}
