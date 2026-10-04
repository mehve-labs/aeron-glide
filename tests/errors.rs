#![cfg(feature = "driver")]
mod common;

use aeron_glide::{DriverIdleStrategy, ErrorKind, MediaDriver, OfferError, ThreadingMode};
use common::{TestDriver, wait_connected};

fn assert_illegal_argument(result: Result<i64, OfferError>) {
    match result {
        Err(OfferError::Error(e)) => assert_eq!(e.kind(), ErrorKind::IllegalArgument, "{e}"),
        other => panic!("expected IllegalArgument, got {other:?}"),
    }
}

#[test]
fn oversized_offer_and_claim_are_errors() {
    let driver = TestDriver::start();
    let client = driver.client();
    let publication = client.add_publication("aeron:ipc", 1).unwrap();
    let sub = client.add_subscription("aeron:ipc", 1).unwrap();
    wait_connected(&sub);

    // Longer than the maximum message length (term length / 8).
    assert_illegal_argument(publication.offer(&vec![0u8; 512 * 1024]));
    // Longer than the maximum payload (MTU - header).
    assert_illegal_argument(publication.try_claim(64 * 1024).map(|c| c.commit()));
    // Beyond int32: rejected rather than truncated.
    assert_illegal_argument(publication.try_claim((1 << 32) + 16).map(|c| c.commit()));
}

#[test]
fn offer_without_subscriber_is_retryable() {
    let driver = TestDriver::start();
    let client = driver.client();
    let publication = client.add_publication("aeron:ipc", 1).unwrap();
    let err = publication.offer(b"nobody listens").unwrap_err();
    assert_eq!(err, OfferError::NotConnected);
    assert!(err.is_retryable());
}

#[test]
fn driver_errors_are_classified() {
    let err = MediaDriver::builder()
        .dir("/dev/null/aeron-glide")
        .start()
        .expect_err("start fails");
    assert_ne!(err.kind(), ErrorKind::Other, "{err}");
    assert_ne!(err.code(), 0, "{err}");
    assert!(err.message().starts_with("Failed to init driver"), "{err}");
}

#[test]
fn invalid_driver_setting_is_reported_by_start() {
    let err = MediaDriver::builder()
        .term_buffer_length(1000)
        .start()
        .expect_err("a term length that is not a power of two is rejected");
    assert_ne!(err.kind(), ErrorKind::Other, "{err}");
}

#[test]
fn builder_keeps_the_first_setter_error() {
    let err = MediaDriver::builder()
        .sender_idle_strategy(DriverIdleStrategy::Sleeping)
        .sender_idle_strategy_init_args("not-a-duration")
        .term_buffer_length(1 << 20)
        .threading_mode(ThreadingMode::Invoker)
        .start()
        .expect_err("invalid init args are rejected");
    // The first failing setter wins, not the later Invoker check.
    assert_eq!(err.kind(), ErrorKind::IllegalArgument, "{err}");
    assert!(err.message().contains("sender_idle_strategy"), "{err}");
}

#[test]
fn error_display_and_timeouts() {
    // Aeron's message already carries the code: it isn't repeated.
    let driver = common::TestDriver::start();
    let err = driver
        .client()
        .add_publication("aeron:udp?endpoint=not-a-host-name.invalid:1", 1)
        .unwrap_err();
    assert_eq!(err.kind(), ErrorKind::Registration);
    let shown = err.to_string();
    assert!(
        shown.starts_with("Registration: (-9) unknown host"),
        "{shown}"
    );
    assert!(!shown.contains("(code -9)"), "{shown}");
    assert!(!err.is_timeout());
    // Timeouts of every kind.
    assert!(aeron_glide::Error::new(ErrorKind::DriverTimeout, "x").is_timeout());
    assert!(aeron_glide::Error::new(ErrorKind::Timeout, "x").is_timeout());
}
