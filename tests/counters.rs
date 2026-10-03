mod common;

use aeron_glide::{Context, CounterEvent, CountersReader, ErrorKind};
use common::{TestDriver, wait_until};
use std::sync::{Arc, Mutex};

const TYPE_ID: i32 = 1001;

fn state(reader: &CountersReader, id: i32) -> i32 {
    reader.get_counter_state(id).unwrap()
}

#[test]
fn added_counter_is_visible_to_readers() {
    let driver = TestDriver::start();
    let client = driver.client();
    let counter = client
        .add_counter(TYPE_ID, b"key bytes", "my counter")
        .unwrap();
    assert!(counter.id() >= 0);
    assert!(counter.registration_id() > 0);
    assert_eq!(counter.label(), "my counter");
    assert_eq!(counter.state(), CountersReader::RECORD_ALLOCATED);
    assert!(!counter.is_closed());

    counter.set(42);
    let reader = client.counters_reader();
    assert_eq!(reader.get_counter_value(counter.id()).unwrap(), 42);
    assert_eq!(reader.get_counter_type_id(counter.id()).unwrap(), TYPE_ID);
    assert_eq!(
        reader.get_counter_label(counter.id()).unwrap(),
        "my counter"
    );
    let mut key = None;
    reader
        .for_each(|id, _, k, _| {
            if id == counter.id() {
                key = Some(k.to_vec());
            }
        })
        .unwrap();
    assert!(key.unwrap().starts_with(b"key bytes"));

    // Another client of the same driver sees it too.
    let other = driver.client().counters_reader();
    assert_eq!(other.get_counter_value(counter.id()).unwrap(), 42);
}

#[test]
fn atomic_counter_operations() {
    let driver = TestDriver::start();
    let client = driver.client();
    let counter = client.add_counter(TYPE_ID, &[], "ops").unwrap();
    assert_eq!(counter.get(), 0);
    counter.increment();
    counter.increment_ordered();
    assert_eq!(counter.get(), 2);
    assert_eq!(counter.get_and_add(10), 2);
    assert_eq!(counter.get_and_add_ordered(-2), 12);
    assert_eq!(counter.get_and_set(7), 10);
    assert!(!counter.compare_and_set(8, 9));
    assert!(counter.compare_and_set(7, 9));
    assert_eq!(counter.get_weak(), 9);
    counter.set_ordered(-5);
    assert_eq!(counter.get(), -5);
    counter.set_weak(i64::MAX);
    assert_eq!(counter.get(), i64::MAX);
}

#[test]
fn concurrent_increments_are_not_lost() {
    let driver = TestDriver::start();
    let client = driver.client();
    let counter = Arc::new(client.add_counter(TYPE_ID, &[], "shared").unwrap());
    let threads: Vec<_> = (0..4)
        .map(|_| {
            let counter = counter.clone();
            std::thread::spawn(move || {
                for _ in 0..10_000 {
                    counter.increment();
                }
            })
        })
        .collect();
    for thread in threads {
        thread.join().unwrap();
    }
    assert_eq!(counter.get(), 40_000);
}

#[test]
fn dropped_counter_is_freed() {
    let driver = TestDriver::start();
    let client = driver.client();
    let reader = client.counters_reader();
    let counter = client.add_counter(TYPE_ID, &[], "temporary").unwrap();
    let id = counter.id();
    drop(counter);
    wait_until("the counter to be freed", || {
        state(&reader, id) != CountersReader::RECORD_ALLOCATED
    });
}

#[test]
fn a_counter_keeps_its_client_open() {
    let driver = TestDriver::start();
    let reader = driver.client().counters_reader();
    let client = driver.client();
    let client_id = client.client_id();
    let counter = client.add_counter(TYPE_ID, &[], "kept").unwrap();
    let id = counter.id();
    drop(client);
    // The client is still open: its heartbeat stays and the counter is usable.
    std::thread::sleep(std::time::Duration::from_millis(100));
    assert!(!counter.is_closed());
    assert_eq!(reader.get_counter_owner_id(id).unwrap(), client_id);
    counter.increment();
    assert_eq!(reader.get_counter_value(id).unwrap(), 1);
    drop(counter);
    wait_until("the counter to be freed", || {
        state(&reader, id) != CountersReader::RECORD_ALLOCATED
    });
}

