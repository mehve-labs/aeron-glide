#![cfg(all(feature = "archive", feature = "driver"))]
//! Archive client tests against a Java `ArchivingMediaDriver` (see
//! tests/common/archive.rs); skipped without Java or the aeron-all jar.

mod common;

use aeron_glide::archive::{
    self, AeronArchive, ArchiveErrorCode, PersistentSubscription, PersistentSubscriptionBuilder,
    RecordingSignal, RecordingSignalCode, ReplayMerge, ReplayParams, ReplicationParams,
    SourceLocation, recording_pos,
};
use aeron_glide::{ChannelBuilder, Context, ControlMode, ErrorKind, ExclusivePublication};
use common::{TIMEOUT, free_udp_port, offer, poll_n, wait_until};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

/// Publish `count` messages on a recorded IPC publication and stop recording it;
/// returns the recording ID and the stop position.
fn record(archive: &AeronArchive, stream_id: i32, count: usize) -> (i64, i64) {
    let publication = archive
        .add_recorded_publication("aeron:ipc", stream_id)
        .unwrap();
    let mut position = 0;
    for i in 0..count {
        position = offer(&publication, format!("message {i}").as_bytes());
    }
    let recording_id = find_recording(archive, stream_id, publication.session_id());
    wait_until("the recording to catch up", || {
        archive.get_recording_position(recording_id).unwrap() >= position
    });
    archive.stop_recording_publication(&publication).unwrap();
    wait_until("the recording to stop", || {
        archive.get_stop_position(recording_id).unwrap() == position
    });
    (recording_id, position)
}

fn find_recording(archive: &AeronArchive, stream_id: i32, session_id: i32) -> i64 {
    let mut recording_id = -1;
    wait_until("the recording", || {
        recording_id = archive
            .find_last_matching_recording(0, "aeron:", stream_id, session_id)
            .unwrap();
        recording_id >= 0
    });
    recording_id
}

fn offer_exclusive(publication: &mut ExclusivePublication, message: &[u8]) -> i64 {
    let deadline = Instant::now() + TIMEOUT;
    loop {
        match publication.offer(message) {
            Ok(position) => return position,
            Err(e) if e.is_retryable() && Instant::now() < deadline => std::thread::yield_now(),
            Err(e) => panic!("offer failed: {e}"),
        }
    }
}

#[test]
fn record_list_and_replay() {
    let driver = archive_or_skip!();
    let client = driver.client();
    let archive = driver.connect(&client);
    assert_eq!(archive.archive_id(), driver.archive_id);
    assert!(archive.control_session_id() > 0);
    let info = archive.context().unwrap();
    assert!(
        info.control_request_channel
            .contains(&driver.control_channel[10..]),
        "{info:?}"
    );
    assert_eq!(
        info.control_request_stream_id,
        common::archive::CONTROL_STREAM_ID
    );
    assert_eq!(
        info.control_response_stream_id,
        common::archive::RESPONSE_STREAM_ID
    );
    assert_eq!(info.message_timeout, TIMEOUT);
    assert_eq!(info.recording_events_channel, "", "unset");

    let (recording_id, stop_position) = record(&archive, 1, 5);
    assert_eq!(archive.get_start_position(recording_id).unwrap(), 0);
    assert_eq!(
        archive.get_max_recorded_position(recording_id).unwrap(),
        stop_position
    );
    assert_eq!(
        archive.get_recording_position(recording_id).unwrap(),
        archive::NULL_POSITION
    );

    let mut descriptors = Vec::new();
    assert_eq!(
        archive
            .list_recording(recording_id, |d| descriptors.push(d))
            .unwrap(),
        1
    );
    let d = &descriptors[0];
    assert_eq!(d.recording_id, recording_id);
    assert_eq!(d.stream_id, 1);
    assert_eq!(d.stop_position, stop_position);
    assert_eq!(d.source_identity, "aeron:ipc");
    assert_eq!(d.term_buffer_length, 65536);
    assert!(d.stop_timestamp >= d.start_timestamp);
    assert_eq!(
        archive.list_recording(recording_id + 100, |_| {}).unwrap(),
        0
    );

    let mut replay = archive
        .replay(
            recording_id,
            "aeron:ipc",
            2,
            &ReplayParams::new().position(0),
        )
        .unwrap();
    let mut messages = Vec::new();
    poll_n(&mut replay, 5, |data| {
        messages.push(String::from_utf8_lossy(data).into_owned())
    });
    assert_eq!(
        messages,
        (0..5).map(|i| format!("message {i}")).collect::<Vec<_>>()
    );
}

