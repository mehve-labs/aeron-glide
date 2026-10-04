//! The embedded media driver ([`MediaDriver`]).

use super::*;

#[cxx::bridge(namespace = "aeron_rs")]
pub(crate) mod ffi {
    unsafe extern "C++" {
        include!("driver_shim.h");

        type MediaDriverWrapper;

        fn create_media_driver() -> Result<UniquePtr<MediaDriverWrapper>>;
        fn start(self: Pin<&mut MediaDriverWrapper>, manual_main_loop: bool) -> Result<()>;
        fn doWork(self: &MediaDriverWrapper) -> Result<i32>;
        fn idle(self: &MediaDriverWrapper, work_count: i32) -> Result<()>;
        fn setTerminationValidator(
            self: Pin<&mut MediaDriverWrapper>,
            validator: fn(usize, &[u8]) -> bool,
            release: fn(usize),
            ctx: usize,
        ) -> Result<()>;
        fn setTerminationHook(
            self: Pin<&mut MediaDriverWrapper>,
            hook: fn(usize),
            release: fn(usize),
            ctx: usize,
        ) -> Result<()>;

        fn setThreadingMode(self: Pin<&mut MediaDriverWrapper>, mode: i32) -> Result<()>;
        fn setDir(self: Pin<&mut MediaDriverWrapper>, dir: &str) -> Result<()>;
    }
}

/// Threading model for the embedded media driver.
#[repr(i32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum ThreadingMode {
    /// Separate threads for conductor, sender, and receiver.
    Dedicated = 0,
    /// Sender and receiver share a thread; conductor is separate.
    SharedNetwork = 1,
    /// All three run on a single shared thread.
    Shared = 2,
    /// No threads: the application runs the driver's duty cycle with
    /// [`MediaDriver::do_work`].
    Invoker = 3,
}

/// Idle strategy for the media driver's threads, by Aeron's name for it
/// (`AERON_*_IDLE_STRATEGY`). Not to be confused with the
/// [`concurrent::IdleStrategy`](crate::concurrent::IdleStrategy) trait for your
/// own duty cycles.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum DriverIdleStrategy {
    /// Progressive back-off: spin → yield → park.
    Backoff,
    /// Busy spin (lowest latency, highest CPU).
    Spin,
    /// Thread yield.
    Yield,
    /// Thread sleep.
    Sleeping,
    /// No-op (do nothing between duty cycles).
    Noop,
}

impl ThreadingMode {
    pub(crate) fn from_c(value: i32) -> Self {
        match value {
            1 => Self::SharedNetwork,
            2 => Self::Shared,
            3 => Self::Invoker,
            _ => Self::Dedicated,
        }
    }
}

impl DriverIdleStrategy {
    /// Aeron's name for the strategy, as the driver settings take it.
    pub fn as_str(&self) -> &'static str {
        match self {
            DriverIdleStrategy::Backoff => "backoff",
            DriverIdleStrategy::Spin => "spin",
            DriverIdleStrategy::Yield => "yield",
            DriverIdleStrategy::Sleeping => "sleeping",
            DriverIdleStrategy::Noop => "noop",
        }
    }
}

/// An embedded C media driver that manages shared memory buffers and handles
/// publication/subscription matching.
///
/// Configure it with [`MediaDriver::builder`]; once started it can no longer be
/// reconfigured. The driver shuts down when dropped.
///
/// ```no_run
/// use aeron_glide::{MediaDriver, ThreadingMode};
///
/// let driver = MediaDriver::builder()
///     .dir("/dev/shm/my-app")
///     .dir_delete_on_start(true)
///     .threading_mode(ThreadingMode::Shared)
///     .start()?;
/// println!("driver running in {}", driver.dir());
/// # Ok::<(), aeron_glide::Error>(())
/// ```
///
/// With [`ThreadingMode::Invoker`] the driver has no threads: run its duty
/// cycle on your own thread with [`do_work`](Self::do_work) (and
/// [`idle`](Self::idle) between cycles).
pub struct MediaDriver {
    pub(crate) inner: cxx::UniquePtr<ffi::MediaDriverWrapper>,
    /// Serialises the duty cycle of a driver in invoker mode.
    pub(crate) duty_cycle: std::sync::Mutex<()>,
}

