mod common;

use common::{TestDriver, free_udp_port, poll_n, wait_connected, wait_until};

fn endpoint() -> String {
    format!("aeron:udp?endpoint=127.0.0.1:{}", free_udp_port())
}

#[test]
fn multi_destination_cast() {
    let driver = TestDriver::start();
    let client = driver.client();
    let destination = endpoint();
    let publication = client
        .add_publication("aeron:udp?control-mode=manual", 1)
        .unwrap();
    let mut sub = client.add_subscription(&destination, 1).unwrap();

    let id = publication.add_destination(&destination).unwrap();
    wait_until("the destination to be added", || {
        publication.find_destination_response(id).unwrap()
    });
    wait_connected(&sub);
    common::offer(&publication, b"to the destination");
    let mut received = Vec::new();
    poll_n(&mut sub, 1, |data| received.push(data.to_vec()));
    assert_eq!(received, [b"to the destination".to_vec()]);

    let removal = publication.remove_destination_by_id(id).unwrap();
    wait_until("the destination to be removed", || {
        publication.find_destination_response(removal).unwrap()
    });
}

#[test]
fn exclusive_publication_destinations_by_endpoint() {
    let driver = TestDriver::start();
    let client = driver.client();
    let destination = endpoint();
    let publication = client
        .add_exclusive_publication("aeron:udp?control-mode=manual", 2)
        .unwrap();
    let id = publication.add_destination(&destination).unwrap();
    wait_until("the destination to be added", || {
        publication.find_destination_response(id).unwrap()
    });
    let removal = publication.remove_destination(&destination).unwrap();
    wait_until("the destination to be removed", || {
        publication.find_destination_response(removal).unwrap()
    });
}

#[test]
fn multi_destination_subscription() {
    let driver = TestDriver::start();
    let client = driver.client();
    let destination = endpoint();
    let mut sub = client
        .add_subscription("aeron:udp?control-mode=manual", 3)
        .unwrap();
    let id = sub.add_destination(&destination).unwrap();
    wait_until("the destination to be added", || {
        sub.find_destination_response(id).unwrap()
    });

    let publication = client.add_publication(&destination, 3).unwrap();
    wait_connected(&sub);
    common::offer(&publication, b"via the destination");
    let mut received = Vec::new();
    poll_n(&mut sub, 1, |data| received.push(data.to_vec()));
    assert_eq!(received, [b"via the destination".to_vec()]);
    wait_until("the transport count", || {
        sub.image_by_index(0)
            .is_some_and(|image| image.active_transport_count().unwrap() == 1)
    });

    let removal = sub.remove_destination(&destination).unwrap();
    wait_until("the destination to be removed", || {
        sub.find_destination_response(removal).unwrap()
    });
}

#[test]
fn unknown_correlation_id_is_an_error() {
    let driver = TestDriver::start();
    let client = driver.client();
    let publication = client
        .add_publication("aeron:udp?control-mode=manual", 4)
        .unwrap();
    assert!(publication.find_destination_response(123_456).is_err());
}