#[test]
fn counters_close_when_the_driver_times_out_the_client() {
    let driver = TestDriver::start();
    let client = driver.connect(
        aeron_glide::Context::new()
            .driver_timeout(std::time::Duration::from_millis(500))
            .error_handler(|_| {}),
    );
    let counter = client.add_counter(TYPE_ID, &[], "orphan").unwrap();
    counter.set(3);
    drop(driver);
    wait_until("the client to close", || counter.is_closed());
    // The handle stays usable (the memory stays mapped) and drops cleanly.
    counter.increment();
    assert!(client.is_closed());
    drop(client);
    drop(counter);
}

#[test]
fn static_counters_are_shared_and_never_freed() {
    let driver = TestDriver::start();
    let client = driver.client();
    let first = client
        .add_static_counter(TYPE_ID, b"k", "static", 777)
        .unwrap();
    assert_eq!(first.registration_id(), 777);
    first.set(5);
    let id = first.id();

    // Another client gets the same counter for the same (type id, registration id).
    let other = driver.client();
    let second = other
        .add_static_counter(TYPE_ID, b"k", "static", 777)
        .unwrap();
    assert_eq!(second.id(), id);
    assert_eq!(second.get(), 5);

    drop(first);
    drop(second);
    drop(client);
    drop(other);
    let reader = driver.client().counters_reader();
    std::thread::sleep(std::time::Duration::from_millis(100));
    assert_eq!(state(&reader, id), CountersReader::RECORD_ALLOCATED);
    assert_eq!(reader.get_counter_value(id).unwrap(), 5);
}

#[test]
fn counter_handles_from_a_reader() {
    let driver = TestDriver::start();
    let client = driver.client();
    let counter = client.add_counter(TYPE_ID, &[], "viewed").unwrap();
    let reader = driver.client().counters_reader();

    // SAFETY: a counter of our own type.
    let view = unsafe { reader.counter(counter.registration_id(), counter.id()) }.unwrap();
    assert_eq!(view.id(), counter.id());
    assert_eq!(view.registration_id(), counter.registration_id());
    assert_eq!(view.label(), "viewed");
    view.get_and_add(3);
    assert_eq!(counter.get(), 3);

    // A view does not own the counter.
    drop(view);
    std::thread::sleep(std::time::Duration::from_millis(50));
    assert_eq!(
        state(&reader, counter.id()),
        CountersReader::RECORD_ALLOCATED
    );
    // SAFETY (here and below): only our own counter is written.
    assert!(
        !unsafe { reader.counter(0, counter.id()) }
            .unwrap()
            .is_closed()
    );

    for id in [-1, reader.max_counter_id() + 1, i32::MAX] {
        let err = unsafe { reader.counter(0, id) }.expect_err("out of range");
        assert_eq!(err.kind(), ErrorKind::IllegalArgument, "{err}");
    }
    // The view outlives the reader and the client it came from.
    let view = unsafe { reader.counter(0, counter.id()) }.unwrap();
    drop(reader);
    view.increment();
    assert_eq!(counter.get(), 4);
}

#[test]
fn async_counter_adds() {
    let driver = TestDriver::start();
    let client = driver.client();
    let mut pending = client.add_counter_async(TYPE_ID, &[], "async").unwrap();
    let id = pending.registration_id();
    let mut counter = None;
    wait_until("the counter", || {
        counter = pending.poll().unwrap();
        counter.is_some()
    });
    assert_eq!(counter.unwrap().registration_id(), id);
    assert_eq!(
        pending.poll().err().unwrap().kind(),
        ErrorKind::IllegalState
    );

    // A static counter's registration ID is the given one, not the add's ID.
    let pending = client
        .add_static_counter_async(TYPE_ID, &[], "async static", 9)
        .unwrap();
    assert_ne!(pending.registration_id(), 9);
    let counter = pending.wait().unwrap();
    assert_eq!(counter.registration_id(), 9);
}

