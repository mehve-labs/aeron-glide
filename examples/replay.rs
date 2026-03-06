use aeron_glide::archive::AeronArchive;
use aeron_glide::AeronClient;
use std::thread;
use std::time::Duration;

const REPLAY_CHANNEL: &str = "aeron:ipc";
const REPLAY_STREAM_ID: i32 = 1002;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("Connecting to Aeron Archive...");
    let mut archive = AeronArchive::connect(
        "aeron:udp?endpoint=localhost:8010",
        10,
        "aeron:udp?endpoint=localhost:0",
        20,
    )?;
    println!(
        "Connected (archive_id={}, session={})",
        archive.archive_id(),
        archive.control_session_id()
    );

    // List recordings and pick the last one
    println!("\n--- Available Recordings ---");
    let mut target_recording_id: Option<i64> = None;
    let mut target_start_pos: i64 = 0;
    let mut target_stop_pos: i64 = 0;

    archive.list_recordings(0, 100, |desc| {
        println!(
            "  Recording {}: stream={} channel={} start_pos={} stop_pos={}",
            desc.recording_id,
            desc.stream_id,
            desc.stripped_channel,
            desc.start_position,
            desc.stop_position,
        );
        target_recording_id = Some(desc.recording_id);
        target_start_pos = desc.start_position;
        target_stop_pos = desc.stop_position;
    })?;

    let recording_id = target_recording_id.ok_or("No recordings found in the archive")?;
    let length = if target_stop_pos > target_start_pos {
        target_stop_pos - target_start_pos
    } else {
        i64::MAX // still active, replay everything available
    };

    println!(
        "\nReplaying recording {} (pos {}..{})...",
        recording_id, target_start_pos, target_stop_pos
    );

    // Create Aeron client and subscribe to the replay channel
    let mut client = AeronClient::new()?;
    client.start();

    let mut sub = client.add_subscription(REPLAY_CHANNEL, REPLAY_STREAM_ID)?;

    // Start replay from the beginning of the recording
    let replay_session = archive.start_replay(
        recording_id,
        REPLAY_CHANNEL,
        REPLAY_STREAM_ID,
        target_start_pos,
        length,
    )?;
    println!("Replay started (session={})", replay_session);

    // Wait for subscription to connect
    println!("Waiting for replay subscription to connect...");
    while !sub.is_connected() {
        thread::sleep(Duration::from_millis(10));
    }

    // Poll for replayed messages
    println!("\n--- Replayed Messages ---");
    let mut total_received = 0;
    let mut idle_count = 0;

    while idle_count < 100 {
        let fragments = sub.poll_assembled(10, |data| {
            let msg = std::str::from_utf8(data).unwrap_or("<binary>");
            println!("  [{}] {}", total_received, msg);
            total_received += 1;
        });
        if fragments == 0 {
            idle_count += 1;
            thread::sleep(Duration::from_millis(10));
        } else {
            idle_count = 0;
        }
    }

    println!(
        "\nReplay complete. Received {} messages total.",
        total_received
    );

    // Cleanup
    archive.stop_replay(replay_session)?;
    Ok(())
}