#[test]
fn recording_control_and_queries() {
    let driver = archive_or_skip!();
    let client = driver.client();
    let archive = driver.connect(&client);

    let subscription_id = archive
        .start_recording("aeron:ipc", 3, SourceLocation::Local, false)
        .unwrap();
    let mut subscriptions = Vec::new();
    assert_eq!(
        archive
            .list_recording_subscriptions(0, 10, "ipc", 3, true, |s| subscriptions.push(s))
            .unwrap(),
        1
    );
    assert_eq!(subscriptions[0].subscription_id, subscription_id);
    assert_eq!(subscriptions[0].stream_id, 3);
    assert_eq!(
        archive
            .list_recording_subscriptions(0, 10, "ipc", 4, true, |_| {})
            .unwrap(),
        0
    );

    // Two sessions on the recorded stream make two recordings.
    let first = client.add_exclusive_publication("aeron:ipc", 3).unwrap();
    let mut second = client.add_exclusive_publication("aeron:ipc", 3).unwrap();
    let position = offer_exclusive(&mut second, b"second");
    let first_id = find_recording(&archive, 3, first.session_id());
    let second_id = find_recording(&archive, 3, second.session_id());
    assert_ne!(first_id, second_id);
    wait_until("the recording", || {
        archive.get_recording_position(second_id).unwrap() == position
    });

    let mut all = Vec::new();
    archive
        .list_recordings(0, 10, |d| all.push(d.recording_id))
        .unwrap();
    assert_eq!(all, [first_id, second_id]);
    let mut matching = Vec::new();
    archive
        .list_recordings_for_uri(0, 10, "aeron:ipc", 3, |d| matching.push(d.session_id))
        .unwrap();
    assert_eq!(matching, [first.session_id(), second.session_id()]);
    assert_eq!(
        archive
            .find_last_matching_recording(0, "aeron:ipc", 3, second.session_id())
            .unwrap(),
        second_id
    );
    assert_eq!(
        archive
            .find_last_matching_recording(0, "udp", 3, second.session_id())
            .unwrap(),
        -1
    );

    assert!(archive.try_stop_recording_by_identity(first_id).unwrap());
    assert!(!archive.try_stop_recording_by_identity(first_id).unwrap());
    // Stopping a recording by identity may remove its recording subscription.
    archive.try_stop_recording(subscription_id).unwrap();
    assert!(!archive.try_stop_recording(subscription_id).unwrap());
    let err = archive.stop_recording(subscription_id).unwrap_err();
    assert_eq!(
        ArchiveErrorCode::of(&err),
        Some(ArchiveErrorCode::UnknownSubscription),
        "{err}"
    );

    // By channel and stream.
    archive
        .start_recording("aeron:ipc", 5, SourceLocation::Local, false)
        .unwrap();
    archive
        .stop_recording_by_channel_and_stream("aeron:ipc", 5)
        .unwrap();
    assert!(
        !archive
            .try_stop_recording_by_channel_and_stream("aeron:ipc", 5)
            .unwrap()
    );

    // Update, truncate and purge a stopped recording.
    archive.try_stop_recording_by_identity(second_id).unwrap();
    wait_until("the recording to stop", || {
        archive.get_stop_position(second_id).unwrap() == position
    });
    archive
        .update_channel(second_id, "aeron:ipc?alias=renamed")
        .unwrap();
    let mut original = String::new();
    archive
        .list_recording(second_id, |d| original = d.original_channel)
        .unwrap();
    assert_eq!(original, "aeron:ipc?alias=renamed");
    archive.truncate_recording(second_id, 0).unwrap();
    assert_eq!(archive.get_stop_position(second_id).unwrap(), 0);
    archive.purge_recording(second_id).unwrap();
    assert_eq!(archive.list_recording(second_id, |_| {}).unwrap(), 0);
    drop(first);
}

#[test]
fn archive_errors_carry_codes() {
    let driver = archive_or_skip!();
    let client = driver.client();
    let archive = driver.connect(&client);
    let err = archive.get_start_position(12345).unwrap_err();
    assert_eq!(
        ArchiveErrorCode::of(&err),
        Some(ArchiveErrorCode::UnknownRecording),
        "{err}"
    );
    assert_eq!(err.code(), -205, "{err}");
    let err = archive.purge_recording(54321).unwrap_err();
    assert_eq!(
        ArchiveErrorCode::of(&err),
        Some(ArchiveErrorCode::UnknownRecording),
        "{err}"
    );
    assert_eq!(archive.poll_for_error_response().unwrap(), None);
    archive.check_for_error_response().unwrap();

    for code in 200..=216 {
        assert_eq!(ArchiveErrorCode::from_code(code).unwrap().code(), code);
    }
    assert_eq!(ArchiveErrorCode::from_code(199), None);
    assert_eq!(ArchiveErrorCode::from_code(217), None);
    assert_eq!(
        ArchiveErrorCode::of(&aeron_glide::Error::new(ErrorKind::Timeout, "x")),
        None
    );
}

