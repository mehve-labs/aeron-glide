use aeron_glide::archive::{self, SourceLocation};
use aeron_glide::{AeronClient, OfferError};
use std::thread;
use std::time::Duration;

const RECORDING_CHANNEL: &str = "aeron:ipc";
const RECORDING_STREAM_ID: i32 = 1001;
const MESSAGE_COUNT: usize = 10;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("Connecting to Aeron Archive...");
    let archive = archive::Context::new()
        .control_request_channel("aeron:udp?endpoint=localhost:8010")
        .control_request_stream_id(10)
        .control_response_channel("aeron:udp?endpoint=localhost:0")
        .control_response_stream_id(20)
        .connect()?;
    println!(
        "Connected (archive_id={}, session={})",
        archive.archive_id(),
        archive.control_session_id()
    );

    // Start recording on the channel
    println!(
        "Starting recording on {} stream {}...",
        RECORDING_CHANNEL, RECORDING_STREAM_ID
    );
    let sub_id = archive.start_recording(
        RECORDING_CHANNEL,
        RECORDING_STREAM_ID,
        SourceLocation::Local,
        false,
    )?;
    println!("Recording started (subscription_id={})", sub_id);

    // Connect an Aeron client and publish messages
    let client = AeronClient::new()?;

    let publ = client.add_publication(RECORDING_CHANNEL, RECORDING_STREAM_ID)?;

    println!("Waiting for publication to connect...");
    while !publ.is_connected() {
        thread::sleep(Duration::from_millis(10));
    }

    println!("Publishing {} messages...", MESSAGE_COUNT);
    for i in 0..MESSAGE_COUNT {
        let msg = format!("Hello Archive! Message #{}", i);
        while !sent(publ.offer(msg.as_bytes())) {
            thread::yield_now();
        }
        println!("  Sent: {}", msg);
        thread::sleep(Duration::from_millis(100));
    }

    // Give the archive time to flush
    thread::sleep(Duration::from_secs(1));

    // Stop recording
    println!("\nStopping recording...");
    archive.stop_recording(sub_id)?;
    println!("Recording stopped.");

    // List all recordings
    println!("\n--- Recordings ---");
    archive.list_recordings(0, 100, |desc| {
        println!(
            "  Recording {}: stream={} channel={} start_pos={} stop_pos={}",
            desc.recording_id,
            desc.stream_id,
            desc.stripped_channel,
            desc.start_position,
            desc.stop_position,
        );
    })?;

    println!("\nDone. Run the replay binary to replay these messages.");
    Ok(())
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
