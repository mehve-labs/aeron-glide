//! A persistent subscription, like Aeron's `PersistentSubscriber.java`: replay
//! a recorded stream from the start, join the live stream once caught up, and
//! ride through losing the live stream without missing a message.
//!
//! The publisher sends to two destinations of a manual MDC channel: one the
//! archive records, one the persistent subscription receives live. The example:
//!
//! 1. publishes some history, then starts the persistent subscription, which
//!    replays the recording and joins live (`on_live_joined`);
//! 2. removes the subscriber's destination, as if its network link failed: once
//!    the live image times out (the driver's image liveness timeout, 10 s by
//!    default) it leaves live (`on_live_left`) and falls back to replaying what
//!    the archive kept recording;
//! 3. restores the destination: it catches up and rejoins live.
//!
//! Every message carries a sequence number; the example checks none is missed
//! or duplicated. Takes about 15 s. Needs the archive server (an
//! `ArchivingMediaDriver` on the default Aeron directory, control channel
//! `localhost:8010`; set `AERON_DIR` and `AERON_ARCHIVE_CONTROL_CHANNEL` for
//! another one):
//!
//! ```text
//! ./scripts/start-archive.sh
//! cargo run --features archive --example persistent_subscription
//! ```

use aeron_glide::archive::{self, PersistentSubscription, PersistentSubscriptionBuilder};
use aeron_glide::archive::{SourceLocation, recording_pos};
use aeron_glide::{AeronClient, OfferError, Publication};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::thread;
use std::time::{Duration, Instant};

const STREAM_ID: i32 = 1001;
const REPLAY_STREAM_ID: i32 = 5000;
const RECORDED: &str = "aeron:udp?endpoint=localhost:20171";
const LIVE: &str = "aeron:udp?endpoint=localhost:20172";

/// The archive of `scripts/start-archive.sh`, or `AERON_ARCHIVE_CONTROL_CHANNEL`'s.
fn archive_context(client: &AeronClient) -> archive::Context {
    let control = std::env::var("AERON_ARCHIVE_CONTROL_CHANNEL")
        .unwrap_or_else(|_| "aeron:udp?endpoint=localhost:8010".to_string());
    archive::Context::new()
        .aeron(client)
        .control_request_channel(&control)
        .control_response_channel("aeron:udp?endpoint=localhost:0")
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let client = AeronClient::new()?;
    let archive = archive_context(&client).connect()?;

    // The archive records the RECORDED destination of the publication.
    let recording = archive.start_recording(RECORDED, STREAM_ID, SourceLocation::Remote, false)?;
    let publication = client.add_publication("aeron:udp?control-mode=manual", STREAM_ID)?;
    add_destination(&publication, RECORDED)?;
    let mut next = 0u64;
    for _ in 0..20 {
        publish(&publication, &mut next);
    }
    // The recording's position counter is keyed by the recorded session.
    let counters = client.counters_reader();
    let mut recording_id = None;
    until(|| {
        recording_id =
            recording_pos::find_counter_id_by_session_id(&counters, publication.session_id())
                .and_then(|counter_id| recording_pos::get_recording_id(&counters, counter_id));
        recording_id.is_some()
    });
    let recording_id = recording_id.unwrap();
    println!("Recording {recording_id} holds {next} messages of history");

    let (joined, left) = (Arc::new(AtomicUsize::new(0)), Arc::new(AtomicUsize::new(0)));
    let (on_joined, on_left) = (joined.clone(), left.clone());
    let mut subscription = PersistentSubscriptionBuilder::new()
        .archive_context(archive_context(&client))
        .aeron(&client)
        .recording_id(recording_id)
        .start_position(PersistentSubscription::FROM_START)
        .live_channel(LIVE)
        .live_stream_id(STREAM_ID)
        .replay_channel("aeron:udp?endpoint=localhost:0")
        .replay_stream_id(REPLAY_STREAM_ID)
        .on_live_joined(move || {
            on_joined.fetch_add(1, Ordering::SeqCst);
            println!("=== joined the live stream");
        })
        .on_live_left(move || {
            on_left.fetch_add(1, Ordering::SeqCst);
            println!("=== left the live stream");
        })
        .on_error(|e| eprintln!("persistent subscription error: {e}"))
        .create()?;
    let live_destination = add_destination(&publication, LIVE)?;

    // Poll, publishing a message every 10 ms, until `done`.
    let mut expected = 0u64;
    let mut run = |what: &str, done: &dyn Fn() -> bool| -> aeron_glide::Result<()> {
        println!("--- {what}");
        let deadline = Instant::now() + Duration::from_secs(60);
        let mut last_publish = Instant::now();
        let (mut replayed, mut live) = (0, 0);
        while !done() {
            assert!(Instant::now() < deadline, "timed out: {what}");
            assert!(
                !subscription.has_failed(),
                "the persistent subscription failed"
            );
            subscription.poll(10, |data, header| {
                let sequence = u64::from_le_bytes(data.try_into().unwrap());
                assert_eq!(sequence, expected, "a message was missed or duplicated");
                expected += 1;
                // Replayed fragments come from the replay stream.
                if header.stream_id() == STREAM_ID {
                    live += 1;
                } else {
                    replayed += 1;
                }
            })?;
            if last_publish.elapsed() >= Duration::from_millis(10) {
                publish(&publication, &mut next);
                last_publish = Instant::now();
            }
            thread::sleep(Duration::from_millis(1));
        }
        println!("    {replayed} messages replayed, {live} live, up to message {expected}");
        Ok(())
    };

    run("replaying the history, then joining live", &|| {
        joined.load(Ordering::SeqCst) == 1
    })?;
    let removal = publication.remove_destination_by_id(live_destination)?;
    until(|| publication.find_destination_response(removal).unwrap());
    run(
        "live destination removed: waiting for the image to time out",
        &|| left.load(Ordering::SeqCst) == 1,
    )?;
    add_destination(&publication, LIVE)?;
    run(
        "live destination restored: catching up and rejoining live",
        &|| joined.load(Ordering::SeqCst) == 2,
    )?;
    println!(
        "Received all {expected} messages in order: joined live {} times, left {} time",
        joined.load(Ordering::SeqCst),
        left.load(Ordering::SeqCst)
    );

    drop(subscription);
    archive.stop_recording(recording)?;
    Ok(())
}

/// Add a destination to the MDC publication and wait until it is in use.
fn add_destination(publication: &Publication, endpoint: &str) -> aeron_glide::Result<i64> {
    let id = publication.add_destination(endpoint)?;
    until(|| publication.find_destination_response(id).unwrap());
    Ok(id)
}

/// Publish the next sequence number.
fn publish(publication: &Publication, next: &mut u64) {
    while !sent(publication.offer(&next.to_le_bytes())) {
        thread::yield_now();
    }
    *next += 1;
}

fn until(mut done: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while !done() {
        assert!(Instant::now() < deadline, "timed out");
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