#[test]
fn replays_with_params() {
    let driver = archive_or_skip!();
    let client = driver.client();
    let archive = driver.connect(&client);
    let (recording_id, _) = record(&archive, 6, 10);
    let mut descriptor = None;
    archive
        .list_recording(recording_id, |d| descriptor = Some(d))
        .unwrap();
    let message_length = 64; // 32-byte header + "message N", aligned to 32

    // start_replay to a subscription of our own, from the third message, for two.
    let mut sub = client.add_subscription("aeron:ipc", 7).unwrap();
    let session = archive
        .start_replay(
            recording_id,
            "aeron:ipc",
            7,
            &ReplayParams::new()
                .position(2 * message_length)
                .length(2 * message_length),
        )
        .unwrap();
    let mut messages = Vec::new();
    poll_n(&mut sub, 2, |data| {
        messages.push(String::from_utf8_lossy(data).into_owned())
    });
    assert_eq!(messages, ["message 2", "message 3"]);
    let image = sub.images().into_iter().next().unwrap();
    assert_eq!(image.session_id(), session as i32);
    drop(image);

    // Replays can be stopped.
    let session = archive
        .start_replay(recording_id, "aeron:ipc", 8, &ReplayParams::new())
        .unwrap();
    archive.stop_replay(session).unwrap();
    archive.stop_all_replays(recording_id).unwrap();
    let err = archive
        .start_replay(
            recording_id,
            "aeron:ipc",
            8,
            &ReplayParams::new().position(33),
        )
        .unwrap_err();
    assert!(ArchiveErrorCode::of(&err).is_some(), "{err}");

    // Upstream (1.53.3): the C client's replay over a response channel reuses
    // the control publication's session ID and fails. Fails until fixed.
    let control = format!("localhost:{}", free_udp_port());
    let response = archive.replay(
        recording_id,
        &ChannelBuilder::udp()
            .control_mode(ControlMode::Response)
            .control_endpoint(&control)
            .build()
            .unwrap(),
        9,
        &ReplayParams::new(),
    );
    assert!(
        response.is_err(),
        "upstream fixed: test the response replay"
    );

    let params = ReplayParams::new()
        .bounding_limit_counter_id(4)
        .file_io_max_length(4096);
    assert!(params.is_bounded());
    assert_eq!(params.get_file_io_max_length(), 4096);
    assert!(descriptor.is_some());
}

#[test]
fn recording_signals_and_handlers() {
    let driver = archive_or_skip!();
    let client = driver.client();
    let signals = Arc::new(Mutex::new(Vec::<RecordingSignal>::new()));
    let idles = Arc::new(AtomicUsize::new(0));
    let credentials = Arc::new(AtomicUsize::new(0));
    let (sink, idle, creds) = (signals.clone(), idles.clone(), credentials.clone());
    let archive = driver
        .context(&client)
        .recording_signal_consumer(move |s| sink.lock().unwrap().push(*s))
        .idle_strategy(move |_| {
            idle.fetch_add(1, Ordering::Relaxed);
        })
        .credentials_supplier(
            move || {
                creds.fetch_add(1, Ordering::Relaxed);
                Vec::new()
            },
            |_| Vec::new(),
        )
        .error_handler(|e| panic!("unexpected archive error: {e}"))
        .connect()
        .unwrap();
    assert!(credentials.load(Ordering::Relaxed) >= 1);

    let (recording_id, _) = record(&archive, 10, 3);
    wait_until("start and stop signals", || {
        archive.poll_for_recording_signals().unwrap();
        let signals = signals.lock().unwrap();
        let codes: Vec<_> = signals
            .iter()
            .filter(|s| s.recording_id == recording_id)
            .map(|s| s.signal)
            .collect();
        codes.contains(&RecordingSignalCode::Start) && codes.contains(&RecordingSignalCode::Stop)
    });
    assert!(
        idles.load(Ordering::Relaxed) > 0,
        "the idle strategy runs while waiting"
    );
    assert_eq!(
        RecordingSignalCode::from_code(42),
        RecordingSignalCode::Unknown(42)
    );
}

#[test]
fn async_connect() {
    let driver = archive_or_skip!();
    let client = driver.client();
    let mut pending = driver.context(&client).connect_async().unwrap();
    let mut archive = None;
    wait_until("the archive to connect", || {
        archive = pending.poll().unwrap();
        archive.is_some()
    });
    assert_eq!(archive.unwrap().archive_id(), driver.archive_id);
    assert_eq!(pending.poll().unwrap_err().kind(), ErrorKind::IllegalState);

    // Abandoning a connection is fine.
    drop(driver.context(&client).connect_async().unwrap());

    // Connecting to nothing fails.
    let err = archive::Context::new()
        .aeron(&client)
        .control_request_channel(&format!("aeron:udp?endpoint=localhost:{}", free_udp_port()))
        .control_response_channel("aeron:udp?endpoint=localhost:0")
        .message_timeout(std::time::Duration::from_millis(300))
        .connect()
        .unwrap_err();
    assert!(
        matches!(
            err.kind(),
            ErrorKind::Timeout | ErrorKind::Archive | ErrorKind::Aeron
        ),
        "{err}"
    );
}

