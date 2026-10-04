//! How fast the archive records a stream, like Aeron's
//! `EmbeddedRecordingThroughput.java`: record an IPC stream, publish a burst
//! of messages on it, wait for the recording to catch up with the publication
//! and report the rate. Each run uses a new publication (so a new recording);
//! the previous recording is truncated, waiting for the archive's recording
//! signals, as the sample does. At the end the recordings are listed from the
//! catalog.
//!
//! Needs the archive server (an `ArchivingMediaDriver` on the default Aeron
//! directory, control channel `localhost:8010`; set `AERON_DIR` and
//! `AERON_ARCHIVE_CONTROL_CHANNEL` for another one):
//!
//! ```text
//! ./scripts/start-archive.sh
//! cargo run --release --features archive --example recording_throughput
//! cargo run --release --features archive --example recording_throughput -- --messages 100000 --runs 3
//! ```

use aeron_glide::archive::{self, AeronArchive, RecordingSignal, RecordingSignalCode};
use aeron_glide::archive::{SourceLocation, recording_pos};
use aeron_glide::{AeronClient, OfferError};
use clap::Parser;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const CHANNEL: &str = "aeron:ipc";
const STREAM_ID: i32 = 1001;

#[derive(Parser)]
#[command(about = "Archive recording throughput (EmbeddedRecordingThroughput)")]
struct Args {
    /// Messages per run.
    #[arg(short, long, default_value_t = 1_000_000)]
    messages: u64,
    /// Message length in bytes (at least 8).
    #[arg(short, long, default_value_t = 32)]
    length: usize,
    /// How many runs.
    #[arg(short, long, default_value_t = 2)]
    runs: u32,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();
    let client = AeronClient::new()?;
    let control = std::env::var("AERON_ARCHIVE_CONTROL_CHANNEL")
        .unwrap_or_else(|_| "aeron:udp?endpoint=localhost:8010".to_string());
    // Collect the recording signals the archive sends while we poll it.
    let signals = Arc::new(Mutex::new(Vec::<RecordingSignal>::new()));
    let sink = signals.clone();
    let archive = archive::Context::new()
        .aeron(&client)
        .control_request_channel(&control)
        .control_response_channel("aeron:udp?endpoint=localhost:0")
        .recording_signal_consumer(move |signal| sink.lock().unwrap().push(*signal))
        .connect()?;
    let subscription_id =
        archive.start_recording(CHANNEL, STREAM_ID, SourceLocation::Local, false)?;
    let counters = client.counters_reader();

    let mut message = vec![0u8; args.length.max(8)];
    let mut recordings = Vec::new();
    for run in 1..=args.runs {
        let mut publication = client.add_exclusive_publication(CHANNEL, STREAM_ID)?;
        while !publication.is_connected() {
            std::thread::yield_now(); // until the archive subscribes
        }
        let start = Instant::now();
        for i in 0..args.messages {
            message[..8].copy_from_slice(&i.to_le_bytes());
            while !sent(publication.offer(&message)) {
                std::thread::yield_now();
            }
        }
        let stop_position = publication.position()?;

        // The recording's position counter shows how far the archive has written.
        let counter_id = loop {
            if let Some(id) =
                recording_pos::find_counter_id_by_session_id(&counters, publication.session_id())
            {
                break id;
            }
            std::thread::yield_now();
        };
        while counters.get_counter_value(counter_id)? < stop_position {
            std::thread::yield_now();
        }
        let seconds = start.elapsed().as_secs_f64();
        let recording_id = recording_pos::get_recording_id(&counters, counter_id).unwrap();
        let mb = stop_position as f64 / (1024.0 * 1024.0);
        println!(
            "run {run}: recorded {mb:.2} MB @ {:.2} MB/s - {:.0} msg/sec - {} byte payload + 32 byte header (recording {recording_id})",
            mb / seconds,
            args.messages as f64 / seconds,
            message.len()
        );

        // Closing the publication stops its recording; then free the previous one.
        drop(publication);
        await_signal(&archive, &signals, recording_id, RecordingSignalCode::Stop);
        if let Some(&previous) = recordings.last() {
            archive.truncate_recording(previous, 0)?;
            await_signal(&archive, &signals, previous, RecordingSignalCode::Delete);
            println!("        truncated recording {previous}");
        }
        recordings.push(recording_id);
    }
    archive.stop_recording(subscription_id)?;

    println!("\nRecordings of this run in the catalog:");
    archive.list_recordings(recordings[0], recordings.len() as i32, |d| {
        println!(
            "  recording {}: session {}, positions {}..{}, {} MB, channel {}",
            d.recording_id,
            d.session_id,
            d.start_position,
            d.stop_position,
            (d.stop_position - d.start_position) / (1024 * 1024),
            d.original_channel
        )
    })?;
    Ok(())
}

/// Poll the archive until it signals `code` for `recording_id`.
fn await_signal(
    archive: &AeronArchive,
    signals: &Mutex<Vec<RecordingSignal>>,
    recording_id: i64,
    code: RecordingSignalCode,
) {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        archive
            .poll_for_recording_signals()
            .expect("poll for signals");
        let mut signals = signals.lock().unwrap();
        if let Some(i) = signals
            .iter()
            .position(|s| s.recording_id == recording_id && s.signal == code)
        {
            signals.drain(..=i);
            return;
        }
        drop(signals);
        assert!(Instant::now() < deadline, "no {code:?} signal");
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
