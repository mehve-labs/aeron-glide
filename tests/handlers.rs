mod common;

use aeron_glide::{Context, ImageEvent};
use common::{TestDriver, free_udp_port, offer, wait_connected, wait_until};
use std::sync::{Arc, Mutex};

type Sink<T> = Arc<Mutex<Vec<T>>>;

fn sink<T>() -> (Sink<T>, Sink<T>) {
    let events = Arc::new(Mutex::new(Vec::new()));
    (events.clone(), events)
}

#[test]
fn image_handlers_from_the_context() {
    let driver = TestDriver::start();
    let (available, available_sink) = sink::<ImageEvent>();
    let (unavailable, unavailable_sink) = sink::<ImageEvent>();
    let client = driver.connect(
        Context::new()
            .on_available_image(move |e| available_sink.lock().unwrap().push(e.clone()))
            .on_unavailable_image(move |e| unavailable_sink.lock().unwrap().push(e.clone())),
    );
    let sub = client.add_subscription("aeron:ipc", 1).unwrap();
    let publication = client.add_exclusive_publication("aeron:ipc", 1).unwrap();
    wait_until("the available image", || {
        !available.lock().unwrap().is_empty()
    });
    let event = available.lock().unwrap()[0].clone();
    assert_eq!(event.session_id, publication.session_id());
    assert_eq!(event.subscription_registration_id, sub.registration_id());
    assert_eq!(event.term_buffer_length, 1 << 20);

    drop(publication);
    wait_until("the unavailable image", || {
        !unavailable.lock().unwrap().is_empty()
    });
    assert_eq!(unavailable.lock().unwrap()[0].session_id, event.session_id);
}

#[test]
fn per_subscription_image_handlers() {
    let driver = TestDriver::start();
    let client = driver.client();
    let (available, available_sink) = sink::<i32>();
    let (unavailable, unavailable_sink) = sink::<i32>();
    let sub = client
        .add_subscription_with_image_handlers(
            "aeron:ipc",
            2,
            move |e| available_sink.lock().unwrap().push(e.session_id),
            move |e| unavailable_sink.lock().unwrap().push(e.session_id),
        )
        .unwrap();
    let publication = client.add_exclusive_publication("aeron:ipc", 2).unwrap();
    wait_connected(&sub);
    wait_until("the available image", || {
        !available.lock().unwrap().is_empty()
    });
    assert_eq!(*available.lock().unwrap(), [publication.session_id()]);
    drop(publication);
    wait_until("the unavailable image", || {
        !unavailable.lock().unwrap().is_empty()
    });
}

#[test]
fn new_resource_handlers() {
    let driver = TestDriver::start();
    let (events, events_sink) = sink::<(String, String, i32, i64)>();
    let (a, b, c) = (events_sink.clone(), events_sink.clone(), events_sink);
    let client = driver.connect(
        Context::new()
            .on_new_publication(move |e| {
                a.lock().unwrap().push((
                    "publication".into(),
                    e.channel.clone(),
                    e.stream_id,
                    e.correlation_id,
                ))
            })
            .on_new_exclusive_publication(move |e| {
                b.lock().unwrap().push((
                    "exclusive".into(),
                    e.channel.clone(),
                    e.stream_id,
                    e.correlation_id,
                ))
            })
            .on_new_subscription(move |e| {
                c.lock().unwrap().push((
                    "subscription".into(),
                    e.channel.clone(),
                    e.stream_id,
                    e.correlation_id,
                ))
            }),
    );
    let publication = client.add_publication("aeron:ipc", 3).unwrap();
    let exclusive = client.add_exclusive_publication("aeron:ipc", 4).unwrap();
    let sub = client.add_subscription("aeron:ipc", 5).unwrap();
    wait_until("three events", || events.lock().unwrap().len() == 3);
    let mut seen = events.lock().unwrap().clone();
    seen.sort();
    assert_eq!(
        seen,
        [
            (
                "exclusive".into(),
                "aeron:ipc".into(),
                4,
                exclusive.registration_id()
            ),
            (
                "publication".into(),
                "aeron:ipc".into(),
                3,
                publication.registration_id()
            ),
            (
                "subscription".into(),
                "aeron:ipc".into(),
                5,
                sub.registration_id()
            ),
        ]
    );
}

#[test]
fn close_client_handlers() {
    let driver = TestDriver::start();
    let (closed, closed_sink) = sink::<&'static str>();
    let context_sink = closed_sink.clone();
    let client = driver.connect(
        Context::new().on_close_client(move || context_sink.lock().unwrap().push("context")),
    );
    let runtime_sink = closed_sink.clone();
    client
        .add_close_client_handler(move || runtime_sink.lock().unwrap().push("runtime"))
        .unwrap();
    let removed = client
        .add_close_client_handler(move || closed_sink.lock().unwrap().push("removed"))
        .unwrap();
    client.remove_close_client_handler(removed).unwrap();

    drop(client);
    let mut seen = closed.lock().unwrap().clone();
    seen.sort();
    assert_eq!(seen, ["context", "runtime"]);
}