#[test]
fn abandoned_counter_adds_are_freed() {
    let driver = TestDriver::start();
    let client = driver.client();
    let reader = client.counters_reader();
    let pending = client.add_counter_async(TYPE_ID, &[], "abandoned").unwrap();
    let registration_id = pending.registration_id();
    drop(pending);
    let find = || {
        let mut found = None;
        reader
            .for_each(|id, _, _, label| {
                if label == "abandoned" && state(&reader, id) == CountersReader::RECORD_ALLOCATED {
                    found = Some(id);
                }
            })
            .unwrap();
        found
    };
    wait_until("the driver to allocate it", || find().is_some());
    let id = find().unwrap();
    assert_eq!(
        reader.get_counter_value(id).unwrap(),
        0,
        "registration {registration_id}"
    );
    // The next add closes it.
    let _other = client.add_counter(TYPE_ID, &[], "other").unwrap();
    wait_until("the abandoned counter to be freed", || find().is_none());
}

#[test]
fn oversized_keys_and_labels() {
    let driver = TestDriver::start();
    let client = driver.client();
    let key = [7u8; CountersReader::MAX_KEY_LENGTH + 1];
    let err = client
        .add_counter(TYPE_ID, &key, "big key")
        .expect_err("key too long");
    assert_eq!(err.kind(), ErrorKind::IllegalArgument, "{err}");

    let label = "x".repeat(CountersReader::MAX_LABEL_LENGTH + 1);
    let err = client
        .add_static_counter(TYPE_ID, &[], &label, 1)
        .expect_err("label too long");
    assert_eq!(err.kind(), ErrorKind::IllegalArgument, "{err}");

    let counter = client.add_counter(TYPE_ID, &[], &label[1..]).unwrap();
    assert_eq!(counter.label(), label[1..]);

    let key = [1u8; CountersReader::MAX_KEY_LENGTH];
    let counter = client.add_counter(TYPE_ID, &key, "max key").unwrap();
    assert_eq!(counter.label(), "max key");
}

#[test]
fn counter_handlers_see_added_counters() {
    let driver = TestDriver::start();
    let available = Arc::new(Mutex::new(Vec::<CounterEvent>::new()));
    let unavailable = Arc::new(Mutex::new(Vec::<CounterEvent>::new()));
    let (a, u) = (available.clone(), unavailable.clone());
    let client = driver.connect(
        Context::new()
            .on_available_counter(move |e| a.lock().unwrap().push(e))
            .on_unavailable_counter(move |e| u.lock().unwrap().push(e)),
    );
    let added = Arc::new(Mutex::new(Vec::<CounterEvent>::new()));
    let sink = added.clone();
    let handler = client
        .add_available_counter_handler(move |e| sink.lock().unwrap().push(e))
        .unwrap();

    let counter = client.add_counter(TYPE_ID, &[], "watched").unwrap();
    let is_counter = |e: &CounterEvent| {
        e.registration_id == counter.registration_id() && e.counter_id == counter.id()
    };
    wait_until("the available counter events", || {
        available.lock().unwrap().iter().any(is_counter)
            && added.lock().unwrap().iter().any(is_counter)
    });
    let event = *available
        .lock()
        .unwrap()
        .iter()
        .find(|e| is_counter(e))
        .unwrap();

    client.remove_available_counter_handler(handler).unwrap();
    drop(counter);
    wait_until("the unavailable counter event", || {
        unavailable.lock().unwrap().contains(&event)
    });
    let second = client.add_counter(TYPE_ID, &[], "unwatched").unwrap();
    wait_until("the context handler", || {
        available
            .lock()
            .unwrap()
            .iter()
            .any(|e| e.counter_id == second.id())
    });
    assert!(
        !added
            .lock()
            .unwrap()
            .iter()
            .any(|e| e.registration_id == second.registration_id()),
        "removed handler still called"
    );
}

