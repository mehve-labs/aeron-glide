#![cfg(feature = "driver")]
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

#[test]
fn vectored_offer_publishes_one_message() {
    let driver = TestDriver::start();
    let client = driver.client();
    let publication = client.add_publication("aeron:ipc", 12).unwrap();
    let mut exclusive = client.add_exclusive_publication("aeron:ipc", 12).unwrap();
    let mut sub = client.add_subscription("aeron:ipc", 12).unwrap();
    wait_connected(&sub);
    wait_until("exclusive to connect", || exclusive.is_connected());

    while let Err(e) = publication.offer_vectored(&[b"he", b"ll", b"o"]) {
        assert!(e.is_retryable(), "{e}");
    }
    // More parts than fit on the stack.
    let bytes: Vec<[u8; 1]> = (0..20u8).map(|i| [i]).collect();
    let parts: Vec<&[u8]> = bytes.iter().map(|b| &b[..]).collect();
    while let Err(e) = exclusive.offer_vectored(&parts) {
        assert!(e.is_retryable(), "{e}");
    }

    let mut received = Vec::new();
    common::poll_n(&mut sub, 2, |data| received.push(data.to_vec()));
    received.sort();
    let twenty: Vec<u8> = (0..20).collect();
    assert!(received.contains(&b"hello".to_vec()), "{received:?}");
    assert!(received.contains(&twenty), "{received:?}");
}

#[test]
fn reserved_value_supplier_sees_every_frame() {
    let driver = TestDriver::start();
    let client = driver.client();
    let publication = client.add_publication("aeron:ipc", 13).unwrap();
    let sub = client.add_subscription("aeron:ipc", 13).unwrap();
    wait_connected(&sub);

    // A message spanning several fragments: the supplier runs once per fragment.
    let message = vec![1u8; 4 * publication.max_payload_length()];
    let mut frames = Vec::new();
    loop {
        frames.clear();
        match publication.offer_with_reserved_value(&message, |frame| {
            frames.push(frame.len());
            frame.len() as i64
        }) {
            Ok(_) => break,
            Err(e) => assert!(e.is_retryable(), "{e}"),
        }
    }
    const HEADER: usize = 32;
    assert_eq!(frames.len(), 4, "{frames:?}");
    assert_eq!(frames.iter().sum::<usize>(), message.len() + 4 * HEADER);
}

#[test]
fn reserved_value_supplier_panic_unwinds() {
    let driver = TestDriver::start();
    let client = driver.client();
    let publication = client.add_publication("aeron:ipc", 14).unwrap();
    let sub = client.add_subscription("aeron:ipc", 14).unwrap();
    wait_connected(&sub);

    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _ = publication
            .offer_vectored_with_reserved_value(&[b"a", b"b"], |_| panic!("supplier panic"));
    }));
    let payload = result.expect_err("the supplier panic propagates");
    assert_eq!(payload.downcast_ref::<&str>(), Some(&"supplier panic"));
    offer(&publication, b"after");
}

#[test]
fn buffer_claim_commit_abort_and_header_fields() {
    let driver = TestDriver::start();
    let client = driver.client();
    let publication = client.add_publication("aeron:ipc", 15).unwrap();
    let mut exclusive = client.add_exclusive_publication("aeron:ipc", 15).unwrap();
    let mut sub = client.add_subscription("aeron:ipc", 15).unwrap();
    wait_connected(&sub);
    wait_until("exclusive to connect", || exclusive.is_connected());

    fn claim_retrying(p: &aeron_glide::Publication, n: usize) -> aeron_glide::BufferClaim<'_> {
        loop {
            match p.try_claim(n) {
                Ok(claim) => return claim,
                Err(e) => assert!(e.is_retryable(), "{e}"),
            }
        }
    }

    // Dropped without commit: aborted, subscribers skip it.
    let mut dropped = claim_retrying(&publication, 4);
    dropped.buffer_mut().copy_from_slice(b"drop");
    drop(dropped);

    // Explicit abort.
    let mut aborted = claim_retrying(&publication, 5);
    aborted.buffer_mut().copy_from_slice(b"abort");
    aborted.abort();

    // Committed, with header fields set.
    let mut claim = claim_retrying(&publication, 6);
    assert_eq!(claim.len(), 6);
    claim.buffer_mut().copy_from_slice(b"commit");
    claim.set_reserved_value(42);
    assert_eq!(claim.reserved_value(), 42);
    let position = claim.position();
    assert_eq!(claim.commit(), position);

    let mut exclusive_claim = loop {
        match exclusive.try_claim(9) {
            Ok(claim) => break claim,
            Err(e) => assert!(e.is_retryable(), "{e}"),
        }
    };
    exclusive_claim.buffer_mut().copy_from_slice(b"exclusive");
    exclusive_claim.commit();

    let mut received = Vec::new();
    common::poll_n(&mut sub, 2, |data| received.push(data.to_vec()));
    received.sort();
    assert_eq!(received, [b"commit".to_vec(), b"exclusive".to_vec()]);
}

#[test]
fn exclusive_and_ipc_channel_status_and_addresses() {
    let driver = TestDriver::start();
    let client = driver.client();
    // IPC: no channel status counter and no socket.
    let ipc = client.add_exclusive_publication("aeron:ipc", 20).unwrap();
    assert_eq!(ipc.channel_status().unwrap(), ChannelStatus::NoStatus);
    assert!(ipc.local_socket_addresses().unwrap().is_empty());
    let shared = client.add_publication("aeron:ipc", 20).unwrap();
    assert_eq!(shared.channel_status().unwrap(), ChannelStatus::NoStatus);
    assert!(shared.local_socket_addresses().unwrap().is_empty());
    assert!(ipc.is_original());

    // UDP exclusive publication: links and reports its status and address.
    let channel = format!("aeron:udp?endpoint=127.0.0.1:{}", free_udp_port());
    let udp = client.add_exclusive_publication(&channel, 21).unwrap();
    wait_until("the channel to become active", || {
        udp.channel_status().unwrap() == ChannelStatus::Active
    });
    assert_eq!(udp.local_socket_addresses().unwrap().len(), 1);
}

#[test]
fn buffer_claim_flags_and_header_type() {
    let driver = TestDriver::start();
    let client = driver.client();
    let publication = client.add_publication("aeron:ipc", 22).unwrap();
    let sub = client.add_subscription("aeron:ipc", 22).unwrap();
    wait_connected(&sub);
    let mut claim = loop {
        match publication.try_claim(4) {
            Ok(claim) => break claim,
            Err(e) => assert!(e.is_retryable(), "{e}"),
        }
    };
    // Unfragmented data frame defaults.
    assert_eq!((claim.flags(), claim.header_type()), (0xC0, 1));
    claim.set_flags(0xC1).set_header_type(1);
    assert_eq!((claim.flags(), claim.header_type()), (0xC1, 1));
    claim.buffer_mut().copy_from_slice(b"flag");
    claim.commit();
}
