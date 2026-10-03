#![cfg(feature = "driver")]
mod common;

use aeron_glide::{AeronClient, Context, ErrorKind, IdleStrategy, InferableBoolean, ThreadingMode};
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
        read += image.poll(10, |_, _| {}).unwrap();
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
fn error_handler_may_drop_the_client() {
    let driver = TestDriver::start();
    let slot: Arc<Mutex<Option<AeronClient>>> = Arc::new(Mutex::new(None));
    let seen = Arc::new(Mutex::new(Vec::new()));
    let (handler_slot, handler_seen) = (slot.clone(), seen.clone());
    let label = String::from("captured by the handler");
    let client = driver.connect(
        Context::new()
            .driver_timeout(Duration::from_millis(500))
            .error_handler(move |e| {
                // Dropping the last client here used to free this closure mid-call
                // and make the conductor join itself.
                drop(handler_slot.lock().unwrap().take());
                handler_seen.lock().unwrap().push((e.kind(), label.clone()));
            }),
    );
    *slot.lock().unwrap() = Some(client);
    drop(driver);
    wait_until("the handler to run", || !seen.lock().unwrap().is_empty());
    let (kind, captured) = seen.lock().unwrap()[0].clone();
    assert_eq!(kind, ErrorKind::DriverTimeout);
    assert_eq!(captured, "captured by the handler");
    assert!(slot.lock().unwrap().is_none());
}

#[test]
fn generated_driver_settings_round_trip() {
    let driver = TestDriver::start_with(|builder| {
        builder
            .publication_linger_timeout_ns(1_000_000)
            .term_buffer_length(1 << 20)
            .conductor_idle_strategy(IdleStrategy::Sleeping)
            .receiver_group_consideration(InferableBoolean::ForceTrue)
            .receiver_group_tag(Some(7))
            .sender_wildcard_port_range(20_000, 20_100)
            .untethered_linger_timeout_ns(u64::MAX)
            // Init args set after their strategy still apply (the strategy is reloaded).
            .sender_idle_strategy(IdleStrategy::Sleeping)
            .sender_idle_strategy_init_args("1us")
            // These C setters keep the pointer they are given (regression: dangling).
            .resolver_name("a-fairly-long-resolver-name-that-is-not-inlined")
            .name_resolver_init_args("some-init-args")
    });
    let d = driver.driver();
    assert_eq!(d.dir(), driver.dir);
    assert!(d.dir_delete_on_shutdown());
    assert_eq!(d.threading_mode(), ThreadingMode::Shared);
    assert_eq!(d.publication_linger_timeout_ns(), 1_000_000);
    assert_eq!(d.term_buffer_length(), 1 << 20);
    assert_eq!(d.conductor_idle_strategy(), "sleeping");
    assert_eq!(
        d.receiver_group_consideration(),
        InferableBoolean::ForceTrue
    );
    assert!(d.receiver_group_tag_is_present());
    assert_eq!(d.receiver_group_tag_value(), 7);
    assert_eq!(d.sender_wildcard_port_range(), (20_000, 20_100));
    assert_eq!(d.untethered_linger_timeout_ns(), u64::MAX);
    assert_eq!(d.sender_idle_strategy(), "sleeping");
    assert_eq!(d.sender_idle_strategy_init_args(), "1us");
    assert_eq!(
        d.resolver_name(),
        "a-fairly-long-resolver-name-that-is-not-inlined"
    );
    assert_eq!(d.name_resolver_init_args(), "some-init-args");
}

#[test]
fn huge_client_timeouts_are_clamped() {
    let driver = TestDriver::start();
    let client = driver.connect(
        Context::new()
            .driver_timeout(Duration::MAX)
            .resource_linger_timeout(Duration::MAX),
    );
    std::thread::sleep(Duration::from_millis(1500));
    assert!(
        !client.is_closed(),
        "an overflowing timeout closed the client"
    );
}