#[test]
fn counters_in_invoker_mode() {
    let driver = TestDriver::start();
    let client = driver.connect(Context::new().use_conductor_agent_invoker(true));
    let counter = client.add_counter(TYPE_ID, &[], "invoker").unwrap();
    counter.increment();
    let id = counter.id();
    let reader = client.counters_reader();
    drop(counter);
    wait_until("the counter to be freed", || {
        client.invoke().unwrap();
        state(&reader, id) != CountersReader::RECORD_ALLOCATED
    });
}

#[test]
fn reader_lookups() {
    let driver = TestDriver::start();
    let client = driver.client();
    let reader = client.counters_reader();
    let mut key = b"the key".to_vec();
    let counter = client.add_counter(TYPE_ID, &key, "looked up").unwrap();
    let (id, registration_id) = (counter.id(), counter.registration_id());

    assert_eq!(
        reader.find_by_type_id_and_registration_id(TYPE_ID, registration_id),
        Some(id)
    );
    assert_eq!(
        reader.find_by_type_id_and_registration_id(TYPE_ID + 1, registration_id),
        None
    );
    assert_eq!(reader.find_by_registration_id(-12345), None);
    assert_eq!(
        reader.get_counter_registration_id(id).unwrap(),
        registration_id
    );
    assert_eq!(reader.get_counter_owner_id(id).unwrap(), client.client_id());
    assert_eq!(
        reader.get_free_for_reuse_deadline(id).unwrap(),
        CountersReader::NOT_FREE_TO_REUSE
    );
    key.resize(CountersReader::MAX_KEY_LENGTH, 0);
    assert_eq!(reader.get_counter_key(id).unwrap(), key);

    // Registration IDs are only unique per type: the driver's system counters use
    // their counter ID.
    let static_counter = client
        .add_static_counter(TYPE_ID, &[], "static", 1 << 40)
        .unwrap();
    assert_eq!(
        reader.find_by_registration_id(1 << 40),
        Some(static_counter.id())
    );
    assert_eq!(
        reader
            .get_counter_registration_id(static_counter.id())
            .unwrap(),
        1 << 40
    );

    drop(counter);
    wait_until("the counter to be freed", || {
        reader
            .find_by_type_id_and_registration_id(TYPE_ID, registration_id)
            .is_none()
    });
    assert_ne!(
        reader.get_free_for_reuse_deadline(id).unwrap(),
        CountersReader::NOT_FREE_TO_REUSE
    );

    let max = reader.max_counter_id();
    assert!(reader.get_counter_key(max).is_ok());
    for bad in [-1, max + 1] {
        assert_eq!(
            reader.get_counter_key(bad).unwrap_err().kind(),
            ErrorKind::IllegalArgument
        );
        assert!(reader.get_counter_registration_id(bad).is_err());
        assert!(reader.get_counter_owner_id(bad).is_err());
        assert!(reader.get_free_for_reuse_deadline(bad).is_err());
    }
}

#[test]
fn driver_counters_have_known_types() {
    use aeron_glide::counter_types::*;
    let driver = TestDriver::start();
    let client = driver.client();
    let reader = client.counters_reader();
    assert_eq!(
        reader.get_counter_type_id(0).unwrap(),
        DRIVER_SYSTEM_COUNTER_TYPE_ID
    );
    let sub = client.add_subscription("aeron:ipc", 3).unwrap();
    let publication = client.add_publication("aeron:ipc", 3).unwrap();
    common::wait_connected(&sub);
    let image = sub.images().into_iter().next().expect("an image");
    assert_eq!(
        reader
            .get_counter_type_id(image.subscriber_position_id())
            .unwrap(),
        DRIVER_SUBSCRIBER_POSITION_TYPE_ID
    );
    assert_eq!(
        reader
            .get_counter_type_id(publication.publication_limit_id())
            .unwrap(),
        DRIVER_PUBLISHER_LIMIT_TYPE_ID
    );
}

