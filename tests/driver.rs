//! The media driver's invoker mode and termination requests.

mod common;

use aeron_glide::{AeronClient, Context, ErrorKind, MediaDriver, ThreadingMode};
use common::TestDriver;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

fn temp_dir(name: &str) -> String {
    std::env::temp_dir()
        .join(format!("aeron-glide-{name}-{}", std::process::id()))
        .to_string_lossy()
        .into_owned()
}

#[test]
fn invoker_driver_and_client_on_one_thread() {
    let dir = temp_dir("invoker-driver");
    let driver = MediaDriver::builder()
        .dir(&dir)
        .dir_delete_on_start(true)
        .dir_delete_on_shutdown(true)
        .threading_mode(ThreadingMode::Invoker)
        .start()
        .unwrap();
    // Connecting waits for the driver, which only runs when invoked: run it on
    // another thread until the client is up.
    let running = std::sync::atomic::AtomicBool::new(true);
    let client = std::thread::scope(|s| {
        let driver = &driver;
        let running = &running;
        s.spawn(move || {
            while running.load(Ordering::Relaxed) {
                let work = driver.do_work().unwrap();
                driver.idle(work).unwrap();
            }
        });
        let client = AeronClient::connect(
            Context::new()
                .aeron_dir(&dir)
                .use_conductor_agent_invoker(true),
        )
        .unwrap();
        running.store(false, Ordering::Relaxed);
        client
    });

    // From here on, one thread runs everything.
    let cycle = || {
        driver.do_work().unwrap();
        client.invoke().unwrap();
    };
    let mut pending = client.add_subscription_async("aeron:ipc", 1).unwrap();
    let mut sub = loop {
        cycle();
        if let Some(sub) = pending.poll().unwrap() {
            break sub;
        }
    };
    let mut pending = client.add_publication_async("aeron:ipc", 1).unwrap();
    let publication = loop {
        cycle();
        if let Some(publication) = pending.poll().unwrap() {
            break publication;
        }
    };
    let deadline = Instant::now() + common::TIMEOUT;
    while !sub.is_connected() {
        assert!(Instant::now() < deadline, "not connected");
        cycle();
    }
    for i in 0..10 {
        let message = format!("message {i}");
        while publication.offer(message.as_bytes()).is_err() {
            assert!(Instant::now() < deadline, "offer");
            cycle();
        }
    }
    let mut received = 0;
    while received < 10 {
        assert!(Instant::now() < deadline, "poll");
        cycle();
        received += sub.poll(10, |_, _| {}).unwrap();
    }
}

#[test]
fn duty_cycle_needs_invoker_mode() {
    let driver = TestDriver::start();
    let err = driver.driver().do_work().unwrap_err();
    assert_eq!(err.kind(), ErrorKind::IllegalState, "{err}");
    assert_eq!(
        driver.driver().idle(0).unwrap_err().kind(),
        ErrorKind::IllegalState
    );
}

#[test]
fn termination_requests() {
    let accepted = Arc::new(AtomicUsize::new(0));
    let requests = Arc::new(AtomicUsize::new(0));
    let (hook, seen) = (accepted.clone(), requests.clone());
    let driver = TestDriver::start_with(move |b| {
        b.termination_validator(move |token| {
            seen.fetch_add(1, Ordering::SeqCst);
            match token {
                b"panic" => panic!("validator panic is contained"),
                token => token == b"let me stop",
            }
        })
        .termination_hook(move || {
            hook.fetch_add(1, Ordering::SeqCst);
        })
    });
    let wait = |count: usize| {
        common::wait_until("the request", || requests.load(Ordering::SeqCst) == count);
        std::thread::sleep(Duration::from_millis(20));
    };
    assert!(Context::request_driver_termination(&driver.dir, b"wrong").unwrap());
    wait(1);
    assert_eq!(accepted.load(Ordering::SeqCst), 0);
    assert!(Context::request_driver_termination(&driver.dir, b"panic").unwrap());
    wait(2);
    assert_eq!(accepted.load(Ordering::SeqCst), 0);
    assert!(Context::request_driver_termination(&driver.dir, b"let me stop").unwrap());
    wait(3);
    assert_eq!(accepted.load(Ordering::SeqCst), 1);
    // The driver keeps running until it is dropped.
    let client = driver.client();
    assert!(!client.is_closed());
}

#[test]
fn mediadriver_binary_runs_invoker_mode_and_terminates_on_request() {
    let dir = temp_dir("binary");
    let config = format!("{dir}.yaml");
    std::fs::write(
        &config,
        format!(
            "dir: {dir}\ndir_delete_on_start: true\ndir_delete_on_shutdown: true\n\
             threading_mode: invoker\ntermination_token: \"stop now\"\n"
        ),
    )
    .unwrap();
    let mut child = std::process::Command::new(env!("CARGO_BIN_EXE_mediadriver"))
        .arg(&config)
        .stdout(std::process::Stdio::null())
        .spawn()
        .unwrap();
    // A client connects through the binary's duty cycle.
    let deadline = Instant::now() + common::TIMEOUT;
    let client = loop {
        if let Ok(client) = AeronClient::connect(
            Context::new()
                .aeron_dir(&dir)
                .driver_timeout(Duration::from_secs(5)),
        ) {
            break client;
        }
        assert!(Instant::now() < deadline, "no driver");
        std::thread::sleep(Duration::from_millis(50));
    };
    client.add_publication("aeron:ipc", 1).unwrap();

    assert!(Context::request_driver_termination(&dir, b"wrong").unwrap());
    std::thread::sleep(Duration::from_millis(200));
    assert!(
        child.try_wait().unwrap().is_none(),
        "a wrong token is rejected"
    );
    assert!(Context::request_driver_termination(&dir, b"stop now").unwrap());
    let status = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break status;
        }
        assert!(Instant::now() < deadline, "the driver did not stop");
        std::thread::sleep(Duration::from_millis(20));
    };
    assert!(status.success(), "{status}");
    let _ = std::fs::remove_file(&config);
}