#[test]
fn extend_a_recording() {
    let driver = archive_or_skip!();
    let client = driver.client();
    let archive = driver.connect(&client);
    let mut publication = archive
        .add_recorded_exclusive_publication("aeron:ipc", 11)
        .unwrap();
    let (session_id, initial_term_id) = (publication.session_id(), publication.initial_term_id());
    let position = offer_exclusive(&mut publication, b"before");
    let recording_id = find_recording(&archive, 11, session_id);
    wait_until("the recording", || {
        archive.get_recording_position(recording_id).unwrap() == position
    });
    archive
        .stop_recording_exclusive_publication(&publication)
        .unwrap();
    drop(publication);
    wait_until("the recording to stop", || {
        archive.get_stop_position(recording_id).unwrap() == position
    });

    // Continue the stream where it stopped, and extend the recording.
    let channel = ChannelBuilder::ipc()
        .initial_position(position, initial_term_id, 65536)
        .session_id(session_id)
        .build()
        .unwrap();
    archive
        .extend_recording(recording_id, &channel, 11, SourceLocation::Local, false)
        .unwrap();
    // The driver keeps the first publication's session until it has lingered.
    let mut publication = None;
    wait_until("the continuing publication", || {
        publication = client.add_exclusive_publication(&channel, 11).ok();
        publication.is_some()
    });
    let mut publication = publication.unwrap();
    let extended = offer_exclusive(&mut publication, b"after");
    wait_until("the extension", || {
        archive.get_recording_position(recording_id).unwrap() == extended
    });
    let mut replay = archive
        .replay(
            recording_id,
            "aeron:ipc",
            12,
            &ReplayParams::new().length(extended),
        )
        .unwrap();
    let mut messages = Vec::new();
    poll_n(&mut replay, 2, |data| messages.push(data.to_vec()));
    assert_eq!(messages, [b"before".to_vec(), b"after".to_vec()]);
}

#[test]
fn segments() {
    let driver = archive_or_skip!();
    let client = driver.client();
    let archive = driver.connect(&client);
    // 256 KiB segments: record a bit over three.
    let publication = archive.add_recorded_publication("aeron:ipc", 13).unwrap();
    let message = vec![7u8; 4064]; // 4096 bytes with the header
    let mut position = 0;
    for _ in 0..200 {
        position = offer(&publication, &message);
    }
    let recording_id = find_recording(&archive, 13, publication.session_id());
    wait_until("the recording", || {
        archive.get_recording_position(recording_id).unwrap() == position
    });
    archive.stop_recording_publication(&publication).unwrap();
    wait_until("the recording to stop", || {
        archive.get_stop_position(recording_id).unwrap() == position
    });

    let segment = 262144;
    assert_eq!(
        AeronArchive::segment_file_base_position(0, 300_000, 65536, segment as i32),
        segment
    );
    assert_eq!(archive.purge_segments(recording_id, segment).unwrap(), 1);
    assert_eq!(archive.get_start_position(recording_id).unwrap(), segment);
    archive.detach_segments(recording_id, 2 * segment).unwrap();
    assert_eq!(
        archive.get_start_position(recording_id).unwrap(),
        2 * segment
    );
    assert_eq!(archive.attach_segments(recording_id).unwrap(), 1);
    assert_eq!(archive.get_start_position(recording_id).unwrap(), segment);
    archive.detach_segments(recording_id, 2 * segment).unwrap();
    assert_eq!(archive.delete_detached_segments(recording_id).unwrap(), 1);
    let err = archive
        .migrate_segments(recording_id, recording_id + 1)
        .unwrap_err();
    assert!(ArchiveErrorCode::of(&err).is_some(), "{err}");
}

