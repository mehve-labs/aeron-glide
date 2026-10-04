//! Idle strategies and agents (C++ `aeron::concurrent`): run a duty cycle on a
//! dedicated thread ([`AgentRunner`]) or on your own ([`AgentInvoker`]), backing
//! off with an [`IdleStrategy`] when there is no work.
//!
//! These are Rust ports of the header-only C++ templates. A client and a media
//! driver in agent invoker mode can be run as agents ([`ClientAgent`],
//! [`MediaDriverAgent`]):
//!
//! ```no_run
//! use aeron_glide::concurrent::{AgentRunner, BackoffIdleStrategy, ClientAgent};
//! use aeron_glide::{AeronClient, Context};
//! use std::sync::Arc;
//!
//! let client = Arc::new(AeronClient::connect(Context::new().use_conductor_agent_invoker(true))?);
//! // Run the client's conductor on a thread of our own, backing off when idle.
//! let runner = AgentRunner::start(
//!     "client-conductor",
//!     ClientAgent::new(client.clone()),
//!     BackoffIdleStrategy::default(),
//!     |e| eprintln!("conductor: {e}"),
//! )?;
//! // ... use the client ...
//! runner.close();
//! # Ok::<(), aeron_glide::Error>(())
//! ```

#![cfg_attr(
    not(feature = "driver"),
    doc = "[`MediaDriverAgent`]: https://docs.rs/aeron-glide/latest/aeron_glide/concurrent/struct.MediaDriverAgent.html"
)]

use crate::{AeronClient, Error, ErrorKind, Result};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::JoinHandle;
use std::time::Duration;

/// How a duty cycle waits when it did no work (C++ `IdleStrategy`).
pub trait IdleStrategy {
    /// Back off if `work_count` is 0 (no work was done); reset otherwise.
    fn idle(&mut self, work_count: usize) {
        if work_count > 0 {
            self.reset();
        } else {
            self.idle_now();
        }
    }

    /// Back off once, whatever the work done (C++ `idle()`).
    fn idle_now(&mut self);

    /// Forget any backoff progress (C++ `reset()`).
    fn reset(&mut self) {}
}

/// Spin, hinting the CPU (C++ `BusySpinIdleStrategy`): lowest latency, one busy core.
#[derive(Debug, Clone, Copy, Default)]
pub struct BusySpinIdleStrategy;

impl IdleStrategy for BusySpinIdleStrategy {
    fn idle_now(&mut self) {
        std::hint::spin_loop();
    }
}

/// Don't wait at all (C++ `NoOpIdleStrategy`).
#[derive(Debug, Clone, Copy, Default)]
pub struct NoOpIdleStrategy;

impl IdleStrategy for NoOpIdleStrategy {
    fn idle_now(&mut self) {}
}

/// Yield the thread (C++ `YieldingIdleStrategy`).
#[derive(Debug, Clone, Copy, Default)]
pub struct YieldingIdleStrategy;

impl IdleStrategy for YieldingIdleStrategy {
    fn idle_now(&mut self) {
        std::thread::yield_now();
    }
}

/// Sleep for a fixed period (C++ `SleepingIdleStrategy`).
#[derive(Debug, Clone, Copy)]
pub struct SleepingIdleStrategy {
    period: Duration,
}

impl SleepingIdleStrategy {
    /// Sleep for `period` when idle.
    pub fn new(period: Duration) -> Self {
        Self { period }
    }
}

impl IdleStrategy for SleepingIdleStrategy {
    fn idle(&mut self, work_count: usize) {
        if work_count == 0 {
            self.idle_now();
        }
    }

