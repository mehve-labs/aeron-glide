#![cfg(feature = "driver")]
mod common;

use aeron_glide::{Context, ErrorKind};
use common::{TestDriver, wait_until};
use std::time::Duration;

#[test]
fn async_adds_complete() {
    let driver = TestDriver::start();
    let client = driver.client();

    let mut pending = client.add_publication_async("aeron:ipc", 1).unwrap();
    let id = pending.registration_id();
    let mut publication = None;
    wait_until("the publication", || {
        publication = pending.poll().unwrap();
        publication.is_some()
    });
    assert_eq!(publication.unwrap().registration_id(), id);
    // Polling again after completion is an error, not a second resource.
    assert_eq!(
        pending.poll().err().unwrap().kind(),
        ErrorKind::IllegalState
    );

    let exclusive = client
        .add_exclusive_publication_async("aeron:ipc", 2)
        .unwrap()
        .wait()
        .unwrap();
    assert_eq!(exclusive.stream_id(), 2);

    let mut pending = client.add_subscription_async("aeron:ipc", 3).unwrap();
    let id = pending.registration_id();
    let mut subscription = None;
    wait_until("the subscription", || {
        subscription = pending.poll().unwrap();
        subscription.is_some()
    });
    assert_eq!(subscription.unwrap().registration_id(), id);
}

#[test]
fn rejected_async_add_is_an_error() {
    let driver = TestDriver::start();
    let client = driver.client();
    let mut pending = client
        .add_publication_async("aeron:udp?endpoint=not-a-host-name.invalid:1", 1)
        .unwrap();
    let mut result = Ok(None);
    wait_until("the driver to answer", || {
        result = pending.poll();
        !matches!(result, Ok(None))
    });
    let err = result.expect_err("the driver rejects the channel");
    assert_eq!(err.kind(), ErrorKind::Registration, "{err}");
    assert!(err.code() < 0, "{err}");
    // The synchronous form reports the same error.
    assert!(
        client
            .add_publication("aeron:udp?endpoint=not-a-host-name.invalid:1", 1)
            .is_err()
    );
}

#[test]
fn client_accessors() {
    let driver = TestDriver::start();
    let client = driver.client();
    assert_eq!(client.aeron_dir(), driver.dir);
    assert!(client.cnc_file_name().unwrap().ends_with("cnc.dat"));
    assert_eq!(client.driver_timeout(), Duration::from_secs(10));
    assert!(client.client_id() >= 0);
    let first = client.next_correlation_id();
    let second = client.next_correlation_id();
    assert!(second > first);

    let other = driver.client();
    assert_ne!(other.client_id(), client.client_id());
}

#[test]
fn context_utilities() {
    assert!(!Context::default_aeron_path().unwrap().is_empty());

    let driver = TestDriver::start();
    // Sent to the running driver (its default validator then rejects it).
    assert!(Context::request_driver_termination(&driver.dir, b"token").unwrap());
    let client = driver.client();
    client.add_publication("aeron:ipc", 1).unwrap();

    // No driver (no CnC file) in this directory.
    let missing = std::env::temp_dir().join("aeron-glide-no-driver-here");
    assert!(Context::request_driver_termination(missing.to_str().unwrap(), b"").is_err());
}

#[test]
fn abandoned_pending_adds_are_closed() {
    let driver = TestDriver::start();
    let client = driver.client();
    let publication = client.add_publication("aeron:ipc", 7).unwrap();
    // A subscription whose pending add is dropped still gets created...
    drop(client.add_subscription_async("aeron:ipc", 7).unwrap());
    wait_until("the abandoned subscription to connect", || {
        publication.is_connected()
    });
    // ...and is closed by a later client call.
    wait_until("the abandoned subscription to close", || {
        drop(client.add_publication_async("aeron:ipc", 8).unwrap().wait());
        !publication.is_connected()
    });
}

#[test]
fn polling_after_a_failure_reports_done() {
    let driver = TestDriver::start();
    let client = driver.client();
    let mut pending = client
        .add_publication_async("aeron:udp?endpoint=not-a-host-name.invalid:1", 1)
        .unwrap();
    let mut result = Ok(None);
    wait_until("the driver to answer", || {
        result = pending.poll();
        !matches!(result, Ok(None))
    });
    let err = result.err().unwrap();
    assert_eq!(err.kind(), ErrorKind::Registration, "{err}");
    assert!(err.code() < 0, "{err}");
    assert_eq!(
        pending.poll().err().unwrap().kind(),
        ErrorKind::IllegalState
    );
    assert!(client.client_name().is_empty());
    assert_eq!(client.idle_sleep_duration(), Duration::from_millis(16));
}

