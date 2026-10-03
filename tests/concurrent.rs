#![cfg(feature = "driver")]
//! Idle strategies, agents (runner and invoker), version and clocks.

mod common;

use aeron_glide::concurrent::{
    Agent, AgentInvoker, AgentRunner, BackoffIdleStrategy, BusySpinIdleStrategy, ClientAgent,
    IdleStrategy, MediaDriverAgent, NoOpIdleStrategy, SleepingIdleStrategy, YieldingIdleStrategy,
};
use aeron_glide::{AeronClient, Context, Error, ErrorKind, MediaDriver, Result, ThreadingMode};
use common::{offer, poll_n, wait_until};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

#[test]
fn idle_strategies() {
    let time = |strategy: &mut dyn IdleStrategy, work: i32, times: usize| {
        let start = Instant::now();
        for _ in 0..times {
            strategy.idle(work);
        }
        start.elapsed()
    };
    // Spinning, yielding and no-op strategies don't sleep.
    for strategy in [
        &mut BusySpinIdleStrategy as &mut dyn IdleStrategy,
        &mut NoOpIdleStrategy,
        &mut YieldingIdleStrategy,
    ] {
        assert!(time(strategy, 0, 1000) < Duration::from_secs(2));
    }
    let mut sleeping = SleepingIdleStrategy::new(Duration::from_millis(5));
    assert!(time(&mut sleeping, 0, 4) >= Duration::from_millis(20));
    assert!(
        time(&mut sleeping, 1, 100) < Duration::from_millis(5),
        "no sleep after work"
    );

    // Backoff: 10 spins and 20 yields are quick, then sleeps double up to the max.
    let mut backoff =
        BackoffIdleStrategy::new(10, 20, Duration::from_millis(1), Duration::from_millis(4));
    assert!(
        time(&mut backoff, 0, 32) < Duration::from_millis(50),
        "spins and yields don't sleep"
    );
    let parked = time(&mut backoff, 0, 4); // 1 + 2 + 4 + 4 ms
    assert!(parked >= Duration::from_millis(11), "{parked:?}");
    backoff.idle(1); // work resets it
    assert!(
        time(&mut backoff, 0, 32) < Duration::from_millis(50),
        "spins and yields don't sleep"
    );
    backoff.reset();
    let mut boxed: Box<dyn IdleStrategy> = Box::new(BackoffIdleStrategy::default());
    boxed.idle(0);
}

/// Counts its lifecycle; `fail_every` cycles fails, `stop_after` cycles terminates.
#[derive(Debug, Default)]
struct Counting {
    started: usize,
    cycles: usize,
    closed: usize,
    fail_start: bool,
    fail_every: Option<usize>,
    stop_after: Option<usize>,
    panic_after: Option<usize>,
}

impl Agent for Counting {
    fn on_start(&mut self) -> Result<()> {
        self.started += 1;
        if self.fail_start {
            return Err(Error::new(ErrorKind::IllegalState, "start failed"));
        }
        Ok(())
    }

    fn do_work(&mut self) -> Result<i32> {
        self.cycles += 1;
        if self.panic_after == Some(self.cycles) {
            panic!("agent panic");
        }
        if self.stop_after == Some(self.cycles) {
            return Err(Error::agent_termination());
        }
        if self
            .fail_every
            .is_some_and(|n| self.cycles.is_multiple_of(n))
        {
            return Err(Error::new(ErrorKind::Other, "cycle failed"));
        }
        Ok(1)
    }

    fn on_close(&mut self) -> Result<()> {
        self.closed += 1;
        Ok(())
    }
}

fn error_sink() -> (Arc<Mutex<Vec<String>>>, impl FnMut(&Error) + Send + 'static) {
    let errors = Arc::new(Mutex::new(Vec::new()));
    let sink = errors.clone();
    (errors, move |e: &Error| {
        sink.lock().unwrap().push(e.message().to_string())
    })
}

#[test]
fn agent_invoker_lifecycle() {
    let (errors, handler) = error_sink();
    let mut invoker = AgentInvoker::new(
        Counting {
            fail_every: Some(3),
            stop_after: Some(5),
            ..Counting::default()
        },
        handler,
    );
    assert_eq!(invoker.invoke(), 0, "not started");
    invoker.start();
    invoker.start();
    assert!(invoker.is_started() && invoker.is_running());
    assert_eq!(invoker.agent().started, 1);
    let work: i32 = (0..10).map(|_| invoker.invoke()).sum();
    assert_eq!(work, 3, "cycles 1, 2 and 4 worked; 3 failed; 5 terminated");
    assert!(invoker.is_closed() && !invoker.is_running());
    assert_eq!(invoker.agent().cycles, 5);
    assert_eq!(invoker.agent().closed, 1);
    invoker.close();
    assert_eq!(invoker.agent_mut().closed, 1, "closed once");
    assert_eq!(*errors.lock().unwrap(), ["cycle failed"]);

    // A failing start closes it.
    let (errors, handler) = error_sink();
    let mut invoker = AgentInvoker::new(
        Counting {
            fail_start: true,
            ..Counting::default()
        },
        handler,
    );
    invoker.start();
    assert!(invoker.is_closed());
    assert_eq!(invoker.invoke(), 0);
    assert_eq!(*errors.lock().unwrap(), ["start failed"]);
}

