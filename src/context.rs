//! Client configuration ([`Context`]).

use crate::{AeronClient, Error, Result, ffi};
use std::panic::{self, AssertUnwindSafe};
use std::time::Duration;

type ErrorHandler = Box<dyn Fn(&Error) + Send + Sync + 'static>;

/// Configuration for an [`AeronClient`], mirroring the Aeron C++ `aeron::Context`.
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
    error_handler: Option<ErrorHandler>,
}

impl Context {
    /// A context with Aeron's defaults.
    pub fn new() -> Self {
        Self::default()
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
    pub fn idle_sleep_duration(mut self, duration: Duration) -> Self {
        self.idle_sleep_duration = Some(duration);
        self
    }

    /// Pre-touch memory-mapped log buffers so later accesses don't page fault.
    pub fn pre_touch_mapped_memory(mut self, value: bool) -> Self {
        self.pre_touch_mapped_memory = Some(value);
        self
    }

    /// Handle errors raised asynchronously by the client, e.g. a
    /// [`DriverTimeout`](crate::ErrorKind::DriverTimeout) after which the client is closed.
    ///
    /// The handler runs on the client conductor thread, so it must be `Send + Sync`.
    /// A panic in the handler is caught and printed to stderr; it cannot propagate.
    ///
    /// Without a handler, errors are printed to stderr. (The Aeron C++ default
    /// handler would call `exit(-1)`; aeron-glide never installs it.)
    pub fn error_handler<F>(mut self, handler: F) -> Self
    where
        F: Fn(&Error) + Send + Sync + 'static,
    {
        self.error_handler = Some(Box::new(handler));
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
        if let Some(handler) = self.error_handler {
            let owned = Box::into_raw(Box::new(handler)).expose_provenance();
            // C++ takes ownership and calls `release_error_handler` when the client is destroyed.
            ctx.pin_mut()
                .setErrorHandler(call_error_handler, release_error_handler, owned);
        }
        Ok(AeronClient {
            inner: ffi::create_aeron(ctx)?,
        })
    }
}

fn millis(duration: Duration) -> i64 {
    i64::try_from(duration.as_millis()).unwrap_or(i64::MAX)
}

fn call_error_handler(ctx: usize, encoded: &[u8]) {
    // SAFETY: `ctx` is the `Box<ErrorHandler>` leaked in `connect`; C++ releases it
    // only after the last call.
    let handler = unsafe { &*std::ptr::with_exposed_provenance::<ErrorHandler>(ctx) };
    let error = Error::from_encoded(&String::from_utf8_lossy(encoded));
    if panic::catch_unwind(AssertUnwindSafe(|| handler(&error))).is_err() {
        eprintln!("aeron-glide: the client error handler panicked while handling: {error}");
    }
}

fn release_error_handler(ctx: usize) {
    // SAFETY: as in `call_error_handler`; C++ calls this exactly once.
    drop(unsafe { Box::from_raw(std::ptr::with_exposed_provenance_mut::<ErrorHandler>(ctx)) });
}
