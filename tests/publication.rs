mod common;

use aeron_glide::ChannelStatus;
use common::{TestDriver, free_udp_port, offer, wait_connected, wait_until};

#[test]
fn publication_accessors() {
    let driver = TestDriver::start();
    let client = driver.client();
    let publication = client.add_publication("aeron:ipc", 7).unwrap();
    let sub = client.add_subscription("aeron:ipc", 7).unwrap();
    wait_connected(&sub);

    assert_eq!(publication.channel(), "aeron:ipc");
    assert_eq!(publication.stream_id(), 7);
    assert!(publication.registration_id() > 0);
    assert_eq!(
        publication.original_registration_id(),
        publication.registration_id()
    );
    assert!(publication.is_original());
    // The harness uses 1 MiB IPC terms; the max message is a term / 8.
    assert_eq!(publication.term_buffer_length(), 1 << 20);
    assert_eq!(publication.max_message_length(), (1 << 20) / 8);
    assert!(publication.max_payload_length() > 0);
    assert!(publication.max_payload_length() < publication.max_message_length());
    assert!(publication.position_bits_to_shift() > 0);
    assert!(publication.max_possible_position() > 0);
    assert!(publication.is_connected());
    assert!(!publication.is_closed());

    let before = publication.position().unwrap();
    let after = offer(&publication, b"hello");
    assert!(after > before);
    wait_until("position to advance", || {
        publication.position().unwrap() >= after
    });
    assert!(publication.publication_limit().unwrap() >= publication.position().unwrap());
    assert!(publication.available_window().unwrap() >= 0);
    assert!(publication.publication_limit_id() >= 0);

    // A second publication on the same stream attaches to the same log.
    let second = client.add_publication("aeron:ipc", 7).unwrap();
    assert_eq!(second.session_id(), publication.session_id());
    assert_ne!(second.registration_id(), publication.registration_id());
    assert_eq!(
        second.original_registration_id(),
        publication.registration_id()
    );
    assert!(!second.is_original());
}

#[test]
fn exclusive_publication_accessors() {
    let driver = TestDriver::start();
    let client = driver.client();
    let first = client.add_exclusive_publication("aeron:ipc", 8).unwrap();
    let second = client.add_exclusive_publication("aeron:ipc", 8).unwrap();
    // Exclusive publications never share a log.
    assert_ne!(first.session_id(), second.session_id());
    assert_eq!(first.channel(), "aeron:ipc");
    assert_eq!(first.stream_id(), 8);
    assert_eq!(first.original_registration_id(), first.registration_id());
    assert_eq!(first.term_buffer_length(), 1 << 20);
    assert!(!first.is_connected());
    assert!(!first.is_closed());
    assert_eq!(first.position().unwrap(), 0);
}

#[test]
fn udp_channel_status_and_local_address() {
    let driver = TestDriver::start();
    let client = driver.client();
    let channel = format!("aeron:udp?endpoint=127.0.0.1:{}", free_udp_port());
    let publication = client.add_publication(&channel, 9).unwrap();
    wait_until("the channel to become active", || {
        publication.channel_status().unwrap() == ChannelStatus::Active
    });
    assert!(publication.channel_status_id() >= 0);
    let addresses = publication.local_socket_addresses().unwrap();
    assert_eq!(addresses.len(), 1, "{addresses:?}");
    assert!(addresses[0].contains(':'), "{addresses:?}");
}

#[test]
fn revoke_ends_the_stream_for_subscribers() {
    let driver = TestDriver::start();
    let client = driver.client();
    let mut publication = client.add_exclusive_publication("aeron:ipc", 10).unwrap();
    let sub = client.add_subscription("aeron:ipc", 10).unwrap();
    wait_connected(&sub);
    while let Err(e) = publication.offer(b"before") {
        assert!(e.is_retryable(), "{e}");
    }

    // A normal close would wait for the subscriber to drain the unread message.
    publication.revoke().unwrap();
    wait_until("the image to go away", || sub.image_count() == 0);
}

#[test]
fn revoke_on_close_ends_the_stream_when_dropped() {
    let driver = TestDriver::start();
    let client = driver.client();
    let mut publication = client.add_exclusive_publication("aeron:ipc", 11).unwrap();
    let sub = client.add_subscription("aeron:ipc", 11).unwrap();
    wait_connected(&sub);

    while let Err(e) = publication.offer(b"unread") {
        assert!(e.is_retryable(), "{e}");
    }
    // A normal close would wait for the subscriber to drain the unread message.
    publication.revoke_on_close();
    drop(publication);
    wait_until("the image to go away", || sub.image_count() == 0);
}
