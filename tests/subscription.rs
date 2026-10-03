#![cfg(feature = "driver")]
mod common;

use aeron_glide::{ChannelStatus, ControlledAction};
use common::{TestDriver, offer, wait_connected, wait_until};

#[test]
fn subscription_accessors() {
    let driver = TestDriver::start();
    let client = driver.client();
    let sub = client.add_subscription("aeron:ipc", 5).unwrap();
    assert_eq!(sub.channel(), "aeron:ipc");
    assert_eq!(sub.stream_id(), 5);
    assert!(sub.registration_id() > 0);
    assert!(!sub.is_closed());
    assert!(!sub.is_connected());
    assert_eq!(sub.image_count(), 0);
    assert!(sub.images().is_empty());
    assert_eq!(sub.channel_status().unwrap(), ChannelStatus::NoStatus);
    assert!(sub.local_socket_addresses().unwrap().is_empty());
}

#[test]
fn wildcard_port_is_resolved() {
    let driver = TestDriver::start();
    let client = driver.client();
    let sub = client
        .add_subscription("aeron:udp?endpoint=127.0.0.1:0", 6)
        .unwrap();
    wait_until("the channel to become active", || {
        sub.channel_status().unwrap() == ChannelStatus::Active
    });
    assert!(sub.channel_status_id() >= 0);
    let endpoint = sub.resolved_endpoint().unwrap().expect("bound endpoint");
    let port: u16 = endpoint.rsplit(':').next().unwrap().parse().unwrap();
    assert_ne!(port, 0, "{endpoint}");
    let channel = sub
        .try_resolve_channel_endpoint_port()
        .unwrap()
        .expect("resolved channel");
    assert!(channel.contains(&format!(":{port}")), "{channel}");
    assert_eq!(sub.local_socket_addresses().unwrap(), [endpoint]);

    // Publishing to the resolved channel reaches the subscription.
    let publication = client.add_publication(&channel, 6).unwrap();
    wait_connected(&sub);
    drop(publication);
}

#[test]
fn controlled_poll_break_and_abort() {
    let driver = TestDriver::start();
    let client = driver.client();
    let publication = client.add_publication("aeron:ipc", 7).unwrap();
    let mut sub = client.add_subscription("aeron:ipc", 7).unwrap();
    wait_connected(&sub);
    for message in [b"one", b"two", b"six"] {
        offer(&publication, message);
    }
    wait_until("the image", || sub.image_count() == 1);

    // Break after the first fragment: only one is delivered.
    let mut seen = Vec::new();
    sub.controlled_poll(10, |data, _| {
        seen.push(data.to_vec());
        ControlledAction::Break
    })
    .unwrap();
    assert_eq!(seen, [b"one".to_vec()]);

    // Abort the next one: it is delivered again by the following poll.
    let mut aborted = None;
    sub.controlled_poll(10, |data, _| {
        aborted = Some(data.to_vec());
        ControlledAction::Abort
    })
    .unwrap();
    wait_until("the remaining two", || {
        sub.controlled_poll(10, |data, _| seen.push(data.to_vec()))
            .unwrap();
        seen.len() == 3
    });
    assert_eq!(aborted, Some(b"two".to_vec()));
    assert_eq!(seen, [b"one".to_vec(), b"two".to_vec(), b"six".to_vec()]);
}

#[test]
fn block_poll_delivers_frames() {
    let driver = TestDriver::start();
    let client = driver.client();
    let publication = client.add_publication("aeron:ipc", 8).unwrap();
    let mut sub = client.add_subscription("aeron:ipc", 8).unwrap();
    wait_connected(&sub);
    offer(&publication, b"block");

    let mut blocks = Vec::new();
    wait_until("a block", || {
        sub.block_poll(64 * 1024, |block, session_id, term_id| {
            blocks.push((block.len(), session_id, term_id));
        })
        .unwrap();
        !blocks.is_empty()
    });
    let (len, session_id, term_id) = blocks[0];
    // One frame: a 32-byte header plus "block", aligned to 32 bytes.
    assert_eq!(len, 64);
    assert_eq!(session_id, publication.session_id());
    assert_eq!(term_id, publication.initial_term_id());
}

#[test]
fn images_snapshot_and_for_each() {
    let driver = TestDriver::start();
    let client = driver.client();
    let first = client.add_exclusive_publication("aeron:ipc", 9).unwrap();
    let second = client.add_exclusive_publication("aeron:ipc", 9).unwrap();
    let sub = client.add_subscription("aeron:ipc", 9).unwrap();
    wait_until("two images", || sub.image_count() == 2);

    let mut sessions: Vec<i32> = sub.images().iter().map(|i| i.session_id()).collect();
    sessions.sort();
    let mut expected = vec![first.session_id(), second.session_id()];
    expected.sort();
    assert_eq!(sessions, expected);

    let mut visited = Vec::new();
    assert_eq!(sub.for_each_image(|i| visited.push(i.session_id())), 2);
    visited.sort();
    assert_eq!(visited, expected);
}