#[test]
fn owners_states_and_static_conflicts() {
    let driver = TestDriver::start();
    let client = driver.client();
    let reader = client.counters_reader();
    // System counters and static counters have no owner.
    assert_eq!(reader.get_counter_owner_id(0).unwrap(), -1);
    let static_counter = client.add_static_counter(TYPE_ID, &[], "s", 55).unwrap();
    assert_eq!(
        reader.get_counter_owner_id(static_counter.id()).unwrap(),
        -1
    );
    assert_eq!(
        reader.get_counter_state(reader.max_counter_id()).unwrap(),
        CountersReader::RECORD_UNUSED
    );

    // A static counter cannot take the (type, registration ID) of a normal one.
    let counter = client.add_counter(TYPE_ID, b"key", "normal").unwrap();
    let err = client
        .add_static_counter(TYPE_ID, &[], "clash", counter.registration_id())
        .expect_err("conflict");
    assert_eq!(err.kind(), ErrorKind::Aeron, "{err}");
    assert!(err.code() < 0, "{err}");

    // A freed counter is reclaimed and its key cleared.
    let id = counter.id();
    drop(counter);
    wait_until("the counter to be freed", || {
        state(&reader, id) == CountersReader::RECORD_RECLAIMED
    });
    assert_eq!(
        reader.get_counter_key(id).unwrap(),
        [0; CountersReader::MAX_KEY_LENGTH]
    );
}

#[test]
fn counter_dropped_from_a_handler_with_the_last_client_reference() {
    let driver = TestDriver::start();
    let slot: Arc<Mutex<Option<aeron_glide::Counter>>> = Arc::new(Mutex::new(None));
    let handler_slot = slot.clone();
    let dropped = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let flag = dropped.clone();
    let client = driver.connect(Context::new().on_unavailable_counter(move |_| {
        // Drops the last handle on the client, on its conductor thread.
        if handler_slot.lock().unwrap().take().is_some() {
            flag.store(true, std::sync::atomic::Ordering::SeqCst);
        }
    }));
    let watched = client.add_counter(TYPE_ID, &[], "trigger").unwrap();
    *slot.lock().unwrap() = Some(client.add_counter(TYPE_ID, &[], "owner").unwrap());
    drop(client);
    drop(watched); // its unavailable event runs the handler
    wait_until("the handler to drop the counter", || {
        dropped.load(std::sync::atomic::Ordering::SeqCst)
    });
}

#[test]
fn compare_and_set_increments_are_not_lost() {
    // A failed compare-and-set must report failure: on ARM the upstream C++
    // could report success without writing, losing increments here.
    let driver = TestDriver::start();
    let client = driver.client();
    let counter = Arc::new(client.add_counter(TYPE_ID, &[], "cas").unwrap());
    let threads: Vec<_> = (0..4)
        .map(|_| {
            let counter = counter.clone();
            std::thread::spawn(move || {
                for _ in 0..20_000 {
                    loop {
                        let value = counter.get();
                        if counter.compare_and_set(value, value + 1) {
                            break;
                        }
                    }
                }
            })
        })
        .collect();
    for thread in threads {
        thread.join().unwrap();
    }
    assert_eq!(counter.get(), 80_000);
}

#[test]
fn adds_on_a_closed_client_fail_fast() {
    let driver = TestDriver::start();
    let client = driver.connect(
        Context::new()
            .driver_timeout(std::time::Duration::from_secs(1))
            .error_handler(|_| {}),
    );
    drop(driver);
    wait_until("the client to close", || client.is_closed());
    let start = std::time::Instant::now();
    let err = client
        .add_counter(TYPE_ID, &[], "late")
        .expect_err("closed");
    assert!(
        start.elapsed() < std::time::Duration::from_millis(500),
        "{err}"
    );
}
