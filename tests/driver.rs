#![cfg(feature = "driver")]
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
    // Connecting needs the driver's heartbeat and synchronous requests need it
    // to answer: run it on another thread until the client is up.
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

#[test]
fn invoker_mode_from_the_environment() {
    if !common::in_child_process(
        "invoker_mode_from_the_environment",
        &[("AERON_THREADING_MODE", "INVOKER")],
    ) {
        return;
    }
    let dir = temp_dir("env-invoker");
    let driver = MediaDriver::builder()
        .dir(&dir)
        .dir_delete_on_start(true)
        .dir_delete_on_shutdown(true)
        .start()
        .unwrap();
    assert_eq!(driver.threading_mode(), ThreadingMode::Invoker);
    driver.do_work().unwrap();
}

#[test]
fn termination_hook_may_drop_the_last_driver_handle() {
    let slot: Arc<std::sync::Mutex<Option<Arc<MediaDriver>>>> =
        Arc::new(std::sync::Mutex::new(None));
    let dropped = Arc::new(AtomicUsize::new(0));
    let (hook_slot, hook_dropped) = (slot.clone(), dropped.clone());
    let dir = temp_dir("hook-drop");
    let driver = MediaDriver::builder()
        .dir(&dir)
        .dir_delete_on_start(true)
        .dir_delete_on_shutdown(true)
        .threading_mode(ThreadingMode::Dedicated)
        .termination_validator(|_| true)
        .termination_hook(move || {
            // Drops the last handle on the conductor thread.
            drop(hook_slot.lock().unwrap().take());
            hook_dropped.fetch_add(1, Ordering::SeqCst);
        })
        .start()
        .unwrap();
    *slot.lock().unwrap() = Some(Arc::new(driver));
    assert!(Context::request_driver_termination(&dir, b"").unwrap());
    common::wait_until("the hook", || dropped.load(Ordering::SeqCst) == 1);
    // The driver closes (on another thread) and removes its directory.
    common::wait_until("the driver to close", || {
        !std::path::Path::new(&dir).exists()
    });
}

#[test]
fn huge_driver_timeouts_are_capped() {
    // The driver keeps these as signed nanoseconds and adds them to its clock:
    // uncapped, `u64::MAX` is -1 there and deadlines wrap into the past. Capped,
    // the driver still works.
    let dir = temp_dir("huge-timeouts");
    let driver = MediaDriver::builder()
        .dir(&dir)
        .dir_delete_on_start(true)
        .dir_delete_on_shutdown(true)
        // Explicit: another test sets AERON_THREADING_MODE=INVOKER for the process.
        .threading_mode(ThreadingMode::Shared)
        .image_liveness_timeout(Duration::MAX)
        .publication_linger_timeout(Duration::MAX)
        .untethered_window_limit_timeout(Duration::MAX)
        .start()
        .unwrap();
    assert_eq!(
        driver.image_liveness_timeout(),
        Duration::from_nanos((i64::MAX / 4) as u64)
    );
    let client = AeronClient::connect(Context::new().aeron_dir(&dir)).unwrap();
    let mut sub = client
        .add_subscription("aeron:udp?endpoint=localhost:0", 9)
        .unwrap();
    let endpoint = loop {
        if let Some(endpoint) = sub.resolved_endpoint().unwrap() {
            break endpoint;
        }
        std::thread::sleep(Duration::from_millis(1));
    };
    let publication = client
        .add_publication(&format!("aeron:udp?endpoint={endpoint}"), 9)
        .unwrap();
    common::wait_connected(&sub);
    for _ in 0..3 {
        common::offer(&publication, b"still alive");
        std::thread::sleep(Duration::from_millis(50));
    }
    common::poll_n(&mut sub, 3, |_| {});
}

