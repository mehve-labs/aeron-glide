//! The Aeron client ([`AeronClient`]).

use super::*;
use std::marker::PhantomData;

/// Aeron client — the main entry point for creating publications and subscriptions.
///
/// The client is `Send + Sync`: share one per process (e.g. in an `Arc`) and add
/// publications and subscriptions from any thread.
pub struct AeronClient {
    pub(crate) inner: cxx::UniquePtr<ffi::AeronWrapper>,
}

impl Drop for AeronClient {
    fn drop(&mut self) {
        callback::drop_outside_conductor(&mut self.inner);
    }
}

// SAFETY: aeron::Aeron is thread-safe: resource registration goes through the C
// client's command queue and the C++ wrapper's `m_adminLock`. The wrapper only
// holds a `shared_ptr<aeron::Aeron>` (atomic reference count), and every method
// bridged as `&self` is a const C++ method.
unsafe impl Send for AeronClient {}
unsafe impl Sync for AeronClient {}

impl AeronClient {
    /// Create a new Aeron client connected to the media driver, with default settings.
    ///
    /// Equivalent to `AeronClient::connect(Context::new())`.
    pub fn new() -> Result<Self> {
        Self::connect(Context::new())
    }

    /// Create a new Aeron client connected to the media driver, configured by `context`.
    pub fn connect(context: Context) -> Result<Self> {
        context.connect()
    }

    /// Start the client conductor thread.
    pub fn start(&self) {
        self.inner.start();
    }

    /// Returns `true` if the client has been closed.
    pub fn is_closed(&self) -> bool {
        self.inner.isClosed()
    }

    /// Add a concurrent publication on the given channel and stream ID, waiting
    /// until the media driver has created it. Multiple publishers can share the
    /// same channel+stream.
    ///
    /// Fails with [`ErrorKind::Timeout`] if the driver does not respond within the
    /// client's driver timeout.
    pub fn add_publication(&self, channel: &str, stream_id: i32) -> Result<Publication> {
        self.add_publication_async(channel, stream_id)?.wait()
    }

    /// Add an exclusive publication on the given channel and stream ID, waiting
    /// until the media driver has created it. Only one publisher is allowed per
    /// session — lower overhead than concurrent.
    ///
    /// Fails with [`ErrorKind::Timeout`] if the driver does not respond within the
    /// client's driver timeout.
    pub fn add_exclusive_publication(
        &self,
        channel: &str,
        stream_id: i32,
    ) -> Result<ExclusivePublication> {
        self.add_exclusive_publication_async(channel, stream_id)?
            .wait()
    }

    /// Add a subscription on the given channel and stream ID, waiting until the
    /// media driver has created it.
    ///
    /// Fails with [`ErrorKind::Timeout`] if the driver does not respond within the
    /// client's driver timeout.
    pub fn add_subscription(&self, channel: &str, stream_id: i32) -> Result<Subscription> {
        self.add_subscription_async(channel, stream_id)?.wait()
    }

