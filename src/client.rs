//! The Aeron client ([`AeronClient`]).

use super::*;
use crate::handlers;
use std::marker::PhantomData;

/// Aeron client — the main entry point for creating publications and subscriptions.
///
/// The client is `Send + Sync`: share one per process (e.g. in an `Arc`) and add
/// publications and subscriptions from any thread.
pub struct AeronClient {
    pub(crate) inner: cxx::UniquePtr<ffi::AeronWrapper>,
    /// Pending adds dropped before completion; their resources are closed by
    /// [`AeronClient::reap`] once the driver has created them.
    pub(crate) abandoned: std::sync::Mutex<Vec<(AddKind, i64)>>,
}

impl Drop for AeronClient {
    fn drop(&mut self) {
        callback::drop_outside_conductor(&mut self.inner);
    }
}

// SAFETY: aeron::Aeron is thread-safe: resource registration goes through the C
// client's command queue and the C++ wrapper's `m_adminLock`. In agent invoker
// mode, where the C client runs that work inline on the calling thread, the shim's
// ConductorLock serialises every conductor-touching call, `invoke` and every
// resource close. The wrapper only holds `shared_ptr`s (atomic reference counts),
// and every method bridged as `&self` is a const C++ method.
unsafe impl Send for AeronClient {}
unsafe impl Sync for AeronClient {}