// SAFETY: a started driver runs on its own threads (or, in invoker mode, on the
// thread calling `do_work`, which the `duty_cycle` mutex serialises). The handle
// otherwise only reads the driver's context, which is immutable once started,
// and closes the driver on drop, which the C driver allows from any thread except
// its own conductor (a drop inside a driver handler is moved to another thread).
unsafe impl Send for MediaDriver {}
unsafe impl Sync for MediaDriver {}

impl Drop for MediaDriver {
    fn drop(&mut self) {
        // Inside a termination handler (on the conductor thread in threaded
        // modes), closing the driver would join the thread it runs on.
        callback::drop_outside_conductor(&mut self.inner);
    }
}

impl std::fmt::Debug for MediaDriver {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MediaDriver")
            .field("dir", &self.dir())
            .finish_non_exhaustive()
    }
}

impl MediaDriver {
    /// Configure a new media driver.
    pub fn builder() -> MediaDriverBuilder {
        MediaDriverBuilder::new()
    }

    /// Start a media driver with default settings.
    pub fn launch() -> Result<Self> {
        Self::builder().start()
    }

    /// Run one duty cycle of a driver started with [`ThreadingMode::Invoker`]
    /// (C `aeron_driver_main_do_work`): its conductor, sender and receiver
    /// work. Returns the amount of work done. Call it regularly, e.g. alongside
    /// [`AeronClient::invoke`](crate::AeronClient::invoke) for a client in agent
    /// invoker mode.
    ///
    /// Fails with [`ErrorKind::IllegalState`] for a driver in another threading
    /// mode, or while another call runs (e.g. from a termination handler).
    ///
    /// Clients can connect while nobody runs the driver, but their requests
    /// time out, and after the driver timeout they no longer connect.
    pub fn do_work(&self) -> Result<usize> {
        let _cycle = self.cycle()?;
        Ok(crate::error::count(self.inner.doWork()?))
    }

    /// Idle after a duty cycle with the driver's shared idle strategy (C
    /// `aeron_driver_main_idle_strategy`): returns at once if `work_count` is
    /// positive, otherwise backs off. Same failures as [`do_work`](Self::do_work).
    pub fn idle(&self, work_count: usize) -> Result<()> {
        let _cycle = self.cycle()?;
        Ok(self.inner.idle(crate::error::ffi_limit(work_count))?)
    }

    fn cycle(&self) -> Result<std::sync::MutexGuard<'_, ()>> {
        match self.duty_cycle.try_lock() {
            Ok(guard) => Ok(guard),
            Err(std::sync::TryLockError::Poisoned(e)) => Ok(e.into_inner()),
            Err(std::sync::TryLockError::WouldBlock) => Err(Error::new(
                ErrorKind::IllegalState,
                "the driver's duty cycle is already running: run it from one thread",
            )),
        }
    }
}

/// Configuration for a [`MediaDriver`], created by [`MediaDriver::builder`].
///
/// Setters can be chained; the first invalid setting is reported by
/// [`start`](Self::start).
pub struct MediaDriverBuilder {
    inner: Result<cxx::UniquePtr<ffi::MediaDriverWrapper>>,
}

// SAFETY: the builder owns an unshared driver context that no thread uses until
// `start`, so it may be configured on one thread and started on another.
unsafe impl Send for MediaDriverBuilder {}

impl Default for MediaDriverBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for MediaDriverBuilder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MediaDriverBuilder")
            .field(
                "threading_mode",
                &self.inner.as_ref().ok().map(|w| {
                    ThreadingMode::from_c(crate::driver_gen::ffi::driver_get_threading_mode(w))
                }),
            )
            .field("error", &self.inner.as_ref().err())
            .finish_non_exhaustive()
    }
}

impl MediaDriverBuilder {
    /// A builder with Aeron's defaults (including `AERON_*` environment variables).
    pub fn new() -> Self {
        Self {
            inner: ffi::create_media_driver().map_err(Error::from),
        }
    }

