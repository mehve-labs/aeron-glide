mod common;

use aeron_glide::{Context, ErrorKind};
use common::{TestDriver, offer, poll_n, wait_until};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

#[test]
fn agent_invoker_mode() {
    let driver = TestDriver::start();
    let new_subscriptions = Arc::new(AtomicUsize::new(0));
    let counter = new_subscriptions.clone();
    let client = driver.connect(
        Context::new()
            .use_conductor_agent_invoker(true)
            .on_new_subscription(move |_| {
                counter.fetch_add(1, Ordering::SeqCst);
            }),
    );
    assert!(client.uses_agent_invoker());

    // Synchronous adds drive the conductor while they wait.
    let publication = client.add_publication("aeron:ipc", 1).unwrap();
    let mut sub = client.add_subscription("aeron:ipc", 1).unwrap();
    assert_eq!(new_subscriptions.load(Ordering::SeqCst), 1);

    // Asynchronous adds complete only when the conductor is invoked.
    let mut pending = client.add_subscription_async("aeron:ipc", 2).unwrap();
    let mut second = None;
    wait_until("the async subscription", || {
        client.invoke().unwrap();
        second = pending.poll().unwrap();
        second.is_some()
    });
    assert_eq!(new_subscriptions.load(Ordering::SeqCst), 2);

    wait_until("the image", || {
        client.invoke().unwrap();
        sub.is_connected()
    });
    offer(&publication, b"invoked");
    let mut received = Vec::new();
    poll_n(&mut sub, 1, |data| received.push(data.to_vec()));
    assert_eq!(received, [b"invoked".to_vec()]);
}

#[test]
fn invoke_requires_invoker_mode() {
    let driver = TestDriver::start();
    let client = driver.client();
    assert!(!client.uses_agent_invoker());
    assert_eq!(client.invoke().unwrap_err().kind(), ErrorKind::IllegalState);
}

#[test]
fn concurrent_invokes_are_serialised() {
    let driver = TestDriver::start();
    let client = Arc::new(driver.connect(Context::new().use_conductor_agent_invoker(true)));
    let threads: Vec<_> = (0..4)
        .map(|_| {
            let client = client.clone();
            std::thread::spawn(move || {
                for _ in 0..1000 {
                    client.invoke().unwrap();
                }
            })
        })
        .collect();
    threads.into_iter().for_each(|t| t.join().unwrap());
    client.add_publication("aeron:ipc", 3).unwrap();
}
