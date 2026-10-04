//! Client configuration ([`Context`]).

use crate::error::Ffi;
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
/// Settings left unset come from Aeron's client environment variables, as in
/// Aeron's C and Java clients: `AERON_DIR`, `AERON_CLIENT_NAME`,
/// `AERON_DRIVER_TIMEOUT`, `AERON_CLIENT_RESOURCE_LINGER_DURATION`,
/// `AERON_CLIENT_IDLE_SLEEP_DURATION` and `AERON_CLIENT_PRE_TOUCH_MAPPED_MEMORY`
/// (parsed by Aeron: `AERON_DRIVER_TIMEOUT` in milliseconds, the durations
/// with a unit, e.g. `5s` or `500ms`), then Aeron's defaults. A variable that
/// does not parse fails [`AeronClient::connect`].
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
    aeron_dir: Option<std::path::PathBuf>,
    client_name: Option<String>,
    driver_timeout: Option<Duration>,
    resource_linger_timeout: Option<Duration>,
    idle_sleep_duration: Option<Duration>,
    pre_touch_mapped_memory: Option<bool>,
    use_conductor_agent_invoker: Option<bool>,
    handlers: Vec<Installer>,
}

impl std::fmt::Debug for Context {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Context")
            .field("aeron_dir", &self.aeron_dir)
            .field("client_name", &self.client_name)
            .field("driver_timeout", &self.driver_timeout)
            .field("resource_linger_timeout", &self.resource_linger_timeout)
            .field("idle_sleep_duration", &self.idle_sleep_duration)
            .field("pre_touch_mapped_memory", &self.pre_touch_mapped_memory)
            .field(
                "use_conductor_agent_invoker",
                &self.use_conductor_agent_invoker,
            )
            .field("handlers", &self.handlers.len())
            .finish()
    }
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
        ffi::defaultAeronPath().ffi()
    }

    /// Ask the media driver running in `aeron_dir` to terminate, presenting
    /// `token` to its termination validator. Returns `true` if the request was
    /// sent, `false` if the driver has not initialised its CnC file yet; fails if
    /// there is no CnC file (no driver) in that directory.
    ///
    /// Whether the driver actually terminates is up to its termination validator;
    /// Aeron's default rejects every request.
    pub fn request_driver_termination(
        aeron_dir: impl AsRef<std::path::Path>,
        token: &[u8],
    ) -> Result<bool> {
        let aeron_dir = crate::error::path_str(aeron_dir.as_ref())?;
        ffi::requestDriverTermination(aeron_dir, token).ffi()
    }

    /// The Aeron directory shared with the media driver.
    ///
    /// Defaults to the `AERON_DIR` environment variable if set, otherwise Aeron's
    /// platform default (e.g. `/dev/shm/aeron-<user>` on Linux).
    pub fn aeron_dir(mut self, dir: impl AsRef<std::path::Path>) -> Self {
        self.aeron_dir = Some(dir.as_ref().to_path_buf());
        self
    }

    /// A name for this client, reported to the media driver (e.g. in its counters).
    pub fn client_name(mut self, name: impl Into<String>) -> Self {
        self.client_name = Some(name.into());
        self
    }

    /// How long without a media driver heartbeat before the client considers the
    /// driver dead, reports [`ErrorKind::DriverTimeout`](crate::ErrorKind::DriverTimeout)
    /// to the error handler and closes. Aeron's default is 10 seconds. Keep it to
    /// a few seconds at least: the client checks liveness about every half
    /// second, so sub-second timeouts expire spuriously under load.
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
    /// Rounded up to whole milliseconds (Aeron takes milliseconds), so a short
    /// non-zero sleep does not become a busy spin; zero does spin.
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
        let mut ctx = ffi::create_context().ffi()?;
        // The C++ context would overwrite Aeron's environment variables with its
        // own defaults: apply them for every setting left unset.
        let env = ffi::clientEnvironment().ffi()?;
        let dir = self.aeron_dir.unwrap_or_else(|| env.dir.into());
        ctx.pin_mut()
            .setAeronDir(crate::error::path_str(&dir)?)
            .ffi()?;
        let name = self.client_name.unwrap_or(env.client_name);
        if !name.is_empty() {
            ctx.pin_mut().setClientName(&name).ffi()?;
        }
        let driver_timeout = self
            .driver_timeout
            .unwrap_or(Duration::from_millis(env.driver_timeout_ms));
        ctx.pin_mut()
            .setDriverTimeoutMs(millis(driver_timeout))
            .ffi()?;
        let linger = self
            .resource_linger_timeout
            .unwrap_or(Duration::from_nanos(env.resource_linger_ns));
        ctx.pin_mut()
            .setResourceLingerTimeoutMs(millis(linger))
            .ffi()?;
        let idle_sleep = self
            .idle_sleep_duration
            .unwrap_or(Duration::from_nanos(env.idle_sleep_ns));
        // Round up: a sub-millisecond sleep must not truncate to 0 (a spin).
        let ms = millis(
            idle_sleep
                .checked_add(Duration::from_nanos(999_999))
                .unwrap_or(idle_sleep),
        );
        ctx.pin_mut().setIdleSleepDurationMs(ms).ffi()?;
        ctx.pin_mut()
            .setPreTouchMappedMemory(
                self.pre_touch_mapped_memory
                    .unwrap_or(env.pre_touch_mapped_memory),
            )
            .ffi()?;
        if let Some(value) = self.use_conductor_agent_invoker {
            ctx.pin_mut().setUseConductorAgentInvoker(value).ffi()?;
        }
        // C++ takes ownership of each handler and releases it when the client is
        // destroyed (or the handler is replaced).
        for install in self.handlers {
            install(ctx.pin_mut());
        }
        Ok(AeronClient {
            inner: ffi::create_aeron(ctx).ffi()?,
            abandoned: std::sync::Mutex::new(Vec::new()),
        })
    }
}

/// Milliseconds for the C++ `long` setters, clamped so Aeron's deadlines
/// (`now + timeout` in nanoseconds) cannot overflow.
#[allow(clippy::unnecessary_cast)] // `c_long` is 32 bits on Windows
fn millis(duration: Duration) -> i64 {
    crate::timeout_millis(duration).min(c_long::MAX as i64)
}