    /// Start adding a concurrent publication without waiting: poll the returned
    /// [`PendingAdd`] until the publication is ready.
    pub fn add_publication_async(
        &self,
        channel: &str,
        stream_id: i32,
    ) -> Result<PendingAdd<'_, Publication>> {
        Ok(PendingAdd::new(
            self,
            self.inner.addPublication(channel, stream_id)?,
        ))
    }

    /// Start adding an exclusive publication without waiting: poll the returned
    /// [`PendingAdd`] until the publication is ready.
    pub fn add_exclusive_publication_async(
        &self,
        channel: &str,
        stream_id: i32,
    ) -> Result<PendingAdd<'_, ExclusivePublication>> {
        Ok(PendingAdd::new(
            self,
            self.inner.addExclusivePublication(channel, stream_id)?,
        ))
    }

    /// Start adding a subscription without waiting: poll the returned
    /// [`PendingAdd`] until the subscription is ready.
    pub fn add_subscription_async(
        &self,
        channel: &str,
        stream_id: i32,
    ) -> Result<PendingAdd<'_, Subscription>> {
        Ok(PendingAdd::new(
            self,
            self.inner.addSubscription(channel, stream_id)?,
        ))
    }

    /// The ID the media driver assigned to this client.
    pub fn client_id(&self) -> i64 {
        self.inner.clientId()
    }

    /// A new correlation ID, unique across clients of the same media driver (e.g.
    /// to tag requests).
    pub fn next_correlation_id(&self) -> i64 {
        self.inner.nextCorrelationId()
    }

    /// The Aeron directory this client is connected through.
    pub fn aeron_dir(&self) -> String {
        self.inner.aeronDir()
    }

    /// The path of the media driver's command-and-control (CnC) file.
    pub fn cnc_file_name(&self) -> Result<String> {
        Ok(self.inner.cncFileName()?)
    }

    /// How long without a media driver heartbeat before the client considers the
    /// driver dead (see [`Context::driver_timeout`]).
    pub fn driver_timeout(&self) -> std::time::Duration {
        std::time::Duration::from_millis(self.inner.driverTimeoutMs().max(0) as u64)
    }

    /// Get a reader for the media driver's CNC counters (bytes sent/received, errors, etc.).
    pub fn counters_reader(&self) -> CountersReader {
        CountersReader {
            inner: self.inner.countersReader(),
        }
    }
}

impl Default for AeronClient {
    fn default() -> Self {
        Self::new().expect("Failed to create AeronClient")
    }
}

/// A publication or subscription being added by the media driver, returned by
/// `AeronClient::add_*_async` (C++ `addPublication` / `findPublication`, ...).
///
/// [`poll`](Self::poll) until it returns the resource. If dropped before then, the
/// resource is still created by the driver and released when the client closes.
#[must_use = "poll the pending add to get the resource"]
pub struct PendingAdd<'a, T> {
    client: &'a AeronClient,
    registration_id: i64,
    done: bool,
    _resource: PhantomData<fn() -> T>,
}

impl<'a, T> PendingAdd<'a, T> {
    fn new(client: &'a AeronClient, registration_id: i64) -> Self {
        Self {
            client,
            registration_id,
            done: false,
            _resource: PhantomData,
        }
    }

    /// The registration ID the resource will have.
    pub fn registration_id(&self) -> i64 {
        self.registration_id
    }

    fn check_not_done(&self) -> Result<()> {
        if self.done {
            return Err(Error::new(
                ErrorKind::IllegalState,
                "the pending add already completed",
            ));
        }
        Ok(())
    }
}

macro_rules! pending_add {
    ($resource:ident, $find:ident) => {
        impl PendingAdd<'_, $resource> {
            #[doc = concat!("Returns the [`", stringify!($resource), "`] once the media driver has created it,")]
            /// `None` while it is pending; fails if the driver rejected it or it
            /// already completed.
            pub fn poll(&mut self) -> Result<Option<$resource>> {
                self.check_not_done()?;
                let inner = self.client.inner.$find(self.registration_id)?;
                if inner.is_null() {
                    return Ok(None);
                }
                self.done = true;
                Ok(Some($resource { inner }))
            }

            /// Poll until the resource is ready, up to the client's driver timeout.
            pub fn wait(mut self) -> Result<$resource> {
                let deadline = std::time::Instant::now() + self.client.driver_timeout();
                loop {
                    if let Some(resource) = self.poll()? {
                        return Ok(resource);
                    }
                    if std::time::Instant::now() >= deadline {
                        return Err(Error::new(
                            ErrorKind::Timeout,
                            format!(
                                "no response from the media driver for registration {}",
                                self.registration_id
                            ),
                        ));
                    }
                    std::thread::yield_now();
                }
            }
        }
    };
}

pending_add!(Publication, findPublication);
pending_add!(ExclusivePublication, findExclusivePublication);
pending_add!(Subscription, findSubscription);
