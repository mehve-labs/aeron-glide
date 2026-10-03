mod common;

use aeron_glide::heartbeat_timestamp::{
    CLIENT_HEARTBEAT_TYPE_ID, find_counter_id_by_registration_id, is_active,
};
use aeron_glide::{CncFile, ErrorKind};
use common::{TestDriver, wait_until};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

#[test]
fn counters_through_the_cnc_file() {
    let driver = TestDriver::start();
    let client = driver.client();
    let counter = client.add_counter(1001, b"k", "from a client").unwrap();
    counter.set(99);

    let cnc = CncFile::map_existing(&driver.dir).unwrap();
    let reader = cnc.counters_reader().unwrap();
    assert_eq!(reader.get_counter_label(0).unwrap(), "Bytes sent");
    assert_eq!(
        reader.get_counter_label(counter.id()).unwrap(),
        "from a client"
    );
    assert_eq!(reader.get_counter_value(counter.id()).unwrap(), 99);
    assert_eq!(
        reader.find_by_type_id_and_registration_id(1001, counter.registration_id()),
        Some(counter.id())
    );

    // The mapping is read-only: no writable counter handles.
    let err = reader
        .counter(counter.registration_id(), counter.id())
        .expect_err("read-only");
    assert_eq!(err.kind(), ErrorKind::UnsupportedOperation, "{err}");

    // The reader keeps the file mapped.
    drop(cnc);
    counter.set(100);
    assert_eq!(reader.get_counter_value(counter.id()).unwrap(), 100);
}

#[test]
fn driver_errors_are_in_the_error_log() {
    let driver = TestDriver::start();
    let cnc = CncFile::map_existing(&driver.dir).unwrap();
    assert_eq!(cnc.read_error_log(0, |_, _, _, _| {}).unwrap(), 0);

    let client = driver.client();
    let channel = "aeron:udp?endpoint=not-a-host-name.invalid:1";
    assert!(client.add_publication(channel, 1).is_err());
    assert!(client.add_publication(channel, 2).is_err());

    let start = Instant::now();
    let mut errors = Vec::new();
    wait_until("the error to be logged", || {
        errors.clear();
        cnc.read_error_log(0, |count, first, last, error| {
            errors.push((count, first, last, error.to_string()))
        })
        .unwrap();
        !errors.is_empty()
    });
    let (count, first, last, error) = &errors[0];
    assert!(*count >= 1, "{errors:?}");
    assert!(first <= last && *first > 0, "{errors:?}");
    assert!(error.contains("not-a-host-name.invalid"), "{error}");

    // Only errors observed at or after the timestamp.
    let future = SystemTime::now().duration_since(UNIX_EPOCH).unwrap() + Duration::from_secs(60);
    assert_eq!(
        cnc.read_error_log(future.as_millis() as i64, |_, _, _, _| panic!("none"))
            .unwrap(),
        0
    );
    assert!(start.elapsed() < Duration::from_secs(5));
}

#[test]
fn error_log_consumer_panics_propagate() {
    let driver = TestDriver::start();
    let client = driver.client();
    assert!(
        client
            .add_publication("aeron:udp?endpoint=not-a-host-name.invalid:1", 1)
            .is_err()
    );
    let cnc = CncFile::map_existing(&driver.dir).unwrap();
    wait_until("the error to be logged", || {
        cnc.read_error_log(0, |_, _, _, _| {}).unwrap() > 0
    });
    let panic = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        cnc.read_error_log(0, |_, _, _, _| panic!("consumer panic"))
    }))
    .expect_err("the panic propagates");
    assert_eq!(panic.downcast_ref::<&str>(), Some(&"consumer panic"));
    // The file is still usable.
    assert!(cnc.read_error_log(0, |_, _, _, _| {}).unwrap() > 0);
}

#[test]
fn missing_cnc_file_is_an_error() {
    let dir = std::env::temp_dir().join(format!("aeron-glide-no-driver-{}", std::process::id()));
    let start = Instant::now();
    let err = CncFile::map_existing(dir.to_str().unwrap()).expect_err("no driver");
    assert_eq!(err.kind(), ErrorKind::Io, "{err}");
    assert!(err.message().contains("CnC"), "{err}");
    // mapExisting waits for the file before giving up.
    assert!(start.elapsed() >= Duration::from_secs(9), "{err}");
}

#[test]
fn client_liveness_through_heartbeats() {
    let driver = TestDriver::start();
    let observer = driver.client();
    let reader = observer.counters_reader();
    let client = driver.client();
    let client_id = client.client_id();
    // The driver allocates the heartbeat counter at the client's first command.
    let sub = client.add_subscription("aeron:ipc", 1).unwrap();
    let _observer_sub = observer.add_subscription("aeron:ipc", 1).unwrap();

    let mut id = None;
    wait_until("the client's heartbeat counter", || {
        id = find_counter_id_by_registration_id(&reader, CLIENT_HEARTBEAT_TYPE_ID, client_id);
        id.is_some()
    });
    let id = id.unwrap();
    assert_eq!(
        reader.get_counter_type_id(id).unwrap(),
        CLIENT_HEARTBEAT_TYPE_ID
    );
    assert!(is_active(&reader, id, CLIENT_HEARTBEAT_TYPE_ID, client_id));
    assert!(!is_active(
        &reader,
        id,
        CLIENT_HEARTBEAT_TYPE_ID,
        client_id + 1000
    ));
    assert!(!is_active(&reader, id, 1001, client_id));
    for bad in [-1, reader.max_counter_id() + 1, i32::MAX] {
        assert!(!is_active(
            &reader,
            bad,
            CLIENT_HEARTBEAT_TYPE_ID,
            client_id
        ));
    }
    assert_eq!(
        find_counter_id_by_registration_id(&reader, CLIENT_HEARTBEAT_TYPE_ID, -42),
        None
    );

    drop(sub); // it keeps the client open
    drop(client);
    wait_until("the heartbeat counter to be freed", || {
        !is_active(&reader, id, CLIENT_HEARTBEAT_TYPE_ID, client_id)
    });

    // The same works through the CnC file.
    let cnc = CncFile::map_existing(&driver.dir).unwrap();
    let reader = cnc.counters_reader().unwrap();
    let observer_id = observer.client_id();
    let id = find_counter_id_by_registration_id(&reader, CLIENT_HEARTBEAT_TYPE_ID, observer_id)
        .expect("observer heartbeat");
    assert!(is_active(
        &reader,
        id,
        CLIENT_HEARTBEAT_TYPE_ID,
        observer_id
    ));
}