/// CPU affinity settings are applied by the driver's threads as they start
/// (they used to be accepted and ignored).
#[cfg(target_os = "linux")]
#[test]
fn conductor_cpu_affinity_is_applied() {
    let dir = temp_dir("affinity");
    let _driver = MediaDriver::builder()
        .dir(&dir)
        .dir_delete_on_start(true)
        .dir_delete_on_shutdown(true)
        .threading_mode(ThreadingMode::Dedicated)
        .conductor_cpu_affinity(0)
        .start()
        .unwrap();
    // The conductor thread's allowed CPUs, from /proc.
    let conductor_cpus = || {
        std::fs::read_dir("/proc/self/task")
            .ok()?
            .flatten()
            .find_map(|task| {
                let name = std::fs::read_to_string(task.path().join("comm")).ok()?;
                // The driver's conductor (not this test's thread, also "conductor_...").
                if name.trim() != "conductor" {
                    return None;
                }
                let status = std::fs::read_to_string(task.path().join("status")).ok()?;
                status
                    .lines()
                    .find_map(|line| line.strip_prefix("Cpus_allowed_list:"))
                    .map(|cpus| cpus.trim().to_string())
            })
    };
    // The thread is named before it applies its affinity: wait for both.
    common::wait_until("the conductor thread on CPU 0", || {
        conductor_cpus().as_deref() == Some("0")
    });
}

/// SIGTERM (systemd, Kubernetes) stops the binary cleanly: the driver closes and
/// deletes its directory. It used to kill the process with the driver open.
#[cfg(unix)]
#[test]
fn mediadriver_binary_stops_cleanly_on_sigterm() {
    let dir = temp_dir("sigterm");
    let config = format!("{dir}.yaml");
    std::fs::write(
        &config,
        format!("dir: {dir}\ndir_delete_on_start: true\ndir_delete_on_shutdown: true\n"),
    )
    .unwrap();
    let mut child = std::process::Command::new(env!("CARGO_BIN_EXE_mediadriver"))
        .arg(&config)
        .stdout(std::process::Stdio::null())
        .spawn()
        .unwrap();
    let cnc = format!("{dir}/cnc.dat");
    common::wait_until("the driver to start", || {
        std::path::Path::new(&cnc).exists()
    });
    let killed = std::process::Command::new("kill")
        .args(["-TERM", &child.id().to_string()])
        .status()
        .unwrap();
    assert!(killed.success());
    let deadline = Instant::now() + common::TIMEOUT;
    let status = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break status;
        }
        assert!(Instant::now() < deadline, "the driver did not stop");
        std::thread::sleep(Duration::from_millis(20));
    };
    let _ = std::fs::remove_file(&config);
    assert!(status.success(), "{status}");
    assert!(
        !std::path::Path::new(&dir).exists(),
        "the directory is deleted on shutdown"
    );
}

/// A termination request accepted by AERON_DRIVER_TERMINATION_VALIDATOR stops
/// the binary even without a configured token (it used to be accepted and
/// ignored).
#[test]
fn mediadriver_binary_stops_on_requests_the_environment_accepts() {
    let dir = temp_dir("env-validator");
    let config = format!("{dir}.yaml");
    std::fs::write(
        &config,
        format!("dir: {dir}\ndir_delete_on_start: true\ndir_delete_on_shutdown: true\n"),
    )
    .unwrap();
    let mut child = std::process::Command::new(env!("CARGO_BIN_EXE_mediadriver"))
        .arg(&config)
        .env("AERON_DRIVER_TERMINATION_VALIDATOR", "allow")
        .stdout(std::process::Stdio::null())
        .spawn()
        .unwrap();
    let cnc = format!("{dir}/cnc.dat");
    common::wait_until("the driver to start", || {
        std::path::Path::new(&cnc).exists()
    });
    let deadline = Instant::now() + common::TIMEOUT;
    let status = loop {
        // Retried: the driver must be ready to read the request.
        let _ = Context::request_driver_termination(&dir, b"");
        if let Some(status) = child.try_wait().unwrap() {
            break status;
        }
        assert!(Instant::now() < deadline, "the driver did not stop");
        std::thread::sleep(Duration::from_millis(50));
    };
    let _ = std::fs::remove_file(&config);
    assert!(status.success(), "{status}");
}