impl std::fmt::Debug for AeronClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AeronClient")
            .field("client_id", &self.client_id())
            .field("is_closed", &self.is_closed())
            .finish_non_exhaustive()
    }
}

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

    /// Add a subscription with its own image handlers (instead of the
    /// [`Context`]'s), waiting until the media driver has created it.
    ///
    /// The handlers run on the client conductor thread, like the
    /// [`Context::on_available_image`] handlers.
    pub fn add_subscription_with_image_handlers<A, U>(
        &self,
        channel: &str,
        stream_id: i32,
        on_available_image: A,
        on_unavailable_image: U,
    ) -> Result<Subscription>
    where
        A: Fn(&ImageEvent) + Send + Sync + 'static,
        U: Fn(&ImageEvent) + Send + Sync + 'static,
    {
        self.add_subscription_with_image_handlers_async(
            channel,
            stream_id,
            on_available_image,
            on_unavailable_image,
        )?
        .wait()
    }

    /// Start adding a subscription with its own image handlers without waiting.
    pub fn add_subscription_with_image_handlers_async<A, U>(
        &self,
        channel: &str,
        stream_id: i32,
        on_available_image: A,
        on_unavailable_image: U,
    ) -> Result<PendingAdd<'_, Subscription>>
    where
        A: Fn(&ImageEvent) + Send + Sync + 'static,
        U: Fn(&ImageEvent) + Send + Sync + 'static,
    {
        self.ensure_open()?;
        let id = self.inner.addSubscriptionWithImageHandlers(
            channel,
            stream_id,
            handlers::image_event::<A>,
            handlers::release::<A>,
            handlers::into_ctx(on_available_image),
            handlers::image_event::<U>,
            handlers::release::<U>,
            handlers::into_ctx(on_unavailable_image),
        )?;
        Ok(PendingAdd::new(self, id, AddKind::Subscription))
    }

    /// Add a handler called when a counter becomes available. Returns its
    /// registration ID for [`remove_available_counter_handler`](Self::remove_available_counter_handler).
    ///
    /// Handlers cannot be added or removed from inside a handler
    /// ([`ErrorKind::Reentrant`]): the conductor running the handler would have to
    /// process the change itself. A handler that owns an `Arc<AeronClient>` keeps
    /// the client alive (a reference cycle) until it is removed.
    pub fn add_available_counter_handler<F>(&self, handler: F) -> Result<i64>
    where
        F: Fn(CounterEvent) + Send + Sync + 'static,
    {
        callback::ensure_not_in_conductor_callback("adding a handler")?;
        Ok(self.inner.addAvailableCounterHandler(
            handlers::counter_event::<F>,
            handlers::release::<F>,
            handlers::into_ctx(handler),
        )?)
    }

    /// Remove a handler added with
    /// [`add_available_counter_handler`](Self::add_available_counter_handler).
    /// Removing an unknown ID does nothing.
    pub fn remove_available_counter_handler(&self, registration_id: i64) -> Result<()> {
        callback::ensure_not_in_conductor_callback("removing a handler")?;
        Ok(self.inner.removeAvailableCounterHandler(registration_id)?)
    }

    /// Add a handler called when a counter becomes unavailable. Returns its
    /// registration ID for
    /// [`remove_unavailable_counter_handler`](Self::remove_unavailable_counter_handler).
    pub fn add_unavailable_counter_handler<F>(&self, handler: F) -> Result<i64>
    where
        F: Fn(CounterEvent) + Send + Sync + 'static,
    {
        callback::ensure_not_in_conductor_callback("adding a handler")?;
        Ok(self.inner.addUnavailableCounterHandler(
            handlers::counter_event::<F>,
            handlers::release::<F>,
            handlers::into_ctx(handler),
        )?)
    }

    /// Remove a handler added with
    /// [`add_unavailable_counter_handler`](Self::add_unavailable_counter_handler).
    pub fn remove_unavailable_counter_handler(&self, registration_id: i64) -> Result<()> {
        callback::ensure_not_in_conductor_callback("removing a handler")?;
        Ok(self
            .inner
            .removeUnavailableCounterHandler(registration_id)?)
    }

    /// Add a handler called when the client closes. Returns its registration ID
    /// for [`remove_close_client_handler`](Self::remove_close_client_handler).
    pub fn add_close_client_handler<F>(&self, handler: F) -> Result<i64>
    where
        F: Fn() + Send + Sync + 'static,
    {
        callback::ensure_not_in_conductor_callback("adding a handler")?;
        Ok(self.inner.addCloseClientHandler(
            handlers::close_client::<F>,
            handlers::release::<F>,
            handlers::into_ctx(handler),
        )?)
    }

    /// Remove a handler added with
    /// [`add_close_client_handler`](Self::add_close_client_handler).
    pub fn remove_close_client_handler(&self, registration_id: i64) -> Result<()> {
        callback::ensure_not_in_conductor_callback("removing a handler")?;
        Ok(self.inner.removeCloseClientHandler(registration_id)?)
    }

    /// Start adding a concurrent publication without waiting: poll the returned
    /// [`PendingAdd`] until the publication is ready.
    pub fn add_publication_async(
        &self,
        channel: &str,
        stream_id: i32,
    ) -> Result<PendingAdd<'_, Publication>> {
        self.ensure_open()?;
        self.reap();
        let id = self.inner.addPublication(channel, stream_id)?;
        Ok(PendingAdd::new(self, id, AddKind::Publication))
    }

    /// Start adding an exclusive publication without waiting: poll the returned
    /// [`PendingAdd`] until the publication is ready.
    pub fn add_exclusive_publication_async(
        &self,
        channel: &str,
        stream_id: i32,
    ) -> Result<PendingAdd<'_, ExclusivePublication>> {
        self.ensure_open()?;
        self.reap();
        let id = self.inner.addExclusivePublication(channel, stream_id)?;
        Ok(PendingAdd::new(self, id, AddKind::ExclusivePublication))
    }

    /// Start adding a subscription without waiting: poll the returned
    /// [`PendingAdd`] until the subscription is ready.
    pub fn add_subscription_async(
        &self,
        channel: &str,
        stream_id: i32,
    ) -> Result<PendingAdd<'_, Subscription>> {
        self.ensure_open()?;
        self.reap();
        let id = self.inner.addSubscription(channel, stream_id)?;
        Ok(PendingAdd::new(self, id, AddKind::Subscription))
    }

    /// Allocate a counter in the media driver, waiting until it is ready (C++
    /// `addCounter` / `findCounter`). `type_id` identifies the kind of counter for
    /// tools (use your own, distinct from Aeron's [`counter_types`](crate::counter_types));
    /// `key` (at most [`CountersReader::MAX_KEY_LENGTH`] bytes) and `label` (at most
    /// [`CountersReader::MAX_LABEL_LENGTH`] bytes) describe it; longer ones fail with
    /// [`ErrorKind::IllegalArgument`], as in the Java client (the C++ client lets
    /// the driver truncate them).
    ///
    /// The counter is freed when the returned [`Counter`] is dropped (which keeps
    /// this client open until then).
    ///
    /// Fails like [`add_publication`](Self::add_publication), e.g. with the
    /// driver's error (a negative [`Error::code`]) when the counters file is full.
    pub fn add_counter(&self, type_id: i32, key: &[u8], label: &str) -> Result<Counter> {
        self.add_counter_async(type_id, key, label)?.wait()
    }

    /// Start allocating a counter without waiting: poll the returned
    /// [`PendingAdd`] until the counter is ready.
    pub fn add_counter_async(
        &self,
        type_id: i32,
        key: &[u8],
        label: &str,
    ) -> Result<PendingAdd<'_, Counter>> {
        self.ensure_open()?;
        check_counter_metadata(key, label)?;
        self.reap();
        let id = self.inner.addCounter(type_id, key, label)?;
        Ok(PendingAdd::new(self, id, AddKind::Counter))
    }

    /// Allocate a static counter, or get the existing one with the same `type_id`
    /// and `registration_id`, waiting until it is ready (C++ `addStaticCounter`).
    ///
    /// A static counter is never freed: it outlives this client and the
    /// returned [`Counter`], so another client can find it again. Its owner ID is
    /// -1 and its registration ID is `registration_id`.
    ///
    /// Fails with the driver's error ([`ErrorKind::Aeron`]) if a non-static
    /// counter with the same type ID and registration ID exists.
    pub fn add_static_counter(
        &self,
        type_id: i32,
        key: &[u8],
        label: &str,
        registration_id: i64,
    ) -> Result<Counter> {
        self.add_static_counter_async(type_id, key, label, registration_id)?
            .wait()
    }

    /// Start allocating a static counter without waiting: poll the returned
    /// [`PendingAdd`] until the counter is ready.
    pub fn add_static_counter_async(
        &self,
        type_id: i32,
        key: &[u8],
        label: &str,
        registration_id: i64,
    ) -> Result<PendingAdd<'_, Counter>> {
        self.ensure_open()?;
        check_counter_metadata(key, label)?;
        self.reap();
        let id = self
            .inner
            .addStaticCounter(type_id, key, label, registration_id)?;
        Ok(PendingAdd::new(self, id, AddKind::Counter))
    }

    /// Returns `true` if the client runs its conductor through [`invoke`](Self::invoke)
    /// (see [`Context::use_conductor_agent_invoker`]).
    pub fn uses_agent_invoker(&self) -> bool {
        self.inner.usesAgentInvoker()
    }

    /// In agent invoker mode, run one duty cycle of the client conductor on this
    /// thread: process driver responses, run handlers, send keepalives. Returns
    /// the amount of work done (0 when idle). Call it regularly. Concurrent calls,
    /// and calls on other threads that add or close resources, are serialised.
    ///
    /// Fails with [`ErrorKind::IllegalState`] unless the client was created with
    /// [`Context::use_conductor_agent_invoker`]. Errors raised by the conductor go
    /// to the client's error handler, as in threaded mode.
    ///
    /// Calling it from a handler (i.e. inside `invoke`) fails with
    /// [`ErrorKind::Reentrant`].
    pub fn invoke(&self) -> Result<usize> {
        let work = self.inner.invokeConductor()?;
        self.reap();
        Ok(crate::error::count(work))
    }

    /// The client name set with [`Context::client_name`] (empty by default).
    pub fn client_name(&self) -> String {
        self.inner.clientName()
    }

    /// How long the client conductor sleeps when idle (see
    /// [`Context::idle_sleep_duration`]).
    pub fn idle_sleep_duration(&self) -> std::time::Duration {
        std::time::Duration::from_millis(self.inner.idleSleepDurationMs().max(0) as u64)
    }

    /// Close the resources of pending adds that were dropped before completing,
    /// once the driver has created them.
    /// Adds on a closed client fail at once, as the synchronous ones do: the
    /// driver will not answer it.
    fn ensure_open(&self) -> Result<()> {
        if self.is_closed() {
            return Err(Error::new(ErrorKind::IllegalState, "the client is closed"));
        }
        Ok(())
    }

    fn reap(&self) {
        if callback::in_conductor_callback() {
            return;
        }
        // Not held while finding: a find waits for the conductor lock, whose
        // holder may be dropping a `PendingAdd` (which pushes here).
        let mut abandoned = match self.abandoned.try_lock() {
            Ok(mut abandoned) => std::mem::take(&mut *abandoned),
            Err(_) => return,
        };
        abandoned.retain(|&(kind, id)| {
            // A found resource is closed when its wrapper is dropped here; a failed
            // add is gone. Keep only the ones still pending.
            let pending = match kind {
                AddKind::Publication => self.inner.findPublication(id).map(|p| p.is_null()),
                AddKind::ExclusivePublication => {
                    self.inner.findExclusivePublication(id).map(|p| p.is_null())
                }
                AddKind::Subscription => self.inner.findSubscription(id).map(|s| s.is_null()),
                AddKind::Counter => self.inner.findCounter(id).map(|c| c.is_null()),
            };
            // Still pending, or the conductor is busy on this thread: retry later.
            match pending {
                Ok(pending) => pending,
                Err(e) => Error::from(e).kind() == ErrorKind::Reentrant,
            }
        });
        if !abandoned.is_empty()
            && let Ok(mut current) = self.abandoned.lock()
        {
            current.append(&mut abandoned);
        }
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

fn check_counter_metadata(key: &[u8], label: &str) -> Result<()> {
    if key.len() > CountersReader::MAX_KEY_LENGTH {
        return Err(Error::new(
            ErrorKind::IllegalArgument,
            format!(
                "counter key is {} bytes, more than the maximum {}",
                key.len(),
                CountersReader::MAX_KEY_LENGTH
            ),
        ));
    }
    if label.len() > CountersReader::MAX_LABEL_LENGTH {
        return Err(Error::new(
            ErrorKind::IllegalArgument,
            format!(
                "counter label is {} bytes, more than the maximum {}",
                label.len(),
                CountersReader::MAX_LABEL_LENGTH
            ),
        ));
    }
    Ok(())
}

/// A publication, subscription or counter being added by the media driver,
/// returned by `AeronClient::add_*_async` (C++ `addPublication` /
/// `findPublication`, ...).
///
/// [`poll`](Self::poll) until it returns the resource. If dropped before then (or
/// if [`wait`](Self::wait) times out), the driver still creates the resource; the
/// client closes it on a later add, poll or `invoke`.
///
/// A rejected add (e.g. an invalid channel) fails with the driver's error,
/// [`ErrorKind::Aeron`] with a negative [`Error::code`].
#[must_use = "poll the pending add to get the resource"]
pub struct PendingAdd<'a, T> {
    client: &'a AeronClient,
    registration_id: i64,
    kind: AddKind,
    done: bool,
    _resource: PhantomData<fn() -> T>,
}

