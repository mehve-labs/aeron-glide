mod common;

use common::{TestDriver, offer, poll_n, wait_connected, wait_until};

#[test]
fn publish_and_receive() {
    let driver = TestDriver::start();
    let client = driver.client();
    let publication = client.add_publication("aeron:ipc", 1).unwrap();
    let mut sub = client.add_subscription("aeron:ipc", 1).unwrap();
    wait_connected(&sub);

    offer(&publication, b"hello");
    let mut received = Vec::new();
    poll_n(&mut sub, 1, |data| received.push(data.to_vec()));
    assert_eq!(received, [b"hello".to_vec()]);
}

#[test]
fn image_lookups() {
    let driver = TestDriver::start();
    let client = driver.client();
    let publication = client.add_publication("aeron:ipc", 1).unwrap();
    let sub = client.add_subscription("aeron:ipc", 1).unwrap();
    wait_connected(&sub);
    offer(&publication, b"hello");

    assert_eq!(sub.image_count(), 1);
    let image = sub.image_by_index(0).expect("image at index 0");
    assert_eq!(image.session_id(), publication.session_id());
    assert!(image.position().unwrap() >= 0);
    assert!(!image.is_closed());
    assert!(!image.is_end_of_stream());

    let sid = image.session_id();
    assert_eq!(sub.image_by_session_id(sid).unwrap().session_id(), sid);
    // Missing images are `None`, not errors or aborts.
    assert!(sub.image_by_index(99).is_none());
    assert!(sub.image_by_session_id(sid.wrapping_add(1)).is_none());
}

#[test]
fn reassembly_and_session_buffers() {
    let driver = TestDriver::start();
    let client = driver.client();
    let publication = client.add_publication("aeron:ipc", 1).unwrap();
    let mut sub = client.add_subscription("aeron:ipc", 1).unwrap();
    wait_connected(&sub);

    // Larger than the MTU, so it is fragmented and reassembled into a session buffer.
    let large = vec![7u8; 16 * 1024];
    offer(&publication, &large);
    let mut received = 0;
    wait_until("reassembled message", || {
        sub.poll_assembled(10, |data: &[u8], _| {
            assert_eq!(data, &large[..]);
            received += 1;
        })
        .unwrap();
        received == 1
    });
    let sid = publication.session_id();
    assert!(sub.delete_session_buffer(sid));
    assert!(!sub.delete_session_buffer(sid));
}

#[test]
fn counters_reject_out_of_range_ids() {
    let driver = TestDriver::start();
    let client = driver.client();
    let counters = client.counters_reader();
    let mut seen = 0;
    counters.for_each(|_, _, _, _| seen += 1).unwrap();
    assert!(seen > 0, "the driver has system counters");
    for bad in [-1, counters.max_counter_id() + 1] {
        assert!(counters.get_counter_value(bad).is_err());
        assert!(counters.get_counter_state(bad).is_err());
        assert!(counters.get_counter_type_id(bad).is_err());
        assert!(counters.get_counter_label(bad).is_err());
    }
}