/// Init args reload the idle strategy in effect, including one chosen by its
/// environment variable (they used to reload Aeron's recorded "backoff",
/// failing here: backoff rejects these args, noop ignores them).
#[test]
fn idle_strategy_init_args_keep_the_strategy_from_the_environment() {
    if !common::in_child_process(
        "idle_strategy_init_args_keep_the_strategy_from_the_environment",
        &[("AERON_SENDER_IDLE_STRATEGY", "noop")],
    ) {
        return;
    }
    let dir = temp_dir("env-idle");
    MediaDriver::builder()
        .dir(&dir)
        .dir_delete_on_start(true)
        .dir_delete_on_shutdown(true)
        .sender_idle_strategy_init_args("not-backoff-args")
        .start()
        .unwrap();
    // A strategy set through the builder still takes precedence.
    let dir = temp_dir("builder-idle");
    let err = MediaDriver::builder()
        .dir(&dir)
        .dir_delete_on_start(true)
        .dir_delete_on_shutdown(true)
        .sender_idle_strategy(aeron_glide::DriverIdleStrategy::Backoff)
        .sender_idle_strategy_init_args("not-backoff-args")
        .start();
    assert!(err.is_err(), "backoff rejects these args");
}

/// Settings Aeron range-checks when they come from environment variables are
/// checked when set through the builder too (they used to reach the driver:
/// a group size of 0 feeds `log(0)`).
#[test]
fn driver_settings_are_range_checked() {
    type Builder = aeron_glide::MediaDriverBuilder;
    fn start(configure: fn(Builder) -> Builder) -> aeron_glide::Result<()> {
        let dir = temp_dir("ranges");
        configure(
            MediaDriver::builder()
                .dir(&dir)
                .dir_delete_on_start(true)
                .dir_delete_on_shutdown(true),
        )
        .start()
        .map(drop)
    }
    let invalid: [fn(Builder) -> Builder; 4] = [
        |b| b.nak_multicast_group_size(0),
        |b| b.nak_unicast_retry_delay_ratio(0),
        |b| b.client_liveness_timeout(Duration::from_nanos(999)),
        // Each in range, but their product overflows.
        |b| {
            b.nak_unicast_delay(Duration::from_nanos(1 << 40))
                .nak_unicast_retry_delay_ratio(1 << 40)
        },
    ];
    for configure in invalid {
        assert_eq!(
            start(configure).unwrap_err().kind(),
            ErrorKind::IllegalArgument
        );
    }
    start(|b| b.nak_multicast_group_size(1)).unwrap();
}

#[test]
fn modes_and_strategies_parse() {
    use aeron_glide::DriverIdleStrategy;
    assert_eq!(
        "SHARED_NETWORK".parse::<ThreadingMode>().unwrap(),
        ThreadingMode::SharedNetwork
    );
    assert_eq!(
        "invoker".parse::<ThreadingMode>().unwrap(),
        ThreadingMode::Invoker
    );
    for strategy in [
        DriverIdleStrategy::Backoff,
        DriverIdleStrategy::Spin,
        DriverIdleStrategy::Yield,
        DriverIdleStrategy::Sleeping,
        DriverIdleStrategy::Noop,
    ] {
        assert_eq!(
            strategy.as_str().parse::<DriverIdleStrategy>().unwrap(),
            strategy
        );
    }
    assert_eq!(
        "busy".parse::<DriverIdleStrategy>().unwrap_err().kind(),
        ErrorKind::IllegalArgument
    );
}

#[test]
fn fnmut_termination_hook_and_explicit_close() {
    let dir = temp_dir("fnmut-hook");
    let (tx, rx) = std::sync::mpsc::channel();
    let mut requests = 0;
    let driver = MediaDriver::builder()
        .dir(&dir)
        .dir_delete_on_start(true)
        .dir_delete_on_shutdown(true)
        .threading_mode(ThreadingMode::Shared)
        .termination_validator(|_| true)
        // FnMut: keeps its own count.
        .termination_hook(move || {
            requests += 1;
            tx.send(requests).ok();
        })
        .start()
        .unwrap();
    let deadline = Instant::now() + common::TIMEOUT;
    let count = loop {
        let _ = aeron_glide::Context::request_driver_termination(&dir, b"");
        if let Ok(count) = rx.recv_timeout(Duration::from_millis(50)) {
            break count;
        }
        assert!(Instant::now() < deadline, "no termination hook call");
    };
    assert_eq!(count, 1);
    driver.close().unwrap();
    assert!(
        !std::path::Path::new(&dir).exists(),
        "closed: the directory is deleted"
    );
}