#[test]
fn replicate_within_an_archive() {
    let driver = archive_or_skip!();
    let client = driver.client();
    let archive = driver.connect(&client);
    let (recording_id, stop_position) = record(&archive, 14, 4);
    let replication_id = archive
        .replicate(
            recording_id,
            common::archive::CONTROL_STREAM_ID,
            &driver.control_channel,
            &ReplicationParams::new(),
        )
        .unwrap();
    assert!(replication_id >= 0);
    let mut replica = None;
    wait_until("the replica", || {
        archive.poll_for_recording_signals().unwrap();
        archive
            .list_recordings(recording_id + 1, 10, |d| {
                if d.stop_position == stop_position {
                    replica = Some(d.recording_id)
                }
            })
            .unwrap();
        replica.is_some()
    });
    assert!(!archive.try_stop_replication(replication_id + 1000).unwrap());
    let err = archive.stop_replication(replication_id + 1000).unwrap_err();
    assert_eq!(
        ArchiveErrorCode::of(&err),
        Some(ArchiveErrorCode::UnknownReplication),
        "{err}"
    );
    let params = ReplicationParams::new()
        .stop_position(64)
        .live_destination("x")
        .encoded_credentials(b"c");
    assert_eq!(params.get_stop_position(), 64);
    assert_eq!(params.get_encoded_credentials(), b"c");
}

#[test]
fn recording_position_counters() {
    let driver = archive_or_skip!();
    let client = driver.client();
    let archive = driver.connect(&client);
    let publication = archive.add_recorded_publication("aeron:ipc", 15).unwrap();
    let position = offer(&publication, b"x");
    let recording_id = find_recording(&archive, 15, publication.session_id());
    let reader = client.counters_reader();
    let mut counter_id = None;
    wait_until("the recording position counter", || {
        counter_id = recording_pos::find_counter_id_by_recording_id(&reader, recording_id);
        counter_id.is_some()
    });
    let counter_id = counter_id.unwrap();
    assert_eq!(
        recording_pos::find_counter_id_by_session_id(&reader, publication.session_id()),
        Some(counter_id)
    );
    assert_eq!(
        recording_pos::get_recording_id(&reader, counter_id),
        Some(recording_id)
    );
    assert_eq!(
        recording_pos::get_source_identity(&reader, counter_id).unwrap(),
        "aeron:ipc"
    );
    assert!(recording_pos::is_active(&reader, counter_id, recording_id).unwrap());
    assert!(!recording_pos::is_active(&reader, counter_id, recording_id + 1).unwrap());
    wait_until("the position", || {
        reader.get_counter_value(counter_id).unwrap() == position
    });
    assert_eq!(recording_pos::get_recording_id(&reader, 0), None);
    assert_eq!(
        recording_pos::find_counter_id_by_recording_id(&reader, 999),
        None
    );
    assert_eq!(recording_pos::get_source_identity(&reader, 0).unwrap(), "");
    assert!(recording_pos::get_source_identity(&reader, -1).is_err());
    assert!(recording_pos::is_active(&reader, -1, recording_id).is_err());

    archive.stop_recording_publication(&publication).unwrap();
    wait_until("the counter to go", || {
        !recording_pos::is_active(&reader, counter_id, recording_id).unwrap()
    });
}

#[test]
fn replay_merge_joins_the_live_stream() {
    let driver = archive_or_skip!();
    let client = driver.client();
    let mut archive = driver.connect(&client);
    let control = format!("localhost:{}", free_udp_port());
    let publication = client
        .add_publication(
            &format!("aeron:udp?control={control}|control-mode=dynamic|linger=0"),
            16,
        )
        .unwrap();
    let session_id = publication.session_id();
    archive
        .start_recording(
            &format!("aeron:udp?session-id={session_id}|control={control}"),
            16,
            SourceLocation::Remote,
            true,
        )
        .unwrap();
    wait_until("the recording subscription", || publication.is_connected());
    for i in 0..20 {
        offer(&publication, format!("recorded {i}").as_bytes());
    }
    let recording_id = find_recording(&archive, 16, session_id);

    let mut sub = client
        .add_subscription(
            &format!("aeron:udp?control-mode=manual|session-id={session_id}"),
            16,
        )
        .unwrap();
    let mut merge = ReplayMerge::new(
        &mut sub,
        &mut archive,
        &format!("aeron:udp?session-id={session_id}"),
        "aeron:udp?endpoint=localhost:0",
        &format!(
            "aeron:udp?endpoint=localhost:{}|control={control}",
            free_udp_port()
        ),
        recording_id,
        0,
    )
    .unwrap();
    let mut received = Vec::new();
    let mut published = 20;
    let deadline = Instant::now() + TIMEOUT * 3;
    while !(merge.is_merged() && received.len() >= published) {
        assert!(
            Instant::now() < deadline,
            "merged={} received={}",
            merge.is_merged(),
            received.len()
        );
        assert!(!merge.has_failed());
        merge
            .poll(10, |data, _| {
                received.push(String::from_utf8_lossy(data).into_owned())
            })
            .unwrap();
        if merge.is_live_added() && published < 40 {
            offer(&publication, format!("live {published}").as_bytes());
            published += 1;
        }
    }
    assert_eq!(received[0], "recorded 0");
    assert!(received.iter().any(|m| m.starts_with("live")));
    assert!(merge.image().is_some());
}

