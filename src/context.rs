//! Client configuration ([`Context`]).

use crate::handlers::{self, into_ctx, release};
use crate::{
    AeronClient, CounterEvent, Error, ImageEvent, NewPublication, NewSubscription,
    PublicationErrorFrame, Result, ffi,
};
use std::os::raw::c_long;
use std::pin::Pin;
use std::time::Duration;

/// Installs one handler on the C++ context; the handler type is known here, so
/// its trampoline is monomorphised and the closure needs no boxing.
type Installer = Box<dyn FnOnce(Pin<&mut ffi::ContextWrapper>) + Send>;

/// Configuration for an [`AeronClient`], mirroring the Aeron C++ `aeron::Context`.
///
/// Apart from `AERON_DIR`, the `AERON_*` client environment variables are not
/// applied: the Aeron C++ wrapper overwrites them with the values set here (or
/// its defaults).
///
/// ```no_run
/// use aeron_glide::{AeronClient, Context};
/// use std::time::Duration;
///
/// let client = AeronClient::connect(
///     Context::new()
///         .aeron_dir("/dev/shm/my-app")
///         .client_name("my-app")
///         .driver_timeout(Duration::from_secs(5))
///         .error_handler(|e| eprintln!("aeron: {e}")),
/// )?;
/// # Ok::<(), aeron_glide::Error>(())
/// ```
#[derive(Default)]
pub struct Context {
    aeron_dir: Option<String>,
    client_name: Option<String>,
    driver_timeout: Option<Duration>,
    resource_linger_timeout: Option<Duration>,
    idle_sleep_duration: Option<Duration>,
    pre_touch_mapped_memory: Option<bool>,
    use_conductor_agent_invoker: Option<bool>,
    handlers: Vec<Installer>,
}

impl Context {
    /// A context with Aeron's defaults.
    pub fn new() -> Self {
        Self::default()
    }

    /// Aeron's default directory on this platform (e.g. `/dev/shm/aeron-<user>`
    /// on Linux), used when neither [`aeron_dir`](Self::aeron_dir) nor `AERON_DIR`
    /// is set.
    pub fn default_aeron_path() -> Result<String> {
        Ok(ffi::defaultAeronPath()?)
    }

    /// Ask the media driver running in `aeron_dir` to terminate, presenting
    /// `token` to its termination validator. Returns `true` if the request was
    /// sent, `false` if the driver has not initialised its CnC file yet; fails if
    /// there is no CnC file (no driver) in that directory.
    ///
    /// Whether the driver actually terminates is up to its termination validator;
    /// Aeron's default rejects every request.
    pub fn request_driver_termination(aeron_dir: &str, token: &[u8]) -> Result<bool> {
        Ok(ffi::requestDriverTermination(aeron_dir, token)?)
    }

    /// The Aeron directory shared with the media driver.
    ///
    /// Defaults to the `AERON_DIR` environment variable if set, otherwise Aeron's
    /// platform default (e.g. `/dev/shm/aeron-<user>` on Linux).
    pub fn aeron_dir(mut self, dir: impl Into<String>) -> Self {
        self.aeron_dir = Some(dir.into());
        self
    }

    /// A name for this client, reported to the media driver (e.g. in its counters).
    pub fn client_name(mut self, name: impl Into<String>) -> Self {
        self.client_name = Some(name.into());
        self
    }

    /// How long without a media driver heartbeat before the client considers the
    /// driver dead, reports [`ErrorKind::DriverTimeout`](crate::ErrorKind::DriverTimeout)
    /// to the error handler and closes. Aeron's default is 10 seconds.
    pub fn driver_timeout(mut self, timeout: Duration) -> Self {
        self.driver_timeout = Some(timeout);
        self
    }

    /// How long inactive resources linger before they are freed. Aeron's default is 3 seconds.
    pub fn resource_linger_timeout(mut self, timeout: Duration) -> Self {
        self.resource_linger_timeout = Some(timeout);
        self
    }

