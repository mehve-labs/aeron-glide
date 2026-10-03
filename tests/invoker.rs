#![cfg(feature = "driver")]
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

#[test]
fn conductor_work_on_other_threads_is_serialised_with_invoke() {
    // In invoker mode the C client runs add/close work inline on the calling
    // thread; this used to race with `invoke` and crash.
    let driver = TestDriver::start();
    let client = Arc::new(driver.connect(Context::new().use_conductor_agent_invoker(true)));
    let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let invoker = {
        let (client, stop) = (client.clone(), stop.clone());
        std::thread::spawn(move || {
            while !stop.load(Ordering::Relaxed) {
                client.invoke().unwrap();
            }
        })
    };
    let workers: Vec<_> = (0..3)
        .map(|i| {
            let client = client.clone();
            std::thread::spawn(move || {
                for n in 0..50 {
                    let mut pending = client
                        .add_publication_async("aeron:ipc", 100 + i * 100 + n)
                        .unwrap();
                    let publication = loop {
                        if let Some(p) = pending.poll().unwrap() {
                            break p;
                        }
                        std::thread::yield_now();
                    };
                    drop(publication);
                }
            })
        })
        .collect();
    workers.into_iter().for_each(|w| w.join().unwrap());
    stop.store(true, Ordering::Relaxed);
    invoker.join().unwrap();
}

#[test]
fn reentrant_calls_inside_invoke_are_rejected() {
    use std::sync::{Mutex, OnceLock, Weak};
    let driver = TestDriver::start();
    let slot: Arc<OnceLock<Weak<aeron_glide::AeronClient>>> = Arc::new(OnceLock::new());
    let results = Arc::new(Mutex::new(Vec::new()));
    let (handler_slot, handler_results) = (slot.clone(), results.clone());
    let client = Arc::new(
        driver.connect(
            Context::new()
                .use_conductor_agent_invoker(true)
                .on_new_subscription(move |_| {
                    let client = handler_slot.get().unwrap().upgrade().unwrap();
                    let mut r = handler_results.lock().unwrap();
                    r.push(client.invoke().map(|_| ()).map_err(|e| e.kind()));
                    r.push(
                        client
                            .add_publication_async("aeron:ipc", 9)
                            .map(|_| ())
                            .map_err(|e| e.kind()),
                    );
                    r.push(
                        client
                            .add_publication("aeron:ipc", 9)
                            .map(|_| ())
                            .map_err(|e| e.kind()),
                    );
                }),
        ),
    );
    slot.set(Arc::downgrade(&client)).unwrap();
    client.add_subscription("aeron:ipc", 1).unwrap();
    assert_eq!(
        *results.lock().unwrap(),
        [
            Err(ErrorKind::Reentrant),
            Err(ErrorKind::Reentrant),
            Err(ErrorKind::Reentrant)
        ]
    );
}
