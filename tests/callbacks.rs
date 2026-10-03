mod common;

use common::{TestDriver, offer, poll_n, wait_connected, wait_until};
use std::panic::{AssertUnwindSafe, catch_unwind};

#[test]
fn handlers_can_poll_other_subscriptions() {
    let driver = TestDriver::start();
    let client = driver.client();
    let (pub1, pub2) = (
        client.add_publication("aeron:ipc", 1).unwrap(),
        client.add_publication("aeron:ipc", 2).unwrap(),
    );
    let mut sub1 = client.add_subscription("aeron:ipc", 1).unwrap();
    let mut sub2 = client.add_subscription("aeron:ipc", 2).unwrap();
    wait_connected(&sub1);
    wait_connected(&sub2);
    offer(&pub1, b"outer");
    offer(&pub2, b"inner");

    let (mut outer, mut inner) = (0, 0);
    wait_until("nested poll", || {
        sub1.poll(10, |_, _| {
            outer += 1;
            sub2.poll(10, |_, _| inner += 1).unwrap();
        })
        .unwrap();
        outer > 0 && inner > 0
    });
}

#[test]
fn handler_panic_unwinds_to_the_caller() {
    let driver = TestDriver::start();
    let client = driver.client();
    let publication = client.add_publication("aeron:ipc", 1).unwrap();
    let mut sub = client.add_subscription("aeron:ipc", 1).unwrap();
    wait_connected(&sub);
    offer(&publication, b"boom");

    let payload = catch_unwind(AssertUnwindSafe(|| {
        wait_until("panicking handler", || {
            sub.poll(10, |_, _| panic!("handler panic")).unwrap();
            false
        })
    }))
    .expect_err("the handler panic propagates");
    assert_eq!(payload.downcast_ref::<&str>(), Some(&"handler panic"));

    // The subscription keeps working.
    offer(&publication, b"after");
    let mut received = Vec::new();
    poll_n(&mut sub, 1, |data| received.push(data.to_vec()));
    assert_eq!(received, [b"after".to_vec()]);
}

#[test]
fn assembled_poll_panic_redelivers_the_fragment() {
    let driver = TestDriver::start();
    let client = driver.client();
    let publication = client.add_publication("aeron:ipc", 1).unwrap();
    let mut sub = client.add_subscription("aeron:ipc", 1).unwrap();
    wait_connected(&sub);
    offer(&publication, b"again");

    let result = catch_unwind(AssertUnwindSafe(|| {
        wait_until("panicking assembled handler", || {
            sub.poll_assembled(10, |_: &[u8], _| -> () { panic!("assembled panic") })
                .unwrap();
            false
        })
    }));
    let payload = result.expect_err("the handler panic propagates");
    assert_eq!(payload.downcast_ref::<&str>(), Some(&"assembled panic"));

    let mut received = Vec::new();
    wait_until("redelivery", || {
        sub.poll_assembled(10, |data: &[u8], _| received.push(data.to_vec()))
            .unwrap();
        !received.is_empty()
    });
    assert_eq!(received, [b"again".to_vec()]);
}