#[test]
fn resources_format_with_debug() {
    let driver = TestDriver::start();
    let client = driver.client();
    let sub = client.add_subscription("aeron:ipc", 4).unwrap();
    let publication = client.add_publication("aeron:ipc", 4).unwrap();
    let exclusive = client.add_exclusive_publication("aeron:ipc", 4).unwrap();
    common::wait_connected(&sub);
    let pending = client.add_counter_async(1001, &[], "debug").unwrap();
    let output = format!(
        "{client:?} {sub:?} {publication:?} {exclusive:?} {:?} {pending:?} {:?} {:?}",
        sub.images(),
        client.counters_reader(),
        Context::new().client_name("x"),
    );
    assert!(output.contains("AeronClient"), "{output}");
    assert!(output.contains("stream_id: 4"), "{output}");
    assert!(output.contains("Image"), "{output}");
    assert!(output.contains("PendingAdd"), "{output}");
    // As Debug escapes it (backslashes on Windows).
    let dir = format!("{:?}", driver.dir);
    assert!(format!("{:?}", driver.driver()).contains(&dir));
}

#[test]
fn interior_nul_characters_are_rejected() {
    let driver = TestDriver::start();
    let client = driver.client();
    for result in [
        client.add_publication("aeron:ipc\0junk", 1).map(drop),
        client.add_subscription("aeron:ipc\0junk", 1).map(drop),
        client
            .add_exclusive_publication("aeron:ipc\0junk", 1)
            .map(drop),
    ] {
        assert_eq!(result.unwrap_err().kind(), ErrorKind::IllegalArgument);
    }
    let publication = client
        .add_publication("aeron:udp?control=localhost:40999|control-mode=manual", 2)
        .unwrap();
    assert_eq!(
        publication
            .add_destination("aeron:udp?endpoint=localhost:1\0")
            .unwrap_err()
            .kind(),
        ErrorKind::IllegalArgument
    );
    let err =
        aeron_glide::AeronClient::connect(Context::new().aeron_dir(format!("{}\0x", driver.dir)))
            .expect_err("NUL in the directory");
    assert_eq!(err.kind(), ErrorKind::IllegalArgument, "{err}");
    let err = aeron_glide::MediaDriver::builder()
        .dir("/tmp/a\0b")
        .start()
        .expect_err("NUL in the directory");
    assert_eq!(err.kind(), ErrorKind::IllegalArgument, "{err}");
}

/// Settings left unset come from Aeron's environment variables, as in its C and
/// Java clients (the C++ wrapper used to overwrite them with its defaults).
#[test]
fn unset_settings_come_from_the_environment() {
    if !common::in_child_process(
        "unset_settings_come_from_the_environment",
        &[
            ("AERON_CLIENT_NAME", "from-the-env"),
            ("AERON_DRIVER_TIMEOUT", "7000"), // milliseconds
            ("AERON_CLIENT_IDLE_SLEEP_DURATION", "2ms"),
        ],
    ) {
        return;
    }
    let driver = TestDriver::start();
    let client = driver.connect(Context::new());
    assert_eq!(client.client_name(), "from-the-env");
    assert_eq!(client.driver_timeout(), std::time::Duration::from_secs(7));
    assert_eq!(
        client.idle_sleep_duration(),
        std::time::Duration::from_millis(2)
    );
    // Explicit settings win.
    let client = driver.connect(
        Context::new()
            .client_name("explicit")
            .driver_timeout(std::time::Duration::from_secs(3)),
    );
    assert_eq!(client.client_name(), "explicit");
    assert_eq!(client.driver_timeout(), std::time::Duration::from_secs(3));
}

#[test]
fn unparseable_environment_variables_fail_connect() {
    if !common::in_child_process(
        "unparseable_environment_variables_fail_connect",
        &[("AERON_DRIVER_TIMEOUT", "not-a-duration")],
    ) {
        return;
    }
    let driver = TestDriver::start();
    let err = aeron_glide::AeronClient::connect(Context::new().aeron_dir(&driver.dir)).unwrap_err();
    assert_eq!(err.kind(), ErrorKind::IllegalArgument, "{err}");
}
