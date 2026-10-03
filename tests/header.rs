mod common;

use common::{TestDriver, wait_connected, wait_until};

const HEADER_LENGTH: i32 = 32;
const HDR_TYPE_DATA: u16 = 1;
const BEGIN_AND_END: u8 = 0xC0;

#[test]
fn header_describes_each_fragment() {
    let driver = TestDriver::start();
    let client = driver.client();
    let publication = client.add_publication("aeron:ipc", 1).unwrap();
    let mut sub = client.add_subscription("aeron:ipc", 1).unwrap();
    wait_connected(&sub);

    let position = loop {
        match publication.offer_with_reserved_value(b"hello", |_| 7) {
            Ok(position) => break position,
            Err(e) => assert!(e.is_retryable(), "{e}"),
        }
    };
    let mut claim = loop {
        match publication.try_claim(3) {
            Ok(claim) => break claim,
            Err(e) => assert!(e.is_retryable(), "{e}"),
        }
    };
    claim.buffer_mut().copy_from_slice(b"abc");
    claim.set_reserved_value(9);
    let claim_position = claim.commit();

    let mut seen = Vec::new();
    wait_until("both fragments", || {
        sub.poll(10, |data, header| {
            seen.push((
                data.to_vec(),
                header.session_id(),
                header.stream_id(),
                header.position(),
                header.frame_length(),
                header.header_type(),
                header.flags(),
                header.reserved_value(),
                header.initial_term_id(),
                header.term_id(),
                header.term_offset(),
                header.position_bits_to_shift(),
            ));
        })
        .unwrap();
        seen.len() == 2
    });

    let session = publication.session_id();
    let initial = publication.initial_term_id();
    let shift = publication.position_bits_to_shift();
    let (data, sid, stream, pos, len, ty, flags, reserved, init, term, offset, bits) = &seen[0];
    assert_eq!(
        (&data[..], *sid, *stream, *pos, *len, *ty, *flags, *reserved),
        (
            &b"hello"[..],
            session,
            1,
            position,
            5 + HEADER_LENGTH,
            HDR_TYPE_DATA,
            BEGIN_AND_END,
            7
        )
    );
    assert_eq!((*init, *term, *offset, *bits), (initial, initial, 0, shift));
    let (data, _, _, pos, len, _, _, reserved, ..) = &seen[1];
    assert_eq!(
        (&data[..], *pos, *len, *reserved),
        (&b"abc"[..], claim_position, 3 + HEADER_LENGTH, 9)
    );
}

#[test]
fn assembled_header_ends_at_the_message_position() {
    let driver = TestDriver::start();
    let client = driver.client();
    let publication = client.add_publication("aeron:ipc", 2).unwrap();
    let mut sub = client.add_subscription("aeron:ipc", 2).unwrap();
    wait_connected(&sub);

    let message = vec![3u8; 3 * publication.max_payload_length()];
    let position = common::offer(&publication, &message);
    let mut seen = None;
    wait_until("the reassembled message", || {
        sub.poll_assembled(10, |data: &[u8], header: &aeron_glide::Header| {
            seen = Some((data.len(), header.session_id(), header.position()));
        })
        .unwrap();
        seen.is_some()
    });
    assert_eq!(
        seen,
        Some((message.len(), publication.session_id(), position))
    );
}
