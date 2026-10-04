//! Handling archive failures:
//!
//! 1. an archive that does not answer: `connect` times out (bounded by the
//!    context's message timeout), so retry a bounded number of times;
//! 2. requests the archive rejects fail with an [`Error`] whose code maps to a
//!    typed [`ArchiveErrorCode`] (`UnknownRecording`, `ActiveSubscription`, ...)
//!    to decide how to react; the session stays usable;
//! 3. errors the archive reports asynchronously go to the context's error
//!    handler, or are polled with `poll_for_error_response`;
//! 4. once the Aeron client is closed (here: a client in agent invoker mode
//!    whose conductor was not run within the driver's client liveness timeout,
//!    10 s by default), every archive request on it (control requests, adding
//!    resources, connecting) fails at once instead of hanging, with
//!    `ErrorKind::IllegalState` ("the Aeron client is closed").
//!
//! Needs the archive server (an `ArchivingMediaDriver` on the default Aeron
//! directory, control channel `localhost:8010`; set `AERON_DIR` and
//! `AERON_ARCHIVE_CONTROL_CHANNEL` for another one):
//!
//! ```text
//! ./scripts/start-archive.sh
//! cargo run --features archive --example archive_error_handling
//! ```

use aeron_glide::archive::{self, AeronArchive, ArchiveErrorCode, ReplayParams, SourceLocation};
use aeron_glide::{AeronClient, CncFile, Context, Error};
use std::time::Duration;

fn context(client: &AeronClient, control: &str) -> archive::Context {
    archive::Context::new()
        .aeron(client)
        .control_request_channel(control)
        .control_response_channel("aeron:udp?endpoint=localhost:0")
        .message_timeout(Duration::from_secs(2))
        .error_handler(|e| eprintln!("asynchronous archive error: {e}"))
}

/// Connect, retrying a few times with a growing pause.
fn connect_with_retry(
    client: &AeronClient,
    control: &str,
    attempts: u32,
) -> Result<AeronArchive, Error> {
    let mut pause = Duration::from_millis(100);
    for attempt in 1.. {
        match context(client, control).connect() {
            Ok(archive) => return Ok(archive),
            Err(e) if attempt < attempts => {
                println!("  attempt {attempt} failed: {}", first_line(&e));
                std::thread::sleep(pause);
                pause *= 2;
            }
            Err(e) => return Err(e),
        }
    }
    unreachable!()
}

/// An error on one line, without the C call sites Aeron adds to its messages.
fn first_line(e: &Error) -> String {
    let lines: Vec<&str> = e
        .message()
        .lines()
        .map(|line| line.split_once("] ").map_or(line, |(_, text)| text).trim())
        .filter(|line| !line.is_empty())
        .collect();
    format!("{:?}: {}", e.kind(), lines.join(" | "))
}

/// Print how a rejected request failed and how one would react.
fn report(request: &str, result: Result<impl std::fmt::Debug, Error>) {
    let Err(e) = result else {
        println!("{request}: unexpectedly succeeded");
        return;
    };
    let reaction = match ArchiveErrorCode::of(&e) {
        Some(ArchiveErrorCode::UnknownRecording) => "refresh the recording ID from the catalog",
        Some(ArchiveErrorCode::ActiveSubscription) => "already recording: nothing to do",
        Some(ArchiveErrorCode::UnknownSubscription | ArchiveErrorCode::UnknownReplication) => {
            "already stopped: nothing to do"
        }
        Some(_) => "rejected by the archive",
        None => "not an archive rejection",
    };
    println!(
        "{request}\n    {}\n    code {} = {:?}: {reaction}",
        first_line(&e),
        e.code(),
        ArchiveErrorCode::of(&e),
    );
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let control = std::env::var("AERON_ARCHIVE_CONTROL_CHANNEL")
        .unwrap_or_else(|_| "aeron:udp?endpoint=localhost:8010".to_string());
    let client = AeronClient::new()?;

    println!("--- 1. An archive that does not answer");
    let err = connect_with_retry(&client, "aeron:udp?endpoint=localhost:18999", 2).unwrap_err();
    println!("  gave up: {}", first_line(&err));
    let archive = connect_with_retry(&client, &control, 5)?;
    println!("  connected to archive {}", archive.archive_id());

    println!("\n--- 2. Requests the archive rejects");
    report(
        "get_start_position(unknown recording)",
        archive.get_start_position(1 << 40),
    );
    report(
        "start_replay(unknown recording)",
        archive.start_replay(1 << 40, "aeron:ipc", 1002, &ReplayParams::new()),
    );
    // (Stop a recording left over by an interrupted earlier run.)
    archive.try_stop_recording_by_channel_and_stream("aeron:ipc", 1003)?;
    let subscription_id =
        archive.start_recording("aeron:ipc", 1003, SourceLocation::Local, false)?;
    report(
        "start_recording(a channel and stream already recorded)",
        archive.start_recording("aeron:ipc", 1003, SourceLocation::Local, false),
    );
    archive.stop_recording(subscription_id)?;
    report(
        "stop_recording(stopped subscription)",
        archive.stop_recording(subscription_id),
    );
    report(
        "stop_replication(unknown replication)",
        archive.stop_replication(1 << 40),
    );
    // Rejections leave the session usable; `try_*` variants report "not found" as `false`.
    println!(
        "  still usable: try_stop_recording -> {}, {} recordings listed",
        archive.try_stop_recording(subscription_id)?,
        archive.list_recordings(0, 1000, |_| {})?
    );

    println!("\n--- 3. Asynchronous error responses");
    // Poll periodically (e.g. in a duty cycle) when no request is awaiting a response.
    match archive.poll_for_error_response()? {
        Some(message) => println!("  the archive reported: {message}"),
        None => println!("  no pending error response"),
    }
    archive.check_for_error_response()?; // the same, as an `Err`

    println!("\n--- 4. A closed Aeron client");
    // A client in agent invoker mode runs its conductor only when invoked.
    let invoker = AeronClient::connect(
        Context::new()
            .use_conductor_agent_invoker(true)
            .error_handler(|e| println!("  client error: {}", first_line(e))),
    )?;
    let mut pending = context(&invoker, &control).connect_async()?;
    let session = loop {
        if let Some(archive) = pending.poll()? {
            break archive; // `poll` also runs the invoker client's conductor
        }
        std::thread::sleep(Duration::from_millis(1));
    };
    // The client must be serviced within the driver's client liveness timeout.
    let cnc = CncFile::map_existing(invoker.aeron_dir())?;
    let timeout = cnc.constants()?.client_liveness_timeout + Duration::from_millis(500);
    println!("  connected with an invoker client; not invoking it for {timeout:?}...");
    std::thread::sleep(timeout);
    let _ = invoker.invoke(); // the conductor notices it was not serviced in time
    println!("  client closed: {}", invoker.is_closed());
    let started = std::time::Instant::now();
    for err in [
        session.get_start_position(0).map(drop).unwrap_err(),
        session
            .add_recorded_publication("aeron:ipc", 1004)
            .map(drop)
            .unwrap_err(),
        context(&invoker, &control).connect().map(drop).unwrap_err(),
    ] {
        println!("  {} ({:?})", first_line(&err), started.elapsed());
    }
    Ok(())
}