#[test]
fn persistent_subscription_replays_then_goes_live() {
    let driver = archive_or_skip!();
    let client = driver.client();
    let archive = driver.connect(&client);
    let live_channel = format!("aeron:udp?endpoint=localhost:{}", free_udp_port());
    let publication = archive.add_recorded_publication(&live_channel, 17).unwrap();
    // A plain subscriber keeps the publication connected (the archive's spy
    // recording does not).
    let _keep = client.add_subscription(&live_channel, 17).unwrap();
    wait_until("the publication to connect", || publication.is_connected());
    for i in 0..5 {
        offer(&publication, format!("old {i}").as_bytes());
    }
    let recording_id = find_recording(&archive, 17, publication.session_id());

    let joined = Arc::new(AtomicUsize::new(0));
    let flag = joined.clone();
    let mut persistent = PersistentSubscriptionBuilder::new()
        .archive_context(driver.context(&client))
        .aeron(&client)
        .recording_id(recording_id)
        .start_position(PersistentSubscription::FROM_START)
        .live_channel(&live_channel)
        .live_stream_id(17)
        .replay_channel("aeron:udp?endpoint=localhost:0")
        .replay_stream_id(18)
        .on_live_joined(move || {
            flag.fetch_add(1, Ordering::SeqCst);
        })
        .on_error(|e| eprintln!("persistent subscription: {e}"))
        .create()
        .unwrap();
    let mut received = Vec::new();
    let mut published = 5;
    let deadline = Instant::now() + TIMEOUT * 3;
    while !(persistent.is_live() && received.len() >= published) {
        assert!(
            Instant::now() < deadline,
            "live={} received={received:?}",
            persistent.is_live()
        );
        assert!(!persistent.has_failed());
        persistent
            .poll(10, |data, _| {
                received.push(String::from_utf8_lossy(data).into_owned())
            })
            .unwrap();
        if received.len() >= 5 && published < 10 {
            offer(&publication, format!("new {published}").as_bytes());
            published += 1;
        }
    }
    assert_eq!(received[..5], ["old 0", "old 1", "old 2", "old 3", "old 4"]);
    assert!(joined.load(Ordering::SeqCst) >= 1);
    assert!(!persistent.is_replaying());

    // A builder without the required settings fails.
    assert!(PersistentSubscriptionBuilder::new().create().is_err());
    assert!(
        PersistentSubscriptionBuilder::new()
            .archive_context(driver.context(&client))
            .create()
            .is_err()
    );
}

#[test]
fn invoker_mode_and_threads() {
    let driver = archive_or_skip!();
    let client = AeronClient_invoker(&driver);
    let archive = Arc::new(driver.connect(&client));
    let (recording_id, _) = record(&archive, 19, 3);
    // Requests from several threads are serialised.
    let threads: Vec<_> = (0..4)
        .map(|_| {
            let archive = archive.clone();
            std::thread::spawn(move || {
                for _ in 0..10 {
                    assert_eq!(archive.list_recording(recording_id, |_| {}).unwrap(), 1);
                }
            })
        })
        .collect();
    for thread in threads {
        thread.join().unwrap();
    }
    // The archive keeps its client alive.
    drop(client);
    assert_eq!(archive.get_start_position(recording_id).unwrap(), 0);
}

#[allow(non_snake_case)]
fn AeronClient_invoker(driver: &common::archive::ArchiveDriver) -> aeron_glide::AeronClient {
    aeron_glide::AeronClient::connect(
        Context::new()
            .aeron_dir(&driver.aeron_dir)
            .use_conductor_agent_invoker(true),
    )
    .unwrap()
}

#[test]
fn replay_merge_validates_the_replay_destination() {
    let driver = archive_or_skip!();
    let client = driver.client();
    let mut archive = driver.connect(&client);
    let mut sub = client
        .add_subscription("aeron:udp?control-mode=manual", 20)
        .unwrap();
    for destination in [
        "",
        "aeron:ipc",
        "aeron:udp?control=localhost:1",
        "aeron:udp?endpoint=x",
    ] {
        let err = ReplayMerge::new(
            &mut sub,
            &mut archive,
            "aeron:udp?endpoint=localhost:0",
            destination,
            "aeron:udp?endpoint=localhost:0",
            0,
            0,
        )
        .expect_err(destination);
        assert!(
            matches!(
                err.kind(),
                ErrorKind::IllegalArgument | ErrorKind::IllegalState
            ),
            "{destination}: {err}"
        );
    }
}

