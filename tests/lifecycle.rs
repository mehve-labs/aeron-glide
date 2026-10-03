mod common;

use aeron_glide::{Context, ErrorKind, IdleStrategy, InferableBoolean, MediaDriver};
use common::{TestDriver, offer, wait_connected, wait_until};
use std::sync::{Arc, Mutex};
use std::time::Duration;

#[test]
fn image_outlives_its_client_handle() {
    let driver = TestDriver::start();
    let client = driver.client();
    let publication = client.add_publication("aeron:ipc", 1).unwrap();
    let sub = client.add_subscription("aeron:ipc", 1).unwrap();
    wait_connected(&sub);
    offer(&publication, b"hello");

    // The subscription the image borrows keeps the C++ client alive. (An image
    // outliving its subscription is rejected at compile time; see the `Image` doctest.)
    let image = sub.image_by_index(0).expect("image");
    drop(client);
    assert!(image.position().is_ok());
    assert!(!image.is_closed());
}

#[test]
fn closed_image_reports_its_final_position() {
    let driver = TestDriver::start();
    let client = driver.client();
    let publication = client.add_publication("aeron:ipc", 1).unwrap();
    let sub = client.add_subscription("aeron:ipc", 1).unwrap();
    wait_connected(&sub);
    offer(&publication, b"last");

    // Consume the message: an IPC publication drains until its subscribers catch up.
    let mut image = sub.image_by_index(0).expect("image");
    let mut read = 0;
    wait_until("the message", || {
        read += image.poll(10, |_| {}).unwrap();
        read == 1
    });
    drop(publication);
    wait_until("image to close", || image.is_closed());
    // Aeron reports the final position of a closed image rather than an error.
    assert!(image.position().unwrap() >= 0);
}

#[test]
fn driver_loss_is_reported_to_the_error_handler() {
    let driver = TestDriver::start();
    let errors = Arc::new(Mutex::new(Vec::new()));
    let sink = errors.clone();
    let client = driver.connect(
        Context::new()
            .client_name("lifecycle-test")
            .driver_timeout(Duration::from_millis(500))
            .error_handler(move |e| sink.lock().unwrap().push(e.clone())),
    );
    assert!(!client.is_closed());

    // Losing the driver is reported to the handler; the process keeps running.
    drop(driver);
    wait_until("an error report", || !errors.lock().unwrap().is_empty());
    let first = errors.lock().unwrap()[0].clone();
    assert_eq!(first.kind(), ErrorKind::DriverTimeout, "{first}");
    assert!(first.is_fatal());
    wait_until("the client to close", || client.is_closed());
}

#[test]
fn generated_driver_settings_round_trip() {
    let dir = std::env::temp_dir().join(format!("aeron-glide-gen-{}", std::process::id()));
    let dir = dir.to_str().unwrap().to_string();
    let driver = MediaDriver::builder()
        .dir(&dir)
        .dir_delete_on_start(true)
        .dir_delete_on_shutdown(true)
        .publication_linger_timeout_ns(1_000_000)
        .term_buffer_length(1 << 20)
        .conductor_idle_strategy(IdleStrategy::Sleeping)
        .receiver_group_consideration(InferableBoolean::ForceTrue)
        .receiver_group_tag(Some(7))
        .sender_wildcard_port_range(20_000, 20_100)
        .start()
        .unwrap();
    assert_eq!(driver.dir(), dir);
    assert!(driver.dir_delete_on_shutdown());
    assert_eq!(driver.publication_linger_timeout_ns(), 1_000_000);
    assert_eq!(driver.term_buffer_length(), 1 << 20);
    assert_eq!(driver.conductor_idle_strategy(), "sleeping");
    assert_eq!(
        driver.receiver_group_consideration(),
        InferableBoolean::ForceTrue
    );
    assert!(driver.receiver_group_tag_is_present());
    assert_eq!(driver.receiver_group_tag_value(), 7);
}
