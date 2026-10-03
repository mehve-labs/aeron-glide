mod common;

use aeron_glide::ControlledAction;
use common::{TestDriver, free_udp_port, offer, wait_connected, wait_until};

#[test]
fn image_accessors_and_revocation() {
    let driver = TestDriver::start();
    let client = driver.client();
    let mut publication = client.add_exclusive_publication("aeron:ipc", 1).unwrap();
    let sub = client.add_subscription("aeron:ipc", 1).unwrap();
    wait_connected(&sub);
    while let Err(e) = publication.offer(b"x") {
        assert!(e.is_retryable(), "{e}");
    }

    let image = sub.image_by_index(0).unwrap();
    assert_eq!(image.initial_term_id(), publication.initial_term_id());
    assert_eq!(image.term_buffer_length(), 1 << 20);
    assert_eq!(
        image.position_bits_to_shift(),
        publication.position_bits_to_shift()
    );
    assert!(image.subscriber_position_id() >= 0);
    assert_eq!(image.subscription_registration_id(), sub.registration_id());
    assert_eq!(
        image.active_transport_count().unwrap(),
        0,
        "IPC has no transport"
    );
    assert!(!image.is_publication_revoked());

    publication.revoke().unwrap();
    wait_until("the image to close", || image.is_closed());
    assert!(image.is_publication_revoked());
}

#[test]
fn bounded_and_controlled_polls() {
    let driver = TestDriver::start();
    let client = driver.client();
    let publication = client.add_publication("aeron:ipc", 2).unwrap();
    let sub = client.add_subscription("aeron:ipc", 2).unwrap();
    wait_connected(&sub);
    let first = offer(&publication, b"one");
    let second = offer(&publication, b"two");
    offer(&publication, b"six");

    let mut image = sub.image_by_index(0).unwrap();
    let mut seen = Vec::new();
    // Stops at the first message's end position.
    wait_until("the first message", || {
        image
            .bounded_poll(first, 10, |data, _| seen.push(data.to_vec()))
            .unwrap();
        !seen.is_empty()
    });
    assert_eq!(seen, [b"one".to_vec()]);

    // Raw controlled poll: abort re-delivers.
    let mut aborted = Vec::new();
    image
        .controlled_poll(10, |data, _| {
            aborted.push(data.to_vec());
            ControlledAction::Abort
        })
        .unwrap();
    assert_eq!(aborted, [b"two".to_vec()]);

    // Bounded controlled poll up to the second message.
    image
        .bounded_controlled_poll(second, 10, |data, _| seen.push(data.to_vec()))
        .unwrap();
    assert_eq!(seen, [b"one".to_vec(), b"two".to_vec()]);

    // Block poll returns the rest as raw frames.
    let mut bytes = 0;
    wait_until("the last frame", || {
        bytes += image
            .block_poll(64 * 1024, |block, session_id, _| {
                assert_eq!(session_id, publication.session_id());
                assert!(!block.is_empty());
            })
            .unwrap();
        bytes > 0
    });
    assert_eq!(bytes, 64, "one 32-byte header + 3 bytes, aligned to 32");
}

#[test]
fn reject_a_udp_publisher() {
    let driver = TestDriver::start();
    let client = driver.client();
    let channel = format!("aeron:udp?endpoint=127.0.0.1:{}", free_udp_port());
    let sub = client.add_subscription(&channel, 3).unwrap();
    let publication = client.add_publication(&channel, 3).unwrap();
    wait_connected(&sub);
    offer(&publication, b"hello");

    let image = sub.image_by_index(0).unwrap();
    wait_until("the transport count", || {
        image.active_transport_count().unwrap() == 1
    });
    image.reject("rejected by the test").unwrap();
    wait_until("the publication to disconnect", || {
        !publication.is_connected()
    });
}

#[test]
fn nested_poll_of_the_same_image_is_rejected() {
    use aeron_glide::ErrorKind;
    let driver = TestDriver::start();
    let client = driver.client();
    let mut first = client.add_exclusive_publication("aeron:ipc", 4).unwrap();
    let mut second = client.add_exclusive_publication("aeron:ipc", 4).unwrap();
    let sub = client.add_subscription("aeron:ipc", 4).unwrap();
    wait_until("two images", || sub.image_count() == 2);
    wait_until("both connected", || {
        first.is_connected() && second.is_connected()
    });
    wait_until("the first offer", || first.offer(b"a").is_ok());
    wait_until("the second offer", || second.offer(b"b").is_ok());

    let mut outer = sub.image_by_session_id(first.session_id()).unwrap();
    let mut same = sub.image_by_session_id(first.session_id()).unwrap();
    let mut other = sub.image_by_session_id(second.session_id()).unwrap();
    let (mut nested_same, mut nested_other) = (None, None);
    wait_until("the outer fragment", || {
        outer
            .poll(10, |_, _| {
                nested_same = Some(same.poll(10, |_, _| {}));
                nested_other = Some(other.poll(10, |_, _| {}));
            })
            .unwrap();
        nested_same.is_some()
    });
    assert_eq!(
        nested_same.unwrap().unwrap_err().kind(),
        ErrorKind::Reentrant
    );
    // Another session's image may be polled from the handler.
    assert!(nested_other.unwrap().is_ok());
    // The guard is released afterwards.
    assert!(same.poll(10, |_, _| {}).is_ok());
}

#[test]
fn bounded_poll_assembled_stops_before_the_limit() {
    let driver = TestDriver::start();
    let client = driver.client();
    let publication = client.add_publication("aeron:ipc", 5).unwrap();
    let sub = client.add_subscription("aeron:ipc", 5).unwrap();
    wait_connected(&sub);
    let large = vec![9u8; 2 * publication.max_payload_length()];
    let end_of_first = offer(&publication, &large);
    offer(&publication, b"second");

    let mut image = sub.image_by_index(0).unwrap();
    let mut seen = Vec::new();
    wait_until("the first message", || {
        image
            .bounded_poll_assembled(end_of_first, 10, |data: &[u8], _: &aeron_glide::Header| {
                seen.push(data.len())
            })
            .unwrap();
        !seen.is_empty()
    });
    for _ in 0..10 {
        image
            .bounded_poll_assembled(end_of_first, 10, |data: &[u8], _: &aeron_glide::Header| {
                seen.push(data.len())
            })
            .unwrap();
    }
    assert_eq!(seen, [large.len()]);
}