#[test]
fn persistent_subscription_counters_are_handed_over() {
    // No archive needed: create fails, and the C context closes the counters it
    // was given, which must not be closed (freed) again.
    let driver = common::TestDriver::start();
    let client = driver.client();
    let counter = client.add_counter(1001, &[], "ps state").unwrap();
    let result = PersistentSubscriptionBuilder::new()
        .archive_context(archive::Context::new().aeron(&client))
        .aeron(&client)
        .state_counter(counter)
        .live_channel("aeron:ipc")
        .replay_channel("aeron:ipc")
        .create();
    assert!(result.is_err());

    // A counter of another client, or a handle from a reader, is refused.
    let other = driver.client();
    let foreign = other.add_counter(1001, &[], "foreign").unwrap();
    assert!(
        PersistentSubscriptionBuilder::new()
            .archive_context(archive::Context::new().aeron(&client))
            .aeron(&client)
            .state_counter(foreign)
            .live_channel("aeron:ipc")
            .replay_channel("aeron:ipc")
            .recording_id(1)
            .create()
            .is_err()
    );
    let reader = client.counters_reader();
    let owned = client.add_counter(1001, &[], "viewed").unwrap();
    let view = reader.counter(owned.registration_id(), owned.id()).unwrap();
    let err = PersistentSubscriptionBuilder::new()
        .live_joined_counter(view)
        .create()
        .unwrap_err();
    assert_eq!(err.kind(), ErrorKind::IllegalArgument, "{err}");
}

#[test]
fn recording_source_identity_is_bounded() {
    // A counter posing as a recording position with a bogus identity length.
    let driver = common::TestDriver::start();
    let client = driver.client();
    let mut key = Vec::new();
    key.extend_from_slice(&7i64.to_le_bytes());
    key.extend_from_slice(&1i32.to_le_bytes());
    key.extend_from_slice(&(-1i32).to_le_bytes());
    let negative = client.add_counter(100, &key, "fake recording").unwrap();
    let reader = client.counters_reader();
    assert_eq!(
        recording_pos::get_source_identity(&reader, negative.id()).unwrap(),
        ""
    );
    key.truncate(12);
    key.extend_from_slice(&i32::MAX.to_le_bytes());
    key.extend_from_slice(b"abc");
    let huge = client.add_counter(100, &key, "fake recording").unwrap();
    let identity = recording_pos::get_source_identity(&reader, huge.id()).unwrap();
    assert!(
        identity.starts_with("abc") && identity.len() == 96,
        "{identity:?}"
    );
}

#[test]
fn error_responses_with_tiny_buffers_and_handler_requests() {
    let driver = archive_or_skip!();
    let client = driver.client();
    let archive = driver
        .context(&client)
        .max_error_message_length(0)
        .connect()
        .unwrap();
    assert_eq!(archive.poll_for_error_response().unwrap(), None);

    // Archive requests from a client handler fail instead of hanging.
    let shared = Arc::new(archive);
    let in_handler = shared.clone();
    let result = Arc::new(Mutex::new(None));
    let sink = result.clone();
    let id = client
        .add_available_counter_handler(move |_| {
            let mut sink = sink.lock().unwrap();
            if sink.is_none() {
                *sink = Some(
                    in_handler
                        .add_recorded_publication("aeron:ipc", 21)
                        .map(drop),
                );
            }
        })
        .unwrap();
    let _counter = client.add_counter(1001, &[], "trigger").unwrap();
    wait_until("the handler", || result.lock().unwrap().is_some());
    client.remove_available_counter_handler(id).unwrap();
    let err = result.lock().unwrap().take().unwrap().unwrap_err();
    assert_eq!(err.kind(), ErrorKind::Reentrant, "{err}");
}

#[test]
fn listing_counts_and_reentrant_handlers() {
    let driver = archive_or_skip!();
    let client = driver.client();
    let reentrant = Arc::new(Mutex::new(Vec::new()));
    let sink = reentrant.clone();
    let archive: Arc<Mutex<Option<Arc<AeronArchive>>>> = Arc::new(Mutex::new(None));
    let inner = archive.clone();
    let connected = Arc::new(
        driver
            .context(&client)
            .recording_signal_consumer(move |_| {
                if let Some(archive) = inner.lock().unwrap().as_ref() {
                    sink.lock()
                        .unwrap()
                        .push(archive.list_recordings(0, 10, |_| {}).unwrap_err().kind());
                }
            })
            .connect()
            .unwrap(),
    );
    *archive.lock().unwrap() = Some(connected.clone());
    let (recording_id, _) = record(&connected, 22, 1);
    wait_until("a signal", || {
        connected.poll_for_recording_signals().unwrap();
        !reentrant.lock().unwrap().is_empty()
    });
    assert!(
        reentrant
            .lock()
            .unwrap()
            .iter()
            .all(|k| *k == ErrorKind::Reentrant)
    );
    archive.lock().unwrap().take();

    // Zero and negative counts don't reach the archive; listing still works.
    let start = Instant::now();
    assert_eq!(connected.list_recordings(0, 0, |_| {}).unwrap(), 0);
    assert_eq!(
        connected
            .list_recordings_for_uri(0, 0, "ipc", 22, |_| {})
            .unwrap(),
        0
    );
    assert!(start.elapsed() < std::time::Duration::from_secs(1));
    for err in [
        connected.list_recordings(0, -1, |_| {}).unwrap_err(),
        connected
            .list_recordings_for_uri(0, -1, "ipc", 22, |_| {})
            .unwrap_err(),
        connected
            .list_recording_subscriptions(0, -1, "ipc", 22, true, |_| {})
            .unwrap_err(),
    ] {
        assert_eq!(err.kind(), ErrorKind::IllegalArgument, "{err}");
    }
    assert_eq!(
        connected.list_recordings(recording_id, 10, |_| {}).unwrap(),
        1
    );

    // Replay what is recorded, then stop.
    let mut replay = connected
        .replay(
            recording_id,
            "aeron:ipc",
            23,
            &ReplayParams::new().length(archive::REPLAY_ALL_AND_STOP),
        )
        .unwrap();
    poll_n(&mut replay, 1, |_| {});
}