    /// How long the client conductor thread sleeps when idle. Aeron's default is 16 ms.
    /// Rounded down to whole milliseconds.
    pub fn idle_sleep_duration(mut self, duration: Duration) -> Self {
        self.idle_sleep_duration = Some(duration);
        self
    }

    /// Pre-touch memory-mapped log buffers so later accesses don't page fault.
    pub fn pre_touch_mapped_memory(mut self, value: bool) -> Self {
        self.pre_touch_mapped_memory = Some(value);
        self
    }

    /// Run the client conductor on your own thread instead of a dedicated one:
    /// call [`AeronClient::invoke`] regularly (e.g. in your event loop) to do its
    /// work. Handlers then run inside `invoke`.
    pub fn use_conductor_agent_invoker(mut self, value: bool) -> Self {
        self.use_conductor_agent_invoker = Some(value);
        self
    }

    /// Handle errors raised asynchronously by the client, e.g. a
    /// [`DriverTimeout`](crate::ErrorKind::DriverTimeout) after which the client is closed.
    ///
    /// Non-fatal errors are reported here too, without closing the client.
    ///
    /// The handler runs on the client conductor thread, so it must be `Send + Sync`.
    /// A panic in the handler is caught and printed to stderr; it cannot propagate.
    /// Dropping the client (or its last publication, subscription or counters
    /// reader) from the handler is allowed: the drop completes on another thread.
    ///
    /// Without a handler, errors are printed to stderr. (The Aeron C++ default
    /// handler would call `exit(-1)`; aeron-glide never installs it.)
    pub fn error_handler<F>(self, handler: F) -> Self
    where
        F: Fn(&Error) + Send + Sync + 'static,
    {
        self.install(move |ctx| {
            ctx.setErrorHandler(handlers::error::<F>, release::<F>, into_ctx(handler))
        })
    }

    /// Called when an image (a publisher session) becomes available on one of
    /// this client's subscriptions (C++ `availableImageHandler`).
    ///
    /// Like every handler here, it runs on the client conductor thread: keep it
    /// short and hand work off to other threads. Panics are caught and printed.
    pub fn on_available_image<F>(self, handler: F) -> Self
    where
        F: Fn(&ImageEvent) + Send + Sync + 'static,
    {
        self.install(move |ctx| {
            ctx.setAvailableImageHandler(
                handlers::image_event::<F>,
                release::<F>,
                into_ctx(handler),
            )
        })
    }

    /// Called when an image becomes unavailable, e.g. its publisher closed or
    /// timed out (C++ `unavailableImageHandler`).
    pub fn on_unavailable_image<F>(self, handler: F) -> Self
    where
        F: Fn(&ImageEvent) + Send + Sync + 'static,
    {
        self.install(move |ctx| {
            ctx.setUnavailableImageHandler(
                handlers::image_event::<F>,
                release::<F>,
                into_ctx(handler),
            )
        })
    }

    /// Called when the media driver has added a concurrent publication for this
    /// client (C++ `newPublicationHandler`).
    pub fn on_new_publication<F>(self, handler: F) -> Self
    where
        F: Fn(&NewPublication) + Send + Sync + 'static,
    {
        self.install(move |ctx| {
            ctx.setNewPublicationHandler(
                handlers::new_publication::<F>,
                release::<F>,
                into_ctx(handler),
            )
        })
    }

    /// Called when the media driver has added an exclusive publication for this
    /// client (C++ `newExclusivePublicationHandler`).
    pub fn on_new_exclusive_publication<F>(self, handler: F) -> Self
    where
        F: Fn(&NewPublication) + Send + Sync + 'static,
    {
        self.install(move |ctx| {
            ctx.setNewExclusivePublicationHandler(
                handlers::new_publication::<F>,
                release::<F>,
                into_ctx(handler),
            )
        })
    }

    /// Called when the media driver has added a subscription for this client
    /// (C++ `newSubscriptionHandler`).
    pub fn on_new_subscription<F>(self, handler: F) -> Self
    where
        F: Fn(&NewSubscription) + Send + Sync + 'static,
    {
        self.install(move |ctx| {
            ctx.setNewSubscriptionHandler(
                handlers::new_subscription::<F>,
                release::<F>,
                into_ctx(handler),
            )
        })
    }

