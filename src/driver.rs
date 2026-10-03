//! The embedded media driver ([`MediaDriver`]).

use super::*;

/// Threading model for the embedded media driver.
#[repr(i32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThreadingMode {
    /// Separate threads for conductor, sender, and receiver.
    Dedicated = 0,
    /// Sender and receiver share a thread; conductor is separate.
    SharedNetwork = 1,
    /// All three run on a single shared thread.
    Shared = 2,
    /// Caller-driven — the application invokes the driver duty cycle.
    /// Not supported yet by [`MediaDriverBuilder::start`].
    Invoker = 3,
}

/// Idle strategy for media driver threads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IdleStrategy {
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

impl IdleStrategy {
    pub fn as_str(&self) -> &'static str {
        match self {
            IdleStrategy::Backoff => "backoff",
            IdleStrategy::Spin => "spin",
            IdleStrategy::Yield => "yield",
            IdleStrategy::Sleeping => "sleeping",
            IdleStrategy::Noop => "noop",
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

// SAFETY: a started driver runs on its own threads; the Rust handle only reads
// the immutable directory name and closes the driver on drop, which the C driver
// allows from any thread.
unsafe impl Send for MediaDriver {}
unsafe impl Sync for MediaDriver {}

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
    pub fn do_work(&self) -> Result<i32> {
        let _cycle = self.cycle()?;
        Ok(self.inner.doWork()?)
    }

    /// Idle after a duty cycle with the driver's shared idle strategy (C
    /// `aeron_driver_main_idle_strategy`): returns at once if `work_count` is
    /// positive, otherwise backs off. Same failures as [`do_work`](Self::do_work).
    pub fn idle(&self, work_count: i32) -> Result<()> {
        let _cycle = self.cycle()?;
        Ok(self.inner.idle(work_count)?)
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
    threading_mode: ThreadingMode,
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
            .field("threading_mode", &self.threading_mode)
            .field("error", &self.inner.as_ref().err())
            .finish_non_exhaustive()
    }
}

impl MediaDriverBuilder {
    /// A builder with Aeron's defaults (including `AERON_*` environment variables).
    pub fn new() -> Self {
        Self {
            inner: ffi::create_media_driver().map_err(Error::from),
            threading_mode: ThreadingMode::Dedicated,
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
    /// With [`ThreadingMode::Invoker`] it starts no threads: run it with
    /// [`MediaDriver::do_work`].
    pub fn start(self) -> Result<MediaDriver> {
        let mut inner = self.inner?;
        inner
            .pin_mut()
            .start(self.threading_mode == ThreadingMode::Invoker)?;
        Ok(MediaDriver {
            inner,
            duty_cycle: std::sync::Mutex::new(()),
        })
    }

    /// Decide whether a termination request (e.g. from
    /// [`Context::request_driver_termination`](crate::Context::request_driver_termination))
    /// is accepted: `validator` gets the request's token (C
    /// `aeron_driver_context_set_driver_termination_validator`). The default
    /// rejects every request.
    ///
    /// It runs on the driver's conductor thread. A panic is caught and printed,
    /// and rejects the request.
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
    /// conductor thread: e.g. signal your program to drop the [`MediaDriver`]
    /// (the driver keeps running until then). Dropping the driver from the
    /// hook itself is not possible, as the hook runs inside it.
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

    /// Threading model of the driver's conductor, sender and receiver.
    ///
    /// The other settings are generated from Aeron's `aeronmd.h`; see
    /// `scripts/gen_driver_context.py`.
    pub fn threading_mode(mut self, mode: ThreadingMode) -> Self {
        self.threading_mode = mode;
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