#[test]
fn closed_clients_fail_instead_of_hanging() {
    let driver = archive_or_skip!();
    let client = aeron_glide::AeronClient::connect(
        Context::new()
            .aeron_dir(&driver.aeron_dir)
            .use_conductor_agent_invoker(true)
            .driver_timeout(std::time::Duration::from_secs(1))
            .error_handler(|_| {}),
    )
    .unwrap();
    // Asynchronous connect drives the invoker client's conductor itself.
    let mut pending = driver.context(&client).connect_async().unwrap();
    let mut archive = None;
    wait_until("the archive to connect", || {
        archive = pending.poll().unwrap();
        archive.is_some()
    });
    let archive = archive.unwrap();

    // Stop the driver: the client times out and closes.
    let (connect, connect_async) = (driver.context(&client), driver.context(&client));
    drop(driver);
    wait_until("the client to close", || {
        client.invoke().ok();
        client.is_closed()
    });
    let start = Instant::now();
    for err in [
        archive
            .add_recorded_publication("aeron:ipc", 24)
            .map(drop)
            .unwrap_err(),
        archive
            .replay(0, "aeron:ipc", 25, &ReplayParams::new())
            .map(drop)
            .unwrap_err(),
        connect.connect().map(drop).unwrap_err(),
        connect_async.connect_async().map(drop).unwrap_err(),
        // Control requests too, not an error from the dead connection.
        archive.get_start_position(0).unwrap_err(),
        archive.stop_recording(0).unwrap_err(),
    ] {
        assert_eq!(err.kind(), ErrorKind::IllegalState, "{err}");
    }
    assert!(start.elapsed() < std::time::Duration::from_secs(2));
}

/// The archive client waits for the next session id while connecting without a
/// deadline of its own; once the client's conductor stops (here the driver
/// stops answering and times out), that wait used to spin forever.
#[cfg(unix)]
#[test]
fn connect_fails_when_the_driver_stops_answering() {
    let driver = archive_or_skip!();
    let client = aeron_glide::AeronClient::connect(
        Context::new()
            .aeron_dir(&driver.aeron_dir)
            .driver_timeout(std::time::Duration::from_secs(1))
            .error_handler(|_| {}),
    )
    .unwrap();
    let context = driver.context(&client);
    driver.signal("-STOP");
    let start = Instant::now();
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let result = context.connect().map(drop);
        tx.send(result).ok();
        drop(client);
    });
    let result = rx.recv_timeout(std::time::Duration::from_secs(30));
    driver.signal("-CONT");
    let err = result.expect("connect hangs").expect_err("no driver");
    assert_eq!(err.kind(), ErrorKind::DriverTimeout, "{err}");
    assert!(
        start.elapsed() < std::time::Duration::from_secs(30),
        "{err}"
    );
}

/// A listing consumer runs inside the archive client's response poll: other
/// archive requests from it fail instead of polling the same responses again.
#[test]
fn requests_from_list_consumers_are_reentrant() {
    let driver = archive_or_skip!();
    let client = driver.client();
    let archive = driver.connect(&client);
    let (recording_id, _) = record(&archive, 1, 2);
    let mut kinds = Vec::new();
    archive
        .list_recording(recording_id, |_| {
            kinds.push(archive.poll_for_recording_signals().unwrap_err().kind());
            kinds.push(archive.poll_for_error_response().unwrap_err().kind());
            kinds.push(archive.check_for_error_response().unwrap_err().kind());
        })
        .unwrap();
    assert_eq!(kinds, [ErrorKind::Reentrant; 3]);
    archive.poll_for_recording_signals().unwrap();
}
