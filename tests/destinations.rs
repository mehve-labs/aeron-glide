#![cfg(feature = "driver")]
mod common;

use common::{TestDriver, free_udp_port, poll_n, wait_connected, wait_until};

fn endpoint() -> String {
    format!("aeron:udp?endpoint=127.0.0.1:{}", free_udp_port())
}

/// Run `bind` with a fresh endpoint, again if another test took the probed port
/// before Aeron bound it.
fn on_free_endpoint<T>(mut bind: impl FnMut(&str) -> aeron_glide::Result<T>) -> (String, T) {
    for _ in 0..5 {
        let destination = endpoint();
        match bind(&destination) {
            Ok(value) => return (destination, value),
            Err(e) if e.message().contains("Address already in use") => continue,
            Err(e) => panic!("{e}"),
        }
    }
    panic!("no free endpoint after 5 tries")
}

#[test]
fn multi_destination_cast() {
    let driver = TestDriver::start();
    let client = driver.client();
    let publication = client
        .add_publication("aeron:udp?control-mode=manual", 1)
        .unwrap();
    let (destination, mut sub) = on_free_endpoint(|e| client.add_subscription(e, 1));

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
    let mut sub = client
        .add_subscription("aeron:udp?control-mode=manual", 3)
        .unwrap();
    // Binding the destination's port fails in the response if it was taken.
    let (destination, _id) = on_free_endpoint(|e| {
        let id = sub.add_destination(e)?;
        let deadline = std::time::Instant::now() + common::TIMEOUT;
        while !sub.find_destination_response(id)? {
            assert!(
                std::time::Instant::now() < deadline,
                "no destination response"
            );
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        Ok(id)
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