#[test]
fn error_frame_from_a_rejected_image() {
    let driver = TestDriver::start();
    let (frames, frames_sink) = sink::<aeron_glide::PublicationErrorFrame>();
    let client = driver.connect(
        Context::new()
            .on_publication_error_frame(move |f| frames_sink.lock().unwrap().push(f.clone())),
    );
    let channel = format!("aeron:udp?endpoint=127.0.0.1:{}", free_udp_port());
    let sub = client.add_subscription(&channel, 6).unwrap();
    let publication = client.add_publication(&channel, 6).unwrap();
    wait_connected(&sub);
    offer(&publication, b"hello");
    sub.image_by_index(0).unwrap().reject("go away").unwrap();

    wait_until("the error frame", || !frames.lock().unwrap().is_empty());
    let frame = frames.lock().unwrap()[0].clone();
    assert_eq!(frame.registration_id, publication.registration_id());
    assert_eq!(frame.session_id, publication.session_id());
    assert_eq!(frame.stream_id, 6);
    let source = frame.source.expect("source address");
    assert!(source.ip().is_loopback(), "{source}");
}

#[test]
fn counter_handlers_and_panicking_handlers() {
    let driver = TestDriver::start();
    let client = driver
        .connect(Context::new().on_new_subscription(|_| panic!("handler panic is contained")));
    let (counters, counters_sink) = sink::<i32>();
    let id = client
        .add_available_counter_handler(move |e| counters_sink.lock().unwrap().push(e.counter_id))
        .unwrap();
    // The panicking handler runs on the conductor thread; the client keeps working.
    let sub = client.add_subscription("aeron:ipc", 7).unwrap();
    let publication = client.add_publication("aeron:ipc", 7).unwrap();
    wait_connected(&sub);
    offer(&publication, b"still working");
    client.remove_available_counter_handler(id).unwrap();
    let unavailable = client.add_unavailable_counter_handler(|_| {}).unwrap();
    client
        .remove_unavailable_counter_handler(unavailable)
        .unwrap();
    // Counter events themselves are tested in tests/counters.rs.
    drop(counters);
}

struct DropFlag(Arc<std::sync::atomic::AtomicBool>);

impl Drop for DropFlag {
    fn drop(&mut self) {
        self.0.store(true, std::sync::atomic::Ordering::SeqCst);
    }
}

fn drop_flag() -> (DropFlag, Arc<std::sync::atomic::AtomicBool>) {
    let flag = Arc::new(std::sync::atomic::AtomicBool::new(false));
    (DropFlag(flag.clone()), flag)
}

#[test]
fn handlers_are_released() {
    use std::sync::atomic::Ordering::SeqCst;
    let driver = TestDriver::start();
    let (context_flag, context_released) = drop_flag();
    let client = driver.connect(Context::new().on_close_client(move || {
        let _ = &context_flag;
    }));

    let (removed_flag, removed_released) = drop_flag();
    let id = client
        .add_close_client_handler(move || {
            let _ = &removed_flag;
        })
        .unwrap();
    client.remove_close_client_handler(id).unwrap();
    assert!(removed_released.load(SeqCst), "removed handler is released");

    let (image_flag, image_released) = drop_flag();
    let sub = client
        .add_subscription_with_image_handlers(
            "aeron:ipc",
            8,
            move |_| {
                let _ = &image_flag;
            },
            |_| {},
        )
        .unwrap();
    drop(sub);
    wait_until("the subscription's handlers to be released", || {
        image_released.load(SeqCst)
    });

    assert!(!context_released.load(SeqCst));
    drop(client);
    assert!(
        context_released.load(SeqCst),
        "context handler released with the client"
    );
}

#[test]
fn handler_owning_the_last_client_is_released_safely() {
    // The per-subscription handlers are released on the conductor thread when the
    // subscription closes; if they own the last client, its drop must not run there.
    let driver = TestDriver::start();
    let client = Arc::new(driver.client());
    let owner = client.clone();
    let sub = client
        .add_subscription_with_image_handlers(
            "aeron:ipc",
            9,
            move |_| {
                let _ = &owner;
            },
            |_| {},
        )
        .unwrap();
    drop(client);
    let (done_tx, done_rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        drop(sub);
        done_tx.send(()).unwrap();
    });
    done_rx
        .recv_timeout(std::time::Duration::from_secs(10))
        .expect("the subscription and client close without hanging");
}

#[test]
fn reentrant_calls_from_handlers_are_rejected() {
    use aeron_glide::{AeronClient, ErrorKind};
    use std::sync::{OnceLock, Weak};
    let driver = TestDriver::start();
    let slot: Arc<OnceLock<Weak<AeronClient>>> = Arc::new(OnceLock::new());
    let (results, results_sink) = sink::<Result<(), ErrorKind>>();
    let handler_slot = slot.clone();
    let client = Arc::new(driver.connect(Context::new().on_new_publication(move |_| {
        let Some(client) = handler_slot.get().and_then(Weak::upgrade) else {
            return;
        };
        let mut r = results_sink.lock().unwrap();
        r.push(
            client
                .add_close_client_handler(|| {})
                .map(|_| ())
                .map_err(|e| e.kind()),
        );
        r.push(client.remove_close_client_handler(1).map_err(|e| e.kind()));
        r.push(
            client
                .add_subscription("aeron:ipc", 2)
                .map(|_| ())
                .map_err(|e| e.kind()),
        );
    })));
    slot.set(Arc::downgrade(&client)).unwrap();
    client.add_publication("aeron:ipc", 1).unwrap();
    wait_until("the handler", || results.lock().unwrap().len() == 3);
    assert_eq!(
        *results.lock().unwrap(),
        [
            Err(ErrorKind::Reentrant),
            Err(ErrorKind::Reentrant),
            Err(ErrorKind::Reentrant)
        ]
    );
}