#[test]
fn agent_runner_lifecycle() {
    let (errors, handler) = error_sink();
    let runner = AgentRunner::start(
        "counting",
        Counting {
            fail_every: Some(1000),
            ..Counting::default()
        },
        NoOpIdleStrategy,
        handler,
    )
    .unwrap();
    assert_eq!(runner.name(), "counting");
    wait_until("some cycles", || !errors.lock().unwrap().is_empty());
    assert!(runner.is_running());
    let agent = runner.close();
    assert_eq!((agent.started, agent.closed), (1, 1));
    assert!(agent.cycles >= 1000);

    // An agent terminating itself stops the runner without an error.
    let (errors, handler) = error_sink();
    let runner = AgentRunner::start(
        "terminating",
        Counting {
            stop_after: Some(3),
            ..Counting::default()
        },
        YieldingIdleStrategy,
        handler,
    )
    .unwrap();
    wait_until("termination", || !runner.is_running());
    let agent = runner.close();
    assert_eq!((agent.cycles, agent.closed), (3, 1));
    assert!(errors.lock().unwrap().is_empty());

    // A failing start skips the duty cycle but still closes.
    let (errors, handler) = error_sink();
    let runner = AgentRunner::start(
        "failing",
        Counting {
            fail_start: true,
            ..Counting::default()
        },
        NoOpIdleStrategy,
        handler,
    )
    .unwrap();
    let agent = runner.close();
    assert_eq!((agent.cycles, agent.closed), (0, 1));
    assert_eq!(*errors.lock().unwrap(), ["start failed"]);

    // Dropping the runner stops the agent too.
    let closed = Arc::new(AtomicUsize::new(0));
    struct Flag(Arc<AtomicUsize>);
    impl Agent for Flag {
        fn do_work(&mut self) -> Result<i32> {
            Ok(0)
        }
        fn on_close(&mut self) -> Result<()> {
            self.0.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }
    }
    drop(
        AgentRunner::start(
            "dropped",
            Flag(closed.clone()),
            BackoffIdleStrategy::default(),
            |_| {},
        )
        .unwrap(),
    );
    assert_eq!(closed.load(Ordering::SeqCst), 1);
}

#[test]
fn agent_runner_resumes_panics_on_close() {
    let runner = AgentRunner::start(
        "panicking",
        Counting {
            panic_after: Some(2),
            ..Counting::default()
        },
        NoOpIdleStrategy,
        |_| {},
    )
    .unwrap();
    wait_until("the panic", || !runner.is_running());
    let panic = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| runner.close()))
        .expect_err("the panic is resumed");
    assert_eq!(panic.downcast_ref::<&str>(), Some(&"agent panic"));
}

#[test]
fn client_and_driver_as_agents() {
    let dir = std::env::temp_dir()
        .join(format!("aeron-glide-agents-{}", std::process::id()))
        .to_string_lossy()
        .into_owned();
    let driver = Arc::new(
        MediaDriver::builder()
            .dir(&dir)
            .dir_delete_on_start(true)
            .dir_delete_on_shutdown(true)
            .threading_mode(ThreadingMode::Invoker)
            .start()
            .unwrap(),
    );
    let driver_runner = AgentRunner::start(
        "driver",
        MediaDriverAgent::new(driver.clone()),
        BackoffIdleStrategy::default(),
        |e| panic!("driver: {e}"),
    )
    .unwrap();
    let client = Arc::new(
        AeronClient::connect(
            Context::new()
                .aeron_dir(&dir)
                .use_conductor_agent_invoker(true),
        )
        .unwrap(),
    );
    let client_runner = AgentRunner::start(
        "client",
        ClientAgent::new(client.clone()),
        BackoffIdleStrategy::default(),
        |e| panic!("client: {e}"),
    )
    .unwrap();

    // Synchronous adds invoke the client too; the conductor lock serialises them.
    let mut sub = client.add_subscription("aeron:ipc", 1).unwrap();
    let publication = client.add_publication("aeron:ipc", 1).unwrap();
    common::wait_connected(&sub);
    for i in 0..10 {
        offer(&publication, format!("{i}").as_bytes());
    }
    poll_n(&mut sub, 10, |_| {});

    drop((sub, publication));
    client_runner.close();
    drop(client);
    driver_runner.close();
}

#[test]
fn version_and_clocks() {
    let version = aeron_glide::aeron_version();
    assert!(version.contains("1.53.3"), "{version}");
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64;
    assert!((aeron_glide::epoch_clock() - now).abs() < 1000);
    let a = aeron_glide::nano_clock();
    std::thread::sleep(Duration::from_millis(2));
    let b = aeron_glide::nano_clock();
    assert!(b - a >= 2_000_000, "{a} {b}");
}

#[test]
fn agents_refuse_clients_and_drivers_not_in_invoker_mode() {
    let driver = common::TestDriver::start();
    let threaded = Arc::new(driver.client());
    let (errors, handler) = error_sink();
    let runner = AgentRunner::start(
        "threaded-client",
        ClientAgent::new(threaded),
        BusySpinIdleStrategy,
        handler,
    )
    .unwrap();
    let agent = runner.close();
    assert_eq!(errors.lock().unwrap().len(), 1, "one error, no flood");
    drop(agent);

    let threaded_driver = Arc::new(
        MediaDriver::builder()
            .dir(&format!("{}-threaded", driver.dir))
            .dir_delete_on_start(true)
            .dir_delete_on_shutdown(true)
            .threading_mode(ThreadingMode::Shared)
            .start()
            .unwrap(),
    );
    let (errors, handler) = error_sink();
    let mut invoker = AgentInvoker::new(MediaDriverAgent::new(threaded_driver), handler);
    invoker.start();
    assert!(invoker.is_closed());
    assert_eq!(errors.lock().unwrap().len(), 1);

    // A runner's name cannot contain a NUL.
    let err =
        AgentRunner::start("a\0b", Counting::default(), NoOpIdleStrategy, |_| {}).expect_err("NUL");
    assert_eq!(err.kind(), ErrorKind::IllegalArgument);
}