/// What a [`PendingAdd`] adds, to close it if the pending add is abandoned.
#[derive(Debug, Clone, Copy)]
pub(crate) enum AddKind {
    Publication,
    ExclusivePublication,
    Subscription,
    Counter,
}

impl<T> Drop for PendingAdd<'_, T> {
    fn drop(&mut self) {
        if !self.done
            && let Ok(mut abandoned) = self.client.abandoned.lock()
        {
            abandoned.push((self.kind, self.registration_id));
        }
    }
}

impl<T> std::fmt::Debug for PendingAdd<'_, T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PendingAdd")
            .field("registration_id", &self.registration_id)
            .field("done", &self.done)
            .finish_non_exhaustive()
    }
}

impl<'a, T> PendingAdd<'a, T> {
    fn new(client: &'a AeronClient, registration_id: i64, kind: AddKind) -> Self {
        Self {
            client,
            registration_id,
            kind,
            done: false,
            _resource: PhantomData,
        }
    }

    /// The ID of this add: the registration ID the resource will have, except
    /// for a static counter, whose registration ID is the one given to
    /// [`AeronClient::add_static_counter_async`].
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
            /// `None` while it is pending; fails if the driver rejected it, it
            /// already completed, or the client is closed. From inside a client
            /// handler in agent invoker mode it fails with
            /// [`ErrorKind::Reentrant`] and stays pending.
            pub fn poll(&mut self) -> Result<Option<$resource>> {
                self.check_not_done()?;
                let found = self.client.inner.$find(self.registration_id);
                self.client.reap();
                let inner = match found.map_err(Error::from) {
                    Ok(inner) => inner,
                    // The conductor is running on this thread (inside a handler):
                    // nothing happened, so the add is still pending.
                    Err(e) if e.kind() == ErrorKind::Reentrant => return Err(e),
                    Err(e) => {
                        // C++ forgets a failed registration; polling again is an error.
                        self.done = true;
                        return Err(e);
                    }
                };
                if inner.is_null() && self.client.is_closed() {
                    // The driver will not answer a closed client.
                    return Err(Error::new(ErrorKind::IllegalState, "the client is closed"));
                }
                if inner.is_null() {
                    return Ok(None);
                }
                self.done = true;
                Ok(Some($resource { inner }))
            }