    /// Called when a counter becomes available (C++ `availableCounterHandler`).
    /// More can be added later with `AeronClient::add_available_counter_handler`.
    pub fn on_available_counter<F>(self, handler: F) -> Self
    where
        F: Fn(CounterEvent) + Send + Sync + 'static,
    {
        self.install(move |ctx| {
            ctx.setAvailableCounterHandler(
                handlers::counter_event::<F>,
                release::<F>,
                into_ctx(handler),
            )
        })
    }

    /// Called when a counter becomes unavailable (C++ `unavailableCounterHandler`).
    pub fn on_unavailable_counter<F>(self, handler: F) -> Self
    where
        F: Fn(CounterEvent) + Send + Sync + 'static,
    {
        self.install(move |ctx| {
            ctx.setUnavailableCounterHandler(
                handlers::counter_event::<F>,
                release::<F>,
                into_ctx(handler),
            )
        })
    }

    /// Called when the client closes (C++ `closeClientHandler`).
    pub fn on_close_client<F>(self, handler: F) -> Self
    where
        F: Fn() + Send + Sync + 'static,
    {
        self.install(move |ctx| {
            ctx.setCloseClientHandler(handlers::close_client::<F>, release::<F>, into_ctx(handler))
        })
    }

    /// Called when one of this client's publications receives an error frame, e.g.
    /// a subscriber rejected its image (C++ `errorFrameHandler`).
    pub fn on_publication_error_frame<F>(self, handler: F) -> Self
    where
        F: Fn(&PublicationErrorFrame) + Send + Sync + 'static,
    {
        self.install(move |ctx| {
            ctx.setErrorFrameHandler(handlers::error_frame::<F>, release::<F>, into_ctx(handler))
        })
    }

    fn install(
        mut self,
        installer: impl FnOnce(Pin<&mut ffi::ContextWrapper>) + Send + 'static,
    ) -> Self {
        self.handlers.push(Box::new(installer));
        self
    }

    pub(crate) fn connect(self) -> Result<AeronClient> {
        let mut ctx = ffi::create_context()?;
        if let Some(dir) = self.aeron_dir.or_else(|| std::env::var("AERON_DIR").ok()) {
            ctx.pin_mut().setAeronDir(&dir)?;
        }
        if let Some(name) = &self.client_name {
            ctx.pin_mut().setClientName(name)?;
        }
        if let Some(timeout) = self.driver_timeout {
            ctx.pin_mut().setDriverTimeoutMs(millis(timeout))?;
        }
        if let Some(timeout) = self.resource_linger_timeout {
            ctx.pin_mut().setResourceLingerTimeoutMs(millis(timeout))?;
        }
        if let Some(duration) = self.idle_sleep_duration {
            ctx.pin_mut().setIdleSleepDurationMs(millis(duration))?;
        }
        if let Some(value) = self.pre_touch_mapped_memory {
            ctx.pin_mut().setPreTouchMappedMemory(value)?;
        }
        if let Some(value) = self.use_conductor_agent_invoker {
            ctx.pin_mut().setUseConductorAgentInvoker(value)?;
        }
        // C++ takes ownership of each handler and releases it when the client is
        // destroyed (or the handler is replaced).
        for install in self.handlers {
            install(ctx.pin_mut());
        }
        Ok(AeronClient {
            inner: ffi::create_aeron(ctx)?,
            abandoned: std::sync::Mutex::new(Vec::new()),
        })
    }
}

/// Milliseconds for the C++ `long` setters, clamped so Aeron's conversion to
/// nanoseconds (`ms * 1_000_000` as `u64`) cannot overflow.
fn millis(duration: Duration) -> i64 {
    let max = (u64::MAX / 1_000_000).min(c_long::MAX as u64);
    duration.as_millis().min(u128::from(max)) as i64
}