    fn idle_now(&mut self) {
        std::thread::sleep(self.period);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BackoffState {
    NotIdle,
    Spinning,
    Yielding,
    Parking,
}

/// Spin, then yield, then sleep for exponentially longer periods (C++
/// `BackoffIdleStrategy`, the default idle strategy of Aeron's agents).
#[derive(Debug, Clone)]
#[repr(align(64))] // keep the hot state off other cache lines, as the C++ padding does
pub struct BackoffIdleStrategy {
    max_spins: u64,
    max_yields: u64,
    min_park: Duration,
    max_park: Duration,
    spins: u64,
    yields: u64,
    park: Duration,
    state: BackoffState,
}

impl Default for BackoffIdleStrategy {
    /// C++'s defaults: 10 spins, 20 yields, then sleeps from 1 µs up to 1 ms.
    fn default() -> Self {
        Self::new(10, 20, Duration::from_micros(1), Duration::from_millis(1))
    }
}

impl BackoffIdleStrategy {
    /// Spin up to `max_spins` times, then yield up to `max_yields` times, then
    /// sleep from `min_park`, doubling up to `max_park`.
    pub fn new(max_spins: u64, max_yields: u64, min_park: Duration, max_park: Duration) -> Self {
        Self {
            max_spins,
            max_yields,
            min_park,
            max_park: max_park.max(min_park),
            spins: 0,
            yields: 0,
            park: min_park,
            state: BackoffState::NotIdle,
        }
    }
}

impl IdleStrategy for BackoffIdleStrategy {
    fn idle_now(&mut self) {
        match self.state {
            BackoffState::NotIdle => {
                self.state = BackoffState::Spinning;
                self.spins += 1;
            }
            BackoffState::Spinning => {
                std::hint::spin_loop();
                self.spins += 1;
                if self.spins > self.max_spins {
                    self.state = BackoffState::Yielding;
                    self.yields = 0;
                }
            }
            BackoffState::Yielding => {
                self.yields += 1;
                if self.yields > self.max_yields {
                    self.state = BackoffState::Parking;
                    self.park = self.min_park;
                } else {
                    std::thread::yield_now();
                }
            }
            BackoffState::Parking => {
                std::thread::sleep(self.park);
                self.park = self.park.saturating_mul(2).min(self.max_park);
            }
        }
    }

    fn reset(&mut self) {
        self.spins = 0;
        self.yields = 0;
        self.park = self.min_park;
        self.state = BackoffState::NotIdle;
    }
}

impl<T: IdleStrategy + ?Sized> IdleStrategy for Box<T> {
    fn idle(&mut self, work_count: usize) {
        (**self).idle(work_count)
    }

    fn idle_now(&mut self) {
        (**self).idle_now()
    }

    fn reset(&mut self) {
        (**self).reset()
    }
}

/// A duty cycle run by an [`AgentRunner`] or [`AgentInvoker`] (C++ agents).
///
/// Errors are passed to the runner's error handler and the agent keeps
/// running, except an [`ErrorKind::AgentTermination`] error (see
/// [`Error::agent_termination`]), which stops it.
pub trait Agent {
    /// Called once before the first duty cycle. An error stops the agent.
    fn on_start(&mut self) -> Result<()> {
        Ok(())
    }

    /// One duty cycle: returns the amount of work done (0 to idle).
    fn do_work(&mut self) -> Result<usize>;

    /// Called once when the agent stops.
    fn on_close(&mut self) -> Result<()> {
        Ok(())
    }
}

impl<A: Agent + ?Sized> Agent for Box<A> {
    fn on_start(&mut self) -> Result<()> {
        (**self).on_start()
    }

    fn do_work(&mut self) -> Result<usize> {
        (**self).do_work()
    }

    fn on_close(&mut self) -> Result<()> {
        (**self).on_close()
    }
}

/// A client in agent invoker mode as an agent: its duty cycle is
/// [`AeronClient::invoke`]. Starting it fails unless the client uses an agent
/// invoker ([`Context::use_conductor_agent_invoker`](crate::Context::use_conductor_agent_invoker)).
#[derive(Debug, Clone)]
pub struct ClientAgent(pub Arc<AeronClient>);

impl ClientAgent {
    /// Run `client`'s conductor as an agent.
    pub fn new(client: Arc<AeronClient>) -> Self {
        Self(client)
    }
}

impl Agent for ClientAgent {
    fn on_start(&mut self) -> Result<()> {
        if !self.0.uses_agent_invoker() {
            return Err(Error::new(
                ErrorKind::IllegalState,
                "the client does not use an agent invoker",
            ));
        }
        Ok(())
    }

    fn do_work(&mut self) -> Result<usize> {
        self.0.invoke()
    }
}

/// A media driver in invoker mode as an agent: its duty cycle is
/// [`MediaDriver::do_work`](crate::MediaDriver::do_work). Starting it fails
/// unless the driver runs in [`ThreadingMode::Invoker`](crate::ThreadingMode::Invoker).
#[cfg(feature = "driver")]
#[cfg_attr(docsrs, doc(cfg(feature = "driver")))]
#[derive(Debug, Clone)]
pub struct MediaDriverAgent(pub Arc<crate::MediaDriver>);

#[cfg(feature = "driver")]
impl MediaDriverAgent {
    /// Run `driver`'s duty cycle as an agent.
    pub fn new(driver: Arc<crate::MediaDriver>) -> Self {
        Self(driver)
    }
}

#[cfg(feature = "driver")]
impl Agent for MediaDriverAgent {
    fn on_start(&mut self) -> Result<()> {
        if self.0.threading_mode() != crate::ThreadingMode::Invoker {
            return Err(Error::new(
                ErrorKind::IllegalState,
                "the media driver does not run in ThreadingMode::Invoker",
            ));
        }
        Ok(())
    }

    fn do_work(&mut self) -> Result<usize> {
        self.0.do_work()
    }
}

/// Report an error unless it is an agent asking to stop.
fn report<H: FnMut(&Error)>(error: &Error, handler: &mut H) {
    if error.kind() != ErrorKind::AgentTermination {
        handler(error);
    }
}

/// Runs an agent's duty cycle on the caller's thread (C++ `AgentInvoker`).
#[derive(Debug)]
pub struct AgentInvoker<A, H> {
    agent: A,
    error_handler: H,
    started: bool,
    running: bool,
    closed: bool,
}

impl<A: Agent, H: FnMut(&Error)> AgentInvoker<A, H> {
    /// An invoker for `agent`, reporting its errors to `error_handler`.
    pub fn new(agent: A, error_handler: H) -> Self {
        Self {
            agent,
            error_handler,
            started: false,
            running: false,
            closed: false,
        }
    }

    /// Start the agent ([`Agent::on_start`]) if it is not started yet. If that
    /// fails, the error is reported (unless it is an
    /// [`AgentTermination`](ErrorKind::AgentTermination)) and the invoker closes.
    pub fn start(&mut self) {
        if self.started {
            return;
        }
        self.started = true;
        match self.agent.on_start() {
            Ok(()) => self.running = true,
            Err(e) => {
                report(&e, &mut self.error_handler);
                self.close();
            }
        }
    }

    /// Run one duty cycle if the agent is running; returns the work done (0
    /// otherwise). Errors are reported; an
    /// [`AgentTermination`](ErrorKind::AgentTermination) error closes the invoker
    /// without being reported (C++ `AgentInvoker` has no termination: it reports
    /// and keeps running).
    pub fn invoke(&mut self) -> usize {
        if !self.running {
            return 0;
        }
        match self.agent.do_work() {
            Ok(work) => work,
            Err(e) if e.kind() == ErrorKind::AgentTermination => {
                self.close();
                0
            }
            Err(e) => {
                (self.error_handler)(&e);
                0
            }
        }
    }

    /// Stop the agent ([`Agent::on_close`]) if it is not closed yet.
    pub fn close(&mut self) {
        if self.closed {
            return;
        }
        self.running = false;
        self.closed = true;
        if let Err(e) = self.agent.on_close() {
            report(&e, &mut self.error_handler);
        }
    }

    /// Returns `true` once [`start`](Self::start) was called.
    pub fn is_started(&self) -> bool {
        self.started
    }

    /// Returns `true` while the agent runs.
    pub fn is_running(&self) -> bool {
        self.running
    }

    /// Returns `true` once the agent is closed.
    pub fn is_closed(&self) -> bool {
        self.closed
    }

    /// The agent.
    pub fn agent(&self) -> &A {
        &self.agent
    }

    /// The agent, mutably.
    pub fn agent_mut(&mut self) -> &mut A {
        &mut self.agent
    }
}

/// Runs an agent's duty cycle on a dedicated thread, idling with an
/// [`IdleStrategy`] (C++ `AgentRunner`). [`close`](Self::close) (or dropping
/// the runner) stops the agent and joins the thread.
///
/// A panic in the agent stops it; `close` then resumes the panic.
#[derive(Debug)]
pub struct AgentRunner<A> {
    name: String,
    running: Arc<AtomicBool>,
    thread: Option<JoinHandle<A>>,
}

impl<A: Agent + Send + 'static> AgentRunner<A> {
    /// Start `agent` on a new thread named `name`: [`Agent::on_start`], then
    /// duty cycles idling with `idle_strategy`, until closed (or the agent
    /// returns an [`AgentTermination`](ErrorKind::AgentTermination) error), then
    /// [`Agent::on_close`]. Other errors go to `error_handler`, and the runner
    /// then idles as if no work was done.
    ///
    /// Fails if `name` contains a NUL or the thread cannot be spawned.
    pub fn start<I, H>(
        name: &str,
        agent: A,
        mut idle_strategy: I,
        mut error_handler: H,
    ) -> Result<Self>
    where
        I: IdleStrategy + Send + 'static,
        H: FnMut(&Error) + Send + 'static,
    {
        if name.contains('\0') {
            return Err(Error::new(
                ErrorKind::IllegalArgument,
                "an agent runner's name cannot contain a NUL",
            ));
        }
        let running = Arc::new(AtomicBool::new(true));
        let flag = running.clone();
        let thread = std::thread::Builder::new()
            .name(name.to_string())
            .spawn(move || {
                let mut agent = agent;
                if let Err(e) = agent.on_start() {
                    flag.store(false, Ordering::Release);
                    report(&e, &mut error_handler);
                }
                while flag.load(Ordering::Acquire) {
                    match agent.do_work() {
                        Ok(work) => idle_strategy.idle(work),
                        Err(e) if e.kind() == ErrorKind::AgentTermination => {
                            flag.store(false, Ordering::Release);
                        }
                        Err(e) => {
                            error_handler(&e);
                            idle_strategy.idle(0);
                        }
                    }
                }
                if let Err(e) = agent.on_close() {
                    report(&e, &mut error_handler);
                }
                agent
            })
            .map_err(|e| Error::new(ErrorKind::Other, format!("cannot start {name}: {e}")))?;
        Ok(Self {
            name: name.to_string(),
            running,
            thread: Some(thread),
        })
    }
}

impl<A> AgentRunner<A> {
    /// The thread's name.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Returns `true` while the agent runs (until it is closed or terminates).
    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::Acquire)
            && self.thread.as_ref().is_some_and(|t| !t.is_finished())
    }

    /// Stop the agent and wait for its thread; returns the agent. If the agent
    /// panicked, the panic is resumed here.
    pub fn close(mut self) -> A {
        match self.join() {
            Ok(agent) => agent,
            Err(panic) => std::panic::resume_unwind(panic),
        }
    }

    fn join(&mut self) -> std::thread::Result<A> {
        self.running.store(false, Ordering::Release);
        self.thread
            .take()
            .expect("the runner's thread is joined once")
            .join()
    }
}

impl<A> Drop for AgentRunner<A> {
    fn drop(&mut self) {
        if self.thread.is_some()
            && let Err(panic) = self.join()
            && !std::thread::panicking()
        {
            std::panic::resume_unwind(panic);
        }
    }
}