    /// Apply a setting unless an earlier one already failed.
    pub(crate) fn apply(
        mut self,
        set: impl FnOnce(
            std::pin::Pin<&mut ffi::MediaDriverWrapper>,
        ) -> std::result::Result<(), cxx::Exception>,
    ) -> Self {
        if let Ok(inner) = &mut self.inner
            && let Err(e) = set(inner.pin_mut())
        {
            self.inner = Err(e.into());
        }
        self
    }

    /// Start the media driver. Fails with the first invalid setting, if any.
    ///
    /// With [`ThreadingMode::Invoker`] (set here or with `AERON_THREADING_MODE`)
    /// it starts no threads: run it with [`MediaDriver::do_work`]. The
    /// conductor's start-up (e.g. its CPU affinity) then runs on this thread.
    pub fn start(self) -> Result<MediaDriver> {
        let mut inner = self.inner?;
        let invoker =
            ThreadingMode::from_c(crate::driver_gen::ffi::driver_get_threading_mode(&inner))
                == ThreadingMode::Invoker;
        inner.pin_mut().start(invoker)?;
        Ok(MediaDriver {
            inner,
            duty_cycle: std::sync::Mutex::new(()),
        })
    }

    /// Decide whether a termination request (e.g. from
    /// [`Context::request_driver_termination`](crate::Context::request_driver_termination))
    /// is accepted: `validator` gets the request's token (C
    /// `aeron_driver_context_set_driver_termination_validator`). Without one,
    /// `AERON_DRIVER_TERMINATION_VALIDATOR` decides (rejecting by default).
    ///
    /// It runs on the driver's conductor thread (in invoker mode, the thread
    /// calling [`MediaDriver::do_work`]). A panic is caught and printed, and
    /// rejects the request.
    pub fn termination_validator<F>(self, validator: F) -> Self
    where
        F: Fn(&[u8]) -> bool + Send + Sync + 'static,
    {
        self.apply(move |w| {
            w.setTerminationValidator(
                termination_validator::<F>,
                crate::handlers::release::<F>,
                crate::handlers::into_ctx(validator),
            )
        })
    }

    /// Called when a termination request is accepted (C
    /// `aeron_driver_context_set_driver_termination_hook`), on the driver's
    /// conductor thread (in invoker mode, the thread calling
    /// [`MediaDriver::do_work`]): e.g. signal your program to drop the
    /// [`MediaDriver`] (the driver keeps running until then). Dropping it from
    /// the hook works too: the driver closes on another thread once the hook
    /// returns. A hook owning an `Arc<MediaDriver>` would keep the driver alive
    /// (a reference cycle): hold a `Weak` instead.
    pub fn termination_hook<F>(self, hook: F) -> Self
    where
        F: Fn() + Send + Sync + 'static,
    {
        self.apply(move |w| {
            w.setTerminationHook(
                termination_hook::<F>,
                crate::handlers::release::<F>,
                crate::handlers::into_ctx(hook),
            )
        })
    }

    /// The Aeron directory of the driver: clients connect through the CnC file
    /// in it (C `aeron_driver_context_set_dir`, environment variable
    /// `AERON_DIR`). Fails at [`start`](Self::start) if it is not valid UTF-8.
    pub fn dir(mut self, dir: impl AsRef<std::path::Path>) -> Self {
        match crate::error::path_str(dir.as_ref()) {
            Ok(dir) => self.apply(|w| w.setDir(dir)),
            Err(e) => {
                if self.inner.is_ok() {
                    self.inner = Err(e);
                }
                self
            }
        }
    }

    /// Threading model of the driver's conductor, sender and receiver.
    ///
    /// The other settings are generated from Aeron's `aeronmd.h`; see
    /// `scripts/gen_driver_context.py`.
    pub fn threading_mode(self, mode: ThreadingMode) -> Self {
        self.apply(|w| w.setThreadingMode(mode as i32))
    }
}

fn termination_validator<F: Fn(&[u8]) -> bool + Send + Sync + 'static>(
    ctx: usize,
    token: &[u8],
) -> bool {
    let mut accepted = false;
    crate::handlers::invoke::<F>(ctx, "termination validator", |f| accepted = f(token));
    accepted
}

fn termination_hook<F: Fn() + Send + Sync + 'static>(ctx: usize) {
    crate::handlers::invoke::<F>(ctx, "termination hook", |f| f());
}
