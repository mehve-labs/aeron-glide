use aeron_rs::archive::{AeronArchive, ReplayMerge, SourceLocation};
use aeron_rs::AeronClient;
use std::thread;
use std::time::{Duration, Instant};

const STREAM_ID: i32 = 1001;

// MDC control endpoint — publication listens here for subscriber registrations.
const CONTROL_ENDPOINT: &str = "localhost:24325";
// Live data endpoint — where the MDS subscription receives live data from the publication.
const LIVE_ENDPOINT: &str = "localhost:24327";
// Replay endpoint — where the subscription receives replayed data from the archive.
// Using :0 lets the driver assign a free port (resolved by ReplayMerge).
const REPLAY_ENDPOINT: &str = "localhost:0";

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== Aeron ReplayMerge Demo ===");
    println!("  Requires: ArchivingMediaDriver (NOT a separate mediadriver)");
    println!();

    // --- Setup ---
    println!("Connecting to Aeron Archive...");
    let mut archive = AeronArchive::connect(
        "aeron:udp?endpoint=localhost:8010",
        10,
        "aeron:udp?endpoint=localhost:0",
        20,
    )?;
    println!(
        "Archive connected (id={}, session={})\n",
        archive.archive_id(),
        archive.control_session_id()
    );

    let mut client = AeronClient::new()?;
    client.start();

    // --- Phase 1: Create publication, then record ---
    println!("--- Phase 1: Recording Messages ---\n");

    // Publication: MDC with dynamic control mode.
    let pub_channel = format!(
        "aeron:udp?control={}|control-mode=dynamic|linger=0",
        CONTROL_ENDPOINT
    );

    let mut pub1 = client.add_publication(&pub_channel, STREAM_ID)?;

    // Session ID is available immediately (assigned by media driver on creation).
    let pub_session_id = pub1.session_id();
    println!("Publication session_id={}", pub_session_id);

    // Start recording BEFORE waiting for connection — the recording's subscription
    // is what connects to the MDC (Multi destination cast) publication.
    let recording_channel = format!(
        "aeron:udp?session-id={}|control={}",
        pub_session_id, CONTROL_ENDPOINT
    );
    let sub_id = archive.start_recording(&recording_channel, STREAM_ID, SourceLocation::Remote, true)?;
    println!("Recording started (subscription_id={})", sub_id);

    // Now wait for publisher to connect (the archive's recording subscription connects to it)
    let deadline = Instant::now() + Duration::from_secs(5);
    while !pub1.is_connected() && Instant::now() < deadline {
        thread::sleep(Duration::from_millis(10));
    }
    if !pub1.is_connected() {
        return Err("Publisher failed to connect".into());
    }

    // Publish initial batch (these get recorded)
    let initial_count = 20;
    for i in 0..initial_count {
        let msg = format!("recorded-msg-{}", i);
        while pub1.offer(msg.as_bytes()) < 0 {
            thread::yield_now();
        }
    }
    println!("Published {} messages (recorded)\n", initial_count);
    thread::sleep(Duration::from_millis(500));

    // Find the recording
    let mut recording_id: Option<i64> = None;
    let mut start_pos: i64 = 0;
    archive.list_recordings_for_uri(0, 100, "udp", STREAM_ID, |desc| {
        println!(
            "  Found recording {}: start={} stop={} session={} channel={}",
            desc.recording_id, desc.start_position, desc.stop_position,
            desc.session_id, desc.stripped_channel
        );
        recording_id = Some(desc.recording_id);
        start_pos = desc.start_position;
    })?;
    let recording_id = recording_id.ok_or("No recording found")?;
    println!();

    // --- Phase 2: ReplayMerge ---
    println!("--- Phase 2: ReplayMerge ---\n");

    // Subscription: control-mode=manual, filtered to the publication's session ID.
    let sub_channel = format!(
        "aeron:udp?control-mode=manual|session-id={}",
        pub_session_id
    );
    let mut sub = client.add_subscription(&sub_channel, STREAM_ID)?;

    // Replay channel: MUST include the publication's session-id so the archive replay
    // uses the same session ID as the live publication. This is what makes both the
    // replay and live transports feed into the SAME image, enabling active_transport_count >= 2
    // which is required for the MERGED transition.
    let replay_channel = format!("aeron:udp?session-id={}", pub_session_id);

    // Replay destination: where the subscription receives replayed data.
    let replay_destination = format!("aeron:udp?endpoint={}", REPLAY_ENDPOINT);

    // Live destination: endpoint where subscriber receives live data + control to register
    // with the MDC publication.
    let live_destination = format!(
        "aeron:udp?endpoint={}|control={}",
        LIVE_ENDPOINT, CONTROL_ENDPOINT
    );

    println!("Creating ReplayMerge:");
    println!("  recording_id={}", recording_id);
    println!("  start_position={}", start_pos);
    println!("  replay_channel={}", replay_channel);
    println!("  replay_destination={}", replay_destination);
    println!("  live_destination={}", live_destination);
    println!();

    let mut merge = ReplayMerge::new(
        &mut sub,
        &mut archive,
        &replay_channel,
        &replay_destination,
        &live_destination,
        recording_id,
        start_pos,
    )?;
    println!("ReplayMerge created successfully\n");

    // --- Phase 3: Poll through the merge ---
    println!("--- Phase 3: Polling ReplayMerge ---\n");

    let mut total_received = 0;
    let mut live_published = 0;
    let poll_deadline = Instant::now() + Duration::from_secs(30);

    let mut was_live_added = false;
    let mut was_merged = false;

    while Instant::now() < poll_deadline {
        // Drive the state machine
        merge.do_work()?;

        // Poll for fragments
        let fragments = merge.poll(10, |data| {
            let msg = std::str::from_utf8(data).unwrap_or("<binary>");
            total_received += 1;
            println!("  [{}] {}", total_received, msg);
        });

        // Monitor state transitions
        if !was_live_added && merge.is_live_added() {
            was_live_added = true;
            println!("\n  >>> Live destination added <<<\n");
        }

        if !was_merged && merge.is_merged() {
            was_merged = true;
            println!("\n  >>> Streams MERGED (replay -> live) <<<\n");
        }

        if merge.has_failed() {
            println!("  !!! ReplayMerge FAILED !!!");
            break;
        }

        // Keep publishing live messages until merged — the live transport must stay
        // active (sending data) for active_transport_count >= 2, which is required
        // for the ATTEMPT_LIVE_JOIN -> MERGED transition.
        if !merge.is_merged() && merge.is_live_added() {
            let msg = format!("live-msg-{}", live_published);
            if pub1.offer(msg.as_bytes()) > 0 {
                live_published += 1;
            }
        }

        // Once merged, drain remaining fragments then exit
        if merge.is_merged() && fragments == 0 {
            thread::sleep(Duration::from_millis(100));
            merge.poll(100, |data| {
                let msg = std::str::from_utf8(data).unwrap_or("<binary>");
                total_received += 1;
                if total_received <= 5 || total_received % 10 == 0 {
                println!("  [{}] {}", total_received, msg);
            }
            });
            break;
        }

        if fragments == 0 {
            thread::sleep(Duration::from_millis(1));
        }
    }

    // --- Summary ---
    println!("\n--- Summary ---\n");
    println!("  Total messages received: {}", total_received);
    println!("  Live messages published: {}", live_published);
    println!("  is_merged: {}", merge.is_merged());
    println!("  has_failed: {}", merge.has_failed());
    println!("  is_live_added: {}", merge.is_live_added());

    if merge.is_merged() {
        match merge.image() {
            Ok(image) => {
                println!(
                    "  Merged image: session_id={} position={}",
                    image.session_id(),
                    image.position()
                );
            }
            Err(e) => println!("  Could not get merged image: {}", e),
        }
    }

    // Cleanup
    archive.stop_recording(sub_id)?;
    println!("\nRecording stopped. Done!");

    Ok(())
}
