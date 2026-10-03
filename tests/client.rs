mod common;

use aeron_glide::ErrorKind;
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
    let err = result.err().expect("the driver rejects the channel");
    assert_ne!(err.code(), 0, "{err}");
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
