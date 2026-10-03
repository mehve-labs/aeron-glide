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

/// A data frame as Aeron writes one (`aeron_data_header_t`, unfragmented),
/// padded to the 32-byte frame alignment.
fn data_frame(
    term_offset: i32,
    session_id: i32,
    stream_id: i32,
    term_id: i32,
    payload: &[u8],
) -> Vec<u8> {
    let mut frame = Vec::new();
    frame.extend_from_slice(&(32 + payload.len() as i32).to_le_bytes()); // frame length
    frame.extend_from_slice(&[0, 0xC0]); // version, flags: begin and end
    frame.extend_from_slice(&1i16.to_le_bytes()); // type: data
    for value in [term_offset, session_id, stream_id, term_id] {
        frame.extend_from_slice(&value.to_le_bytes());
    }
    frame.extend_from_slice(&0i64.to_le_bytes()); // reserved value
    frame.extend_from_slice(payload);
    frame.resize(frame.len().next_multiple_of(32), 0);
    frame
}

/// Frames for `messages`, starting at `publication`'s current position.
fn block_at_position(
    publication: &aeron_glide::ExclusivePublication,
    messages: &[&[u8]],
) -> Vec<u8> {
    let position = publication.position().unwrap();
    let term_length = publication.term_buffer_length() as i64;
    let term_id = publication.initial_term_id() + (position >> term_length.trailing_zeros()) as i32;
    let mut term_offset = (position & (term_length - 1)) as i32;
    let mut block = Vec::new();
    for message in messages {
        let frame = data_frame(
            term_offset,
            publication.session_id(),
            publication.stream_id(),
            term_id,
            message,
        );
        term_offset += frame.len() as i32;
        block.extend(frame);
    }
    block
}

fn rejected(result: Result<i64, aeron_glide::OfferError>) -> aeron_glide::ErrorKind {
    match result {
        Err(aeron_glide::OfferError::Error(e)) => e.kind(),
        other => panic!("expected an error, got {other:?}"),
    }
}

#[test]
fn offer_block_publishes_preformatted_frames() {
    use aeron_glide::ErrorKind;
    let driver = TestDriver::start();
    let client = driver.client();
    let mut sub = client.add_subscription("aeron:ipc", 30).unwrap();
    let mut publication = client.add_exclusive_publication("aeron:ipc", 30).unwrap();
    wait_connected(&sub);

    let block = block_at_position(&publication, &[b"one", b"two", b"three"]);
    let before = publication.position().unwrap();
    assert_eq!(
        publication.offer_block(&block).unwrap(),
        before + block.len() as i64
    );
    let mut received = Vec::new();
    common::poll_n(&mut sub, 3, |data| received.push(data.to_vec()));
    assert_eq!(
        received,
        [b"one".to_vec(), b"two".to_vec(), b"three".to_vec()]
    );

    // Malformed blocks are rejected before they reach the log.
    let good = block_at_position(&publication, &[b"four", b"five"]);
    let second = 64; // each frame above is 64 bytes
    let mut corrupt = Vec::new();
    corrupt.push(good[..16].to_vec()); // shorter than a header
    corrupt.push(good[..good.len() - 8].to_vec()); // not frame aligned
    let mut b = good.clone();
    b[second..second + 4].copy_from_slice(&0i32.to_le_bytes()); // zero-length frame
    corrupt.push(b);
    let mut b = good.clone();
    b[second..second + 4].copy_from_slice(&4096i32.to_le_bytes()); // runs past the block
    corrupt.push(b);
    let mut b = good.clone();
    b[second + 12..second + 16].copy_from_slice(&12345i32.to_le_bytes()); // other session
    corrupt.push(b);
    let mut b = good.clone();
    b[second + 8..second + 12].copy_from_slice(&0i32.to_le_bytes()); // wrong term offset
    corrupt.push(b);
    let mut b = good.clone();
    b[second + 6..second + 8].copy_from_slice(&5i16.to_le_bytes()); // not data or padding
    corrupt.push(b);
    for block in corrupt {
        assert_eq!(
            rejected(publication.offer_block(&block)),
            ErrorKind::IllegalArgument
        );
    }
    // A well-formed block that doesn't start at the position: Aeron rejects it.
    let mut stale = good.clone();
    let shifted = 64i32.to_le_bytes();
    for frame in [0, second] {
        let offset = i32::from_le_bytes(stale[frame + 8..frame + 12].try_into().unwrap());
        stale[frame + 8..frame + 12]
            .copy_from_slice(&(offset + i32::from_le_bytes(shifted)).to_le_bytes());
    }
    assert!(matches!(
        publication.offer_block(&stale),
        Err(aeron_glide::OfferError::Error(_))
    ));

    // The stream is intact.
    assert_eq!(publication.position().unwrap(), before + block.len() as i64);
    publication.offer_block(&good).unwrap();
    let mut received = Vec::new();
    common::poll_n(&mut sub, 2, |data| received.push(data.to_vec()));
    assert_eq!(received, [b"four".to_vec(), b"five".to_vec()]);
}

#[test]
fn append_padding_advances_the_position_without_messages() {
    let driver = TestDriver::start();
    let client = driver.client();
    let mut sub = client.add_subscription("aeron:ipc", 31).unwrap();
    let mut publication = client.add_exclusive_publication("aeron:ipc", 31).unwrap();
    wait_connected(&sub);

    let first = loop {
        if let Ok(position) = publication.offer(b"a") {
            break position;
        }
    };
    let padded = publication.append_padding(1000).unwrap();
    assert!(padded >= first + 1000, "{first} -> {padded}");
    publication.offer(b"b").unwrap();
    let mut received = Vec::new();
    common::poll_n(&mut sub, 2, |data| received.push(data.to_vec()));
    assert_eq!(received, [b"a".to_vec(), b"b".to_vec()]);

    let too_long = publication.max_message_length() + 1;
    assert_eq!(
        rejected(publication.append_padding(too_long)),
        aeron_glide::ErrorKind::IllegalArgument
    );
}
