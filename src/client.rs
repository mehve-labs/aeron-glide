//! The Aeron client ([`AeronClient`]).

use super::*;

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

    /// Add a concurrent publication on the given channel and stream ID.
    /// Multiple publishers can share the same channel+stream.
    pub fn add_publication(&self, channel: &str, stream_id: i32) -> Result<Publication> {
        let pub_inner = self.inner.addPublication(channel, stream_id)?;
        Ok(Publication { inner: pub_inner })
    }

    /// Add an exclusive publication on the given channel and stream ID.
    /// Only one publisher is allowed per session — lower overhead than concurrent.
    pub fn add_exclusive_publication(
        &self,
        channel: &str,
        stream_id: i32,
    ) -> Result<ExclusivePublication> {
        let pub_inner = self.inner.addExclusivePublication(channel, stream_id)?;
        Ok(ExclusivePublication { inner: pub_inner })
    }

    /// Add a subscription on the given channel and stream ID.
    pub fn add_subscription(&self, channel: &str, stream_id: i32) -> Result<Subscription> {
        let sub_inner = self.inner.addSubscription(channel, stream_id)?;
        Ok(Subscription { inner: sub_inner })
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
