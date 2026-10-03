#![cfg(feature = "driver")]
mod common;

use aeron_glide::{ErrorKind, Header};
use common::{TestDriver, offer, wait_connected, wait_until};

fn three_fragment_message(publication: &aeron_glide::Publication) -> Vec<u8> {
    (0..3 * publication.max_payload_length())
        .map(|i| i as u8)
        .collect()
}

#[test]
fn message_split_across_image_handles() {
    let driver = TestDriver::start();
    let client = driver.client();
    let publication = client.add_publication("aeron:ipc", 1).unwrap();
    let sub = client.add_subscription("aeron:ipc", 1).unwrap();
    wait_connected(&sub);
    let message = three_fragment_message(&publication);
    offer(&publication, &message);

    // One fragment through the first handle: buffered, nothing delivered yet.
    let mut first = sub.image_by_index(0).unwrap();
    let mut delivered = Vec::new();
    wait_until("the first fragment", || {
        first
            .poll_assembled(1, |data: &[u8], _: &Header| delivered.push(data.to_vec()))
            .unwrap()
            == 1
    });
    assert!(delivered.is_empty());
    drop(first);

    // A new handle completes the message.
    let mut second = sub.image_by_index(0).unwrap();
    wait_until("the rest of the message", || {
        second
            .poll_assembled(10, |data: &[u8], _: &Header| delivered.push(data.to_vec()))
            .unwrap();
        !delivered.is_empty()
    });
    assert_eq!(delivered, [message]);
}

#[test]
fn message_split_between_subscription_and_image() {
    let driver = TestDriver::start();
    let client = driver.client();
    let publication = client.add_publication("aeron:ipc", 2).unwrap();
    let mut sub = client.add_subscription("aeron:ipc", 2).unwrap();
    wait_connected(&sub);
    let message = three_fragment_message(&publication);
    offer(&publication, &message);

    let mut delivered = Vec::new();
    wait_until("the first fragment", || {
        sub.poll_assembled(1, |data: &[u8], _: &Header| delivered.push(data.to_vec()))
            .unwrap()
            == 1
    });
    assert!(delivered.is_empty());
    let mut image = sub.image_by_index(0).unwrap();
    wait_until("the rest of the message", || {
        image
            .poll_assembled(10, |data: &[u8], _: &Header| delivered.push(data.to_vec()))
            .unwrap();
        !delivered.is_empty()
    });
    assert_eq!(delivered, [message]);
}

#[test]
fn nested_assembled_polls_on_one_subscription_are_rejected() {
    let driver = TestDriver::start();
    let client = driver.client();
    let publication = client.add_publication("aeron:ipc", 3).unwrap();
    let sub = client.add_subscription("aeron:ipc", 3).unwrap();
    wait_connected(&sub);
    offer(&publication, b"outer");

    let mut outer = sub.image_by_index(0).unwrap();
    let mut inner = sub.image_by_index(0).unwrap();
    let mut nested = None;
    wait_until("the outer message", || {
        outer
            .poll_assembled(10, |_: &[u8], _: &Header| {
                nested = Some(inner.poll_assembled(10, |_: &[u8], _: &Header| {}));
            })
            .unwrap();
        nested.is_some()
    });
    let err = nested.unwrap().unwrap_err();
    assert_eq!(err.kind(), ErrorKind::Reentrant, "{err}");
}
