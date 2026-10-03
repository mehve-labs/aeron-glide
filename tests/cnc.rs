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
    let reader = cnc.counters_reader();
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
    // SAFETY: refused before any write.
    let err =
        unsafe { reader.counter(counter.registration_id(), counter.id()) }.expect_err("read-only");
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
    assert_eq!(cnc.read_error_log(0, |_| {}).unwrap(), 0);

    let client = driver.client();
    let channel = "aeron:udp?endpoint=not-a-host-name.invalid:1";
    assert!(client.add_publication(channel, 1).is_err());
    assert!(client.add_publication(channel, 2).is_err());

    let start = Instant::now();
    let mut errors = Vec::new();
    wait_until("the error to be logged", || {
        errors.clear();
        cnc.read_error_log(0, |e| {
            errors.push((
                e.observation_count,
                e.first_observation_timestamp,
                e.last_observation_timestamp,
                e.error.to_string(),
            ))
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
        cnc.read_error_log(future.as_millis() as i64, |_| panic!("none"))
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
        cnc.read_error_log(0, |_| {}).unwrap() > 0
    });
    let panic = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        cnc.read_error_log(0, |_| panic!("consumer panic"))
    }))
    .expect_err("the panic propagates");
    assert_eq!(panic.downcast_ref::<&str>(), Some(&"consumer panic"));
    // The file is still usable.
    assert!(cnc.read_error_log(0, |_| {}).unwrap() > 0);
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
    let reader = cnc.counters_reader();
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

#[test]
fn driver_liveness_and_constants() {
    let driver = TestDriver::start();
    let cnc = CncFile::map_existing(&driver.dir).unwrap();
    assert!(cnc.file_name().ends_with("cnc.dat"), "{}", cnc.file_name());
    wait_until("the driver heartbeat", || {
        cnc.is_driver_active(Duration::from_secs(10))
    });
    let constants = cnc.constants().unwrap();
    assert_eq!(constants.pid, i64::from(std::process::id()));
    assert!(constants.start_timestamp > 0);
    assert!(constants.client_liveness_timeout > Duration::ZERO);
    assert!(constants.counter_values_buffer_length > 0);
    assert_eq!(
        cnc.counters_reader().max_counter_id() as usize + 1,
        constants.counter_values_buffer_length / 128
    );

    // The mapping outlives the driver, which stops heartbeating (a clean
    // shutdown clears the heartbeat).
    drop(driver);
    std::thread::sleep(Duration::from_millis(50));
    assert!(!cnc.is_driver_active(Duration::from_secs(60)));
}

#[test]
fn map_without_waiting() {
    let dir = std::env::temp_dir().join(format!("aeron-glide-no-cnc-{}", std::process::id()));
    let start = Instant::now();
    let err = CncFile::map_existing_with_timeout(dir.to_str().unwrap(), Duration::ZERO)
        .expect_err("no driver");
    assert_eq!(err.kind(), ErrorKind::Io, "{err}");
    assert!(start.elapsed() < Duration::from_secs(1));
}

#[test]
fn rejects_nul_and_corrupt_files() {
    let driver = TestDriver::start();
    let err = CncFile::map_existing(&format!("{}\0/elsewhere", driver.dir)).expect_err("NUL");
    assert_eq!(err.kind(), ErrorKind::IllegalArgument, "{err}");

    // A CnC file whose buffer lengths wrap around when summed as size_t.
    let dir = std::env::temp_dir().join(format!("aeron-glide-bad-cnc-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let mut file = vec![0u8; 4096];
    let version: i32 = 2 << 8; // 0.2.0, the CnC version of this Aeron release
    file[0..4].copy_from_slice(&version.to_le_bytes());
    file[4..8].copy_from_slice(&(-(1i32 << 30)).to_le_bytes()); // to-driver buffer
    file[16..20].copy_from_slice(&(1i32 << 30).to_le_bytes()); // counter values
    std::fs::write(dir.join("cnc.dat"), &file).unwrap();
    let result = CncFile::map_existing_with_timeout(dir.to_str().unwrap(), Duration::ZERO);
    let _ = std::fs::remove_dir_all(&dir);
    assert_eq!(result.expect_err("corrupt").kind(), ErrorKind::Io);
}
