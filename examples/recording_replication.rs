//! Replicate a recording from one archive to another, like Aeron's
//! `RecordingReplicator.java`: the destination archive connects to the source
//! archive's control channel and copies the recording; its recording signals
//! (`Replicate`, `Sync`, `ReplicateEnd`, ...) report the progress.
//!
//! The source is the archive on this client's media driver: the example
//! records a few messages there first. The destination is `--dst-control`,
//! by default the same archive, which then holds a second copy of the
//! recording (as `tests/archive.rs` does): a real deployment would point it at
//! a backup archive (a second `ArchivingMediaDriver` with its own control port
//! and `aeron.archive.id`).
//!
//! Needs the archive server (an `ArchivingMediaDriver` on the default Aeron
//! directory, control channel `localhost:8010`; set `AERON_DIR` for another
//! media driver):
//!
//! ```text
//! ./scripts/start-archive.sh
//! cargo run --features archive --example recording_replication
//! cargo run --features archive --example recording_replication -- --dst-control aeron:udp?endpoint=localhost:8011
//! ```

use aeron_glide::archive::{self, AeronArchive, RecordingSignal, RecordingSignalCode};
use aeron_glide::archive::{ReplicationParams, recording_pos};
use aeron_glide::{AeronClient, OfferError};
use clap::Parser;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const STREAM_ID: i32 = 1001;

#[derive(Parser)]
#[command(about = "Replicate a recording between archives (RecordingReplicator)")]
struct Args {
    /// The source archive's control channel.
    #[arg(long, default_value = "aeron:udp?endpoint=localhost:8010")]
    src_control: String,
    /// The source archive's control stream ID.
    #[arg(long, default_value_t = 10)]
    src_stream_id: i32,
    /// The destination archive's control channel (the source if omitted).
    #[arg(long)]
    dst_control: Option<String>,
    /// How many messages to record and replicate.
    #[arg(short, long, default_value_t = 1000)]
    messages: u64,
}

fn connect(client: &AeronClient, control: &str) -> aeron_glide::Result<AeronArchive> {
    archive::Context::new()
        .aeron(client)
        .control_request_channel(control)
        .control_response_channel("aeron:udp?endpoint=localhost:0")
        .connect()
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();
    let client = AeronClient::new()?;

    // Record some messages on the source archive.
    let src = connect(&client, &args.src_control)?;
    let publication = src.add_recorded_publication("aeron:ipc", STREAM_ID)?;
    for i in 0..args.messages {
        let message = format!("message {i}");
        while !sent(publication.offer(message.as_bytes())) {
            std::thread::yield_now();
        }
    }
    let counters = client.counters_reader();
    let counter_id = wait_for(|| {
        recording_pos::find_counter_id_by_session_id(&counters, publication.session_id())
    });
    let src_recording_id = recording_pos::get_recording_id(&counters, counter_id).unwrap();
    let stop_position = publication.position()?;
    wait_for(|| {
        (src.get_recording_position(src_recording_id).ok()? >= stop_position).then_some(())
    });
    src.stop_recording_publication(&publication)?;
    println!(
        "Source archive {}: recording {src_recording_id} stopped at position {stop_position}",
        src.archive_id()
    );

    // The destination archive's session collects its recording signals.
    let signals = Arc::new(Mutex::new(Vec::<RecordingSignal>::new()));
    let sink = signals.clone();
    let dst_control = args.dst_control.as_deref().unwrap_or(&args.src_control);
    let dst = archive::Context::new()
        .aeron(&client)
        .control_request_channel(dst_control)
        .control_response_channel("aeron:udp?endpoint=localhost:0")
        .recording_signal_consumer(move |signal| sink.lock().unwrap().push(*signal))
        .connect()?;
    println!("Destination archive {} at {dst_control}", dst.archive_id());

    let replication_id = dst.replicate(
        src_recording_id,
        args.src_stream_id,
        &args.src_control,
        &ReplicationParams::new(), // a new recording, up to the source's stop position
    )?;
    println!("Replication {replication_id} started");
    let dst_recording_id = wait_for(|| {
        dst.poll_for_recording_signals().expect("poll for signals");
        let mut signals = signals.lock().unwrap();
        let mut done = None;
        for signal in signals.drain(..) {
            println!(
                "  signal {:?}: recording {} at position {}",
                signal.signal, signal.recording_id, signal.position
            );
            if signal.signal == RecordingSignalCode::ReplicateEnd {
                done = Some(signal.recording_id);
            }
        }
        done
    });

    let mut copy = None;
    dst.list_recording(dst_recording_id, |d| copy = Some(d))?;
    let copy = copy.ok_or("the replicated recording is not in the catalog")?;
    println!(
        "Replicated to recording {dst_recording_id}: positions {}..{}, stream {}, source {}",
        copy.start_position, copy.stop_position, copy.stream_id, copy.source_identity
    );
    assert_eq!(copy.stop_position, stop_position, "incomplete copy");
    Ok(())
}

/// Poll `f` until it returns a value (10 s at most).
fn wait_for<T>(mut f: impl FnMut() -> Option<T>) -> T {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if let Some(value) = f() {
            return value;
        }
        assert!(Instant::now() < deadline, "timed out");
        std::thread::sleep(Duration::from_millis(1));
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
