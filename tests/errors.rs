mod common;

use aeron_glide::{ErrorKind, MediaDriver, OfferError};
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
    assert_illegal_argument(publication.try_claim(64 * 1024, |_| true));
    // Beyond int32: rejected rather than truncated.
    assert_illegal_argument(
        publication.try_claim((1 << 32) + 16, |_| panic!("must not be called")),
    );
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
        .err()
        .expect("start fails");
    assert_ne!(err.kind(), ErrorKind::Other, "{err}");
    assert_ne!(err.code(), 0, "{err}");
    assert!(err.message().starts_with("Failed to init driver"), "{err}");
}

#[test]
fn invalid_driver_setting_is_reported_by_start() {
    let err = MediaDriver::builder()
        .term_buffer_length(1000)
        .start()
        .err()
        .expect("a term length that is not a power of two is rejected");
    assert_ne!(err.kind(), ErrorKind::Other, "{err}");
}