            /// Poll until the resource is ready, up to the client's driver timeout. In
            /// agent invoker mode this also runs the conductor while waiting. Fails
            /// with [`ErrorKind::IllegalState`] if the client is (or becomes) closed.
            ///
            /// Fails with [`ErrorKind::Reentrant`] from inside a client handler, where
            /// waiting would stall the conductor that has to answer.
            pub fn wait(mut self) -> Result<$resource> {
                callback::ensure_not_in_conductor_callback("waiting for an add")?;
                let deadline = std::time::Instant::now() + self.client.driver_timeout();
                let invoke = self.client.uses_agent_invoker();
                // Spin briefly, then yield, then sleep (up to 100 µs) between polls.
                let mut idle = crate::concurrent::BackoffIdleStrategy::new(
                    10,
                    20,
                    std::time::Duration::from_micros(1),
                    std::time::Duration::from_micros(100),
                );
                loop {
                    if self.client.is_closed() {
                        // The driver will not answer a closed client.
                        return Err(Error::new(
                            ErrorKind::IllegalState,
                            "the client is closed",
                        ));
                    }
                    if invoke {
                        self.client.invoke()?;
                    }
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
                    crate::concurrent::IdleStrategy::idle(&mut idle, 0);
                }
            }
        }
    };
}

pending_add!(Publication, findPublication);
pending_add!(ExclusivePublication, findExclusivePublication);
pending_add!(Subscription, findSubscription);
pending_add!(Counter, findCounter);
