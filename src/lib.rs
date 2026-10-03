//! Safe, idiomatic Rust wrapper for the [Aeron](https://github.com/real-logic/aeron) C++ API.
//!
//! This crate binds directly to the Aeron C++ client using [`cxx`](https://cxx.rs/),
//! providing zero-cost abstractions over publications, subscriptions, images, and the
//! embedded media driver. Closures are passed cleanly across the FFI boundary via
//! trampolines, so the API feels native to Rust.
//!
//! # Quick start
//!
//! ```no_run
//! use aeron_glide::AeronClient;
//!
//! let mut client = AeronClient::new().unwrap();
//! client.start();
//!
//! let pub1 = client.add_publication("aeron:ipc", 1001).unwrap();
//! let mut sub1 = client.add_subscription("aeron:ipc", 1001).unwrap();
//!
//! // Publish
//! while let Err(e) = pub1.offer(b"hello aeron") {
//!     assert!(e.is_retryable(), "offer failed: {e}");
//! }
//!
//! // Subscribe
//! sub1.poll(10, |data| {
//!     println!("Received: {}", String::from_utf8_lossy(data));
//! })
//! .unwrap();
//! ```
//!
//! # Features
//!
//! - **IPC and UDP** transports via [`ChannelBuilder`]
//! - **Publications** ([`Publication`]) and **exclusive publications** ([`ExclusivePublication`])
//! - **Zero-copy publish** via [`Publication::try_claim`]
//! - **Fragment reassembly** via [`Subscription::poll_assembled`] with [`ControlledAction`] flow control
//! - **Image** access for per-session stream inspection
//! - **Counters** reader for real-time driver statistics
//! - **Embedded media driver** ([`MediaDriver`]) with full configuration
//! - **Archive client** (behind the `archive` feature flag): recording, replay, listing, and `ReplayMerge`
//!
//! # Thread safety
//!
//! | Type | `Send` | `Sync` |
//! |---|---|---|
//! | [`AeronClient`], [`Publication`], [`CountersReader`] | yes | yes |
//! | [`ExclusivePublication`], [`Subscription`] | yes | no |
//! | [`Image`] (borrows its `Subscription`) | no | no |
//!
//! Share one client per process and add resources from any thread; offer on a
//! concurrent [`Publication`] from several threads; move exclusive publications
//! and subscriptions to the thread that uses them.
//!
//! ```compile_fail,E0277
//! fn sync<T: Sync>() {}
//! sync::<aeron_glide::Subscription>();
//! ```
//!
//! ```compile_fail,E0277
//! fn sync<T: Sync>() {}
//! sync::<aeron_glide::ExclusivePublication>();
//! ```
//!
//! ```compile_fail,E0277
//! fn send<T: Send>() {}
//! send::<aeron_glide::Image<'static>>();
//! ```
//!
//! # Prerequisites
//!
//! - CMake and a C++14 compiler (Aeron C++ is built from source automatically)
//! - A running Aeron media driver (use the included `mediadriver` binary or [`MediaDriver`])
//! - Java 17+ only if building with `--features archive`
#![cfg_attr(docsrs, feature(doc_cfg))]

#[cfg(feature = "archive")]
#[cfg_attr(docsrs, doc(cfg(feature = "archive")))]
pub mod archive;

mod callback;
mod context;
mod driver_gen;
mod error;
use callback::Callback;
pub use context::Context;
pub use driver_gen::{InferableBoolean, ThreadNaming};
pub use error::{Error, ErrorKind, OfferError, Result};
use std::marker::PhantomData;

#[cxx::bridge(namespace = "aeron_rs")]
pub mod ffi {
    unsafe extern "C++" {
        include!("shim.h");

        type ContextWrapper;
        type AeronWrapper;
        type PublicationWrapper;
        type ExclusivePublicationWrapper;
        type SubscriptionWrapper;
        type MediaDriverWrapper;
        type CountersReaderWrapper;

        fn create_context() -> Result<UniquePtr<ContextWrapper>>;
        fn setAeronDir(self: Pin<&mut ContextWrapper>, dir: &str) -> Result<()>;
        fn setClientName(self: Pin<&mut ContextWrapper>, name: &str) -> Result<()>;
        fn setDriverTimeoutMs(self: Pin<&mut ContextWrapper>, value: i64) -> Result<()>;
        fn setResourceLingerTimeoutMs(self: Pin<&mut ContextWrapper>, value: i64) -> Result<()>;
        fn setIdleSleepDurationMs(self: Pin<&mut ContextWrapper>, value: i64) -> Result<()>;
        fn setPreTouchMappedMemory(self: Pin<&mut ContextWrapper>, value: bool) -> Result<()>;
        fn setErrorHandler(
            self: Pin<&mut ContextWrapper>,
            handler: fn(usize, &[u8]),
            release: fn(usize),
            ctx: usize,
        );
        fn create_aeron(context: UniquePtr<ContextWrapper>) -> Result<UniquePtr<AeronWrapper>>;
        fn create_media_driver() -> Result<UniquePtr<MediaDriverWrapper>>;

        fn start(self: &AeronWrapper);
        fn isClosed(self: &AeronWrapper) -> bool;
        fn addPublication(
            self: &AeronWrapper,
            channel: &str,
            stream_id: i32,
        ) -> Result<UniquePtr<PublicationWrapper>>;
        fn addExclusivePublication(
            self: &AeronWrapper,
            channel: &str,
            stream_id: i32,
        ) -> Result<UniquePtr<ExclusivePublicationWrapper>>;
        fn addSubscription(
            self: &AeronWrapper,
            channel: &str,
            stream_id: i32,
        ) -> Result<UniquePtr<SubscriptionWrapper>>;
        fn countersReader(self: &AeronWrapper) -> UniquePtr<CountersReaderWrapper>;

        fn start(self: Pin<&mut MediaDriverWrapper>) -> Result<()>;

        fn setThreadingMode(self: Pin<&mut MediaDriverWrapper>, mode: i32) -> Result<()>;

        fn offer(self: &PublicationWrapper, buffer: &[u8]) -> Result<i64>;
        fn tryClaim(
            self: &PublicationWrapper,
            length: usize,
            handler: fn(usize, &mut [u8]) -> bool,
            ctx: usize,
        ) -> Result<i64>;
        fn isConnected(self: &PublicationWrapper) -> bool;
        fn sessionId(self: &PublicationWrapper) -> i32;

        fn offer(self: Pin<&mut ExclusivePublicationWrapper>, buffer: &[u8]) -> Result<i64>;
        fn tryClaim(
            self: Pin<&mut ExclusivePublicationWrapper>,
            length: usize,
            handler: fn(usize, &mut [u8]) -> bool,
            ctx: usize,
        ) -> Result<i64>;
        fn isConnected(self: &ExclusivePublicationWrapper) -> bool;

        fn poll(
            self: Pin<&mut SubscriptionWrapper>,
            fragment_limit: i32,
            handler: fn(usize, &[u8]),
            ctx: usize,
        ) -> Result<i32>;
        fn controlledPollAssembled(
            self: Pin<&mut SubscriptionWrapper>,
            fragment_limit: i32,
            handler: fn(usize, &[u8]) -> i32,
            ctx: usize,
        ) -> Result<i32>;
        fn isConnected(self: &SubscriptionWrapper) -> bool;
        fn deleteSessionBuffer(self: Pin<&mut SubscriptionWrapper>, session_id: i32) -> bool;
        fn imageCount(self: &SubscriptionWrapper) -> i32;
        fn imageByIndex(self: &SubscriptionWrapper, index: usize) -> UniquePtr<ImageWrapper>;
        fn imageBySessionId(self: &SubscriptionWrapper, session_id: i32)
        -> UniquePtr<ImageWrapper>;

        type ImageWrapper;
        fn sessionId(self: &ImageWrapper) -> i32;
        fn correlationId(self: &ImageWrapper) -> i64;
        fn joinPosition(self: &ImageWrapper) -> i64;
        fn sourceIdentity(self: &ImageWrapper) -> String;
        fn position(self: &ImageWrapper) -> Result<i64>;
        fn isClosed(self: &ImageWrapper) -> bool;
        fn isEndOfStream(self: &ImageWrapper) -> bool;
        fn endOfStreamPosition(self: &ImageWrapper) -> i64;
        fn poll(
            self: Pin<&mut ImageWrapper>,
            fragment_limit: i32,
            handler: fn(usize, &[u8]),
            ctx: usize,
        ) -> Result<i32>;
        fn controlledPollAssembled(
            self: Pin<&mut ImageWrapper>,
            fragment_limit: i32,
            handler: fn(usize, &[u8]) -> i32,
            ctx: usize,
        ) -> Result<i32>;

        fn maxCounterId(self: &CountersReaderWrapper) -> i32;
        fn getCounterValue(self: &CountersReaderWrapper, id: i32) -> Result<i64>;
        fn getCounterState(self: &CountersReaderWrapper, id: i32) -> Result<i32>;
        fn getCounterTypeId(self: &CountersReaderWrapper, id: i32) -> Result<i32>;
        fn getCounterLabel(self: &CountersReaderWrapper, id: i32) -> Result<String>;
        fn forEach(
            self: &CountersReaderWrapper,
            handler: fn(usize, i32, i32, &[u8], &[u8]),
            ctx: usize,
        ) -> Result<()>;
    }
}

/// Aeron client — the main entry point for creating publications and subscriptions.
///
/// The client is `Send + Sync`: share one per process (e.g. in an `Arc`) and add
/// publications and subscriptions from any thread.
pub struct AeronClient {
    inner: cxx::UniquePtr<ffi::AeronWrapper>,
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

/// A concurrent publication for sending messages on a channel+stream.
///
/// `Send + Sync`: several threads may `offer` / `try_claim` on the same
/// publication concurrently (e.g. through an `Arc<Publication>`).
pub struct Publication {
    inner: cxx::UniquePtr<ffi::PublicationWrapper>,
}

impl Drop for Publication {
    fn drop(&mut self) {
        callback::drop_outside_conductor(&mut self.inner);
    }
}

// SAFETY: a concurrent publication is designed for use from multiple threads:
// aeron_publication_offer / try_claim are thread-safe, the other bridged methods
// only read state, and every method bridged as `&self` is a const C++ method.
unsafe impl Send for Publication {}
unsafe impl Sync for Publication {}

impl Publication {
    /// Publish a message. Returns the new stream position on success.
    ///
    /// On failure, [`OfferError::is_retryable`] tells whether retrying can succeed
    /// (not connected, back pressured, admin action).
    pub fn offer(&self, buffer: &[u8]) -> std::result::Result<i64, OfferError> {
        error::offer_result(self.inner.offer(buffer)?)
    }

    /// Zero-copy publish: claims a region of the log buffer, calls `handler` with a mutable
    /// slice pointing directly into shared memory, then commits or aborts based on the return value.
    /// Returns the new stream position if the claim succeeded.
    pub fn try_claim<F>(&self, length: usize, handler: F) -> std::result::Result<i64, OfferError>
    where
        F: FnMut(&mut [u8]) -> bool,
    {
        let length = claim_length(length)?;
        let mut cb = Callback::new(handler);
        let result = self.inner.tryClaim(length, callback::claim::<F>, cb.ctx());
        error::offer_result(cb.finish(result)?)
    }

    /// Returns `true` if there is at least one subscriber connected to this publication.
    pub fn is_connected(&self) -> bool {
        self.inner.isConnected()
    }

    /// The session ID assigned by the media driver for this publication.
    pub fn session_id(&self) -> i32 {
        self.inner.sessionId()
    }
}

/// An exclusive publication — single-writer, lower overhead than [`Publication`].
///
/// `Send` but not `Sync`: it can move to another thread, but only one thread may
/// use it at a time.
pub struct ExclusivePublication {
    inner: cxx::UniquePtr<ffi::ExclusivePublicationWrapper>,
}

impl Drop for ExclusivePublication {
    fn drop(&mut self) {
        callback::drop_outside_conductor(&mut self.inner);
    }
}

// SAFETY: an exclusive publication has no thread affinity; it only requires a
// single writer at a time, which `&mut self` on every mutating method enforces.
unsafe impl Send for ExclusivePublication {}

impl ExclusivePublication {
    /// Publish a message. Returns the new stream position on success.
    ///
    /// On failure, [`OfferError::is_retryable`] tells whether retrying can succeed
    /// (not connected, back pressured, admin action).
    pub fn offer(&mut self, buffer: &[u8]) -> std::result::Result<i64, OfferError> {
        error::offer_result(self.inner.pin_mut().offer(buffer)?)
    }

    /// Zero-copy publish: claims a region of the log buffer, calls `handler` with a mutable
    /// slice pointing directly into shared memory, then commits or aborts based on the return value.
    /// Returns the new stream position if the claim succeeded.
    pub fn try_claim<F>(
        &mut self,
        length: usize,
        handler: F,
    ) -> std::result::Result<i64, OfferError>
    where
        F: FnMut(&mut [u8]) -> bool,
    {
        let length = claim_length(length)?;
        let mut cb = Callback::new(handler);
        let result = self
            .inner
            .pin_mut()
            .tryClaim(length, callback::claim::<F>, cb.ctx());
        error::offer_result(cb.finish(result)?)
    }

    /// Returns `true` if there is at least one subscriber connected to this publication.
    pub fn is_connected(&self) -> bool {
        self.inner.isConnected()
    }
}

/// Aeron claim lengths are `int32`; reject longer ones instead of truncating them.
fn claim_length(length: usize) -> std::result::Result<usize, OfferError> {
    if length > i32::MAX as usize {
        return Err(OfferError::Error(Error::new(
            ErrorKind::IllegalArgument,
            format!("claim length {length} exceeds the maximum of {}", i32::MAX),
        )));
    }
    Ok(length)
}

/// Flow-control actions for `poll_assembled` when the handler returns a `ControlledAction`.
/// Matches Aeron's `ControlledPollAction` enum values.
#[repr(i32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ControlledAction {
    /// Abort polling — rewind position, re-deliver this fragment next poll.
    Abort = 0,
    /// Stop polling this image, commit position up to this fragment.
    Break = 1,
    /// Checkpoint position for flow control, continue polling.
    Commit = 2,
    /// Continue processing (default behavior).
    Continue = 3,
}

/// Trait that allows `poll_assembled` to accept handlers returning either `()` or `ControlledAction`.
/// Closures returning `()` map to `ControlledAction::Continue`.
pub trait PollAction {
    fn into_action(self) -> ControlledAction;
}

impl PollAction for () {
    #[inline]
    fn into_action(self) -> ControlledAction {
        ControlledAction::Continue
    }
}

impl PollAction for ControlledAction {
    #[inline]
    fn into_action(self) -> ControlledAction {
        self
    }
}

/// A subscription for receiving messages on a channel+stream.
///
/// `Send` but not `Sync`: it can move to another thread, but polling is
/// single-threaded. [`Image`]s borrow the subscription, so they stay on its thread.
pub struct Subscription {
    inner: cxx::UniquePtr<ffi::SubscriptionWrapper>,
}

impl Drop for Subscription {
    fn drop(&mut self) {
        callback::drop_outside_conductor(&mut self.inner);
    }
}

// SAFETY: a subscription has no thread affinity; polling only requires a single
// thread at a time, which `&mut self` on `poll` enforces. The image list is
// published by the client conductor with atomics, so reading it from the owning
// thread is fine. A `ReplayMerge` that shares the C++ subscription holds a
// `&mut Subscription` borrow for its whole life, so the subscription cannot be
// moved to another thread while the merge polls it.
unsafe impl Send for Subscription {}

impl Subscription {
    /// Poll for new messages, calling `handler` for each fragment received.
    /// Returns the number of fragments dispatched.
    ///
    /// # Panics
    ///
    /// If `handler` panics, the panic is resumed once Aeron returns from the poll;
    /// the remaining fragments of this poll are consumed without being delivered.
    pub fn poll<F>(&mut self, limit: i32, handler: F) -> Result<i32>
    where
        F: FnMut(&[u8]),
    {
        let mut cb = Callback::new(handler);
        let result = self
            .inner
            .pin_mut()
            .poll(limit, callback::fragment::<F>, cb.ctx());
        Ok(cb.finish(result)?)
    }

    /// Poll with automatic fragment reassembly. Messages that span multiple fragments
    /// are reassembled before being delivered to the handler, which always receives
    /// complete messages.
    ///
    /// The handler can return `()` (maps to Continue) or a `ControlledAction` for
    /// flow-control (Abort to retry, Break to stop, Commit to checkpoint, Continue to proceed).
    ///
    /// # Panics
    ///
    /// If `handler` panics, the panic is resumed once Aeron returns from the poll;
    /// the fragment being handled is aborted and delivered again by the next poll.
    pub fn poll_assembled<R, F>(&mut self, limit: i32, handler: F) -> Result<i32>
    where
        R: PollAction,
        F: FnMut(&[u8]) -> R,
    {
        let mut cb = Callback::new(handler);
        let result = self.inner.pin_mut().controlledPollAssembled(
            limit,
            callback::controlled_fragment::<F, R>,
            cb.ctx(),
        );
        Ok(cb.finish(result)?)
    }

    /// Returns `true` if there is at least one publisher connected to this subscription.
    pub fn is_connected(&self) -> bool {
        self.inner.isConnected()
    }

    /// Free the reassembly buffer held for a publisher session to reduce memory
    /// pressure, e.g. once its image goes inactive or no more large messages are
    /// expected from it. The buffer is recreated on demand if the session later
    /// sends another fragmented message.
    ///
    /// Returns `true` if a buffer was freed.
    pub fn delete_session_buffer(&mut self, session_id: i32) -> bool {
        self.inner.pin_mut().deleteSessionBuffer(session_id)
    }

    #[cfg(feature = "archive")]
    pub(crate) fn inner_pin_mut(&mut self) -> std::pin::Pin<&mut ffi::SubscriptionWrapper> {
        self.inner.pin_mut()
    }

    /// The number of active images (one per publisher session) on this subscription.
    pub fn image_count(&self) -> i32 {
        self.inner.imageCount()
    }

    /// Get an image by its index (0-based), or `None` if there is no image at that index.
    /// Images appear in the order they were connected.
    ///
    /// The image borrows this subscription, so the subscription cannot be polled
    /// (or dropped) while the image is alive. Each call returns a new handle with
    /// its own reassembly state (see [`Image::poll_assembled`]).
    pub fn image_by_index(&self, index: usize) -> Option<Image<'_>> {
        Image::from_raw(self.inner.imageByIndex(index))
    }

    /// Get an image by the publisher's session ID, or `None` if no such image exists.
    ///
    /// The image borrows this subscription, like [`image_by_index`](Self::image_by_index).
    pub fn image_by_session_id(&self, session_id: i32) -> Option<Image<'_>> {
        Image::from_raw(self.inner.imageBySessionId(session_id))
    }
}

/// A single publisher session as seen by a subscriber.
///
/// Each publisher session creates one image on each matching subscription.
/// Images track their own position and can be polled independently.
///
/// An `Image` borrows the [`Subscription`] (or `ReplayMerge`) it came from, so it
/// cannot outlive it:
///
/// ```compile_fail,E0597
/// # use aeron_glide::AeronClient;
/// let mut client = AeronClient::new().unwrap();
/// let image = {
///     let sub = client.add_subscription("aeron:ipc", 1).unwrap();
///     sub.image_by_index(0).unwrap()
/// }; // error: `sub` does not live long enough
/// # drop(image);
/// ```
pub struct Image<'a> {
    inner: cxx::UniquePtr<ffi::ImageWrapper>,
    _owner: PhantomData<&'a Subscription>,
}

impl Image<'_> {
    pub(crate) fn from_raw(inner: cxx::UniquePtr<ffi::ImageWrapper>) -> Option<Self> {
        (!inner.is_null()).then_some(Self {
            inner,
            _owner: PhantomData,
        })
    }

    /// The session ID of the publisher that created this image.
    pub fn session_id(&self) -> i32 {
        self.inner.sessionId()
    }

    /// The correlation ID assigned by the media driver when the image was created.
    pub fn correlation_id(&self) -> i64 {
        self.inner.correlationId()
    }

    /// The position at which this image was joined.
    pub fn join_position(&self) -> i64 {
        self.inner.joinPosition()
    }

    /// The source identity string (e.g., `"192.168.1.1:40123"`).
    pub fn source_identity(&self) -> String {
        self.inner.sourceIdentity()
    }

    /// The current consumption position within the stream, or the final position
    /// once the image is closed.
    pub fn position(&self) -> Result<i64> {
        Ok(self.inner.position()?)
    }

    /// Returns `true` if the image has been closed (publisher disconnected or timed out).
    pub fn is_closed(&self) -> bool {
        self.inner.isClosed()
    }

    /// Returns `true` if the publisher has signalled end-of-stream.
    pub fn is_end_of_stream(&self) -> bool {
        self.inner.isEndOfStream()
    }

    /// The position at which the end-of-stream was signalled.
    pub fn end_of_stream_position(&self) -> i64 {
        self.inner.endOfStreamPosition()
    }

    /// Poll this specific image for fragments. Returns the number of fragments dispatched.
    ///
    /// # Panics
    ///
    /// If `handler` panics, the panic is resumed once Aeron returns from the poll;
    /// the remaining fragments of this poll are consumed without being delivered.
    pub fn poll<F>(&mut self, limit: i32, handler: F) -> Result<i32>
    where
        F: FnMut(&[u8]),
    {
        let mut cb = Callback::new(handler);
        let result = self
            .inner
            .pin_mut()
            .poll(limit, callback::fragment::<F>, cb.ctx());
        Ok(cb.finish(result)?)
    }

    /// Poll this image with automatic fragment reassembly, like
    /// [`Subscription::poll_assembled`].
    ///
    /// Reassembly state belongs to this `Image` handle: if a message's fragments
    /// are split across `poll_assembled` calls on different handles for the same
    /// session (e.g. after fetching the image again), the partial message is lost.
    /// Keep one `Image` for as long as you reassemble from it.
    ///
    /// # Panics
    ///
    /// If `handler` panics, the panic is resumed once Aeron returns from the poll;
    /// the fragment being handled is aborted and delivered again by the next poll.
    pub fn poll_assembled<R, F>(&mut self, limit: i32, handler: F) -> Result<i32>
    where
        R: PollAction,
        F: FnMut(&[u8]) -> R,
    {
        let mut cb = Callback::new(handler);
        let result = self.inner.pin_mut().controlledPollAssembled(
            limit,
            callback::controlled_fragment::<F, R>,
            cb.ctx(),
        );
        Ok(cb.finish(result)?)
    }
}

/// Reader for the media driver's CNC (Command and Control) counters.
///
/// Provides access to real-time statistics like bytes sent/received, NAKs,
/// errors, and heartbeats. `Send + Sync`.
pub struct CountersReader {
    inner: cxx::UniquePtr<ffi::CountersReaderWrapper>,
}

impl Drop for CountersReader {
    fn drop(&mut self) {
        callback::drop_outside_conductor(&mut self.inner);
    }
}

// SAFETY: the reader only reads the counters' shared memory (written by the media
// driver with ordered stores), and holds a `shared_ptr<aeron::Aeron>` (atomic
// reference count). All bridged methods are const.
unsafe impl Send for CountersReader {}
unsafe impl Sync for CountersReader {}

impl CountersReader {
    /// The highest counter ID currently allocated.
    pub fn max_counter_id(&self) -> i32 {
        self.inner.maxCounterId()
    }

    /// Read the current value of a counter by ID. Fails for an out-of-range ID.
    pub fn get_counter_value(&self, id: i32) -> Result<i64> {
        Ok(self.inner.getCounterValue(id)?)
    }

    /// Get the state of a counter (e.g., active, inactive). Fails for an out-of-range ID.
    pub fn get_counter_state(&self, id: i32) -> Result<i32> {
        Ok(self.inner.getCounterState(id)?)
    }

    /// Get the type ID of a counter. Fails for an out-of-range ID.
    pub fn get_counter_type_id(&self, id: i32) -> Result<i32> {
        Ok(self.inner.getCounterTypeId(id)?)
    }

    /// Get the human-readable label of a counter. Fails for an out-of-range ID.
    /// Invalid UTF-8 is replaced with `U+FFFD`.
    pub fn get_counter_label(&self, id: i32) -> Result<String> {
        Ok(self.inner.getCounterLabel(id)?)
    }

    /// Iterate over all counters, calling `handler(counter_id, type_id, key_bytes, label)` for each.
    pub fn for_each<F>(&self, handler: F) -> Result<()>
    where
        F: FnMut(i32, i32, &[u8], &str),
    {
        let mut cb = Callback::new(handler);
        let result = self.inner.forEach(callback::counter::<F>, cb.ctx());
        Ok(cb.finish(result)?)
    }
}

impl Default for AeronClient {
    fn default() -> Self {
        Self::new().expect("Failed to create AeronClient")
    }
}

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
pub struct MediaDriver {
    inner: cxx::UniquePtr<ffi::MediaDriverWrapper>,
}

// SAFETY: a started driver runs on its own threads; the Rust handle only reads
// the immutable directory name and closes the driver on drop, which the C driver
// allows from any thread.
unsafe impl Send for MediaDriver {}
unsafe impl Sync for MediaDriver {}

impl MediaDriver {
    /// Configure a new media driver.
    pub fn builder() -> MediaDriverBuilder {
        MediaDriverBuilder::new()
    }

    /// Start a media driver with default settings.
    pub fn launch() -> Result<Self> {
        Self::builder().start()
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

impl MediaDriverBuilder {
    /// A builder with Aeron's defaults (including `AERON_*` environment variables).
    pub fn new() -> Self {
        Self {
            inner: ffi::create_media_driver().map_err(Error::from),
            threading_mode: ThreadingMode::Dedicated,
        }
    }

    /// Apply a setting unless an earlier one already failed.
    fn apply(
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
    /// [`ThreadingMode::Invoker`] is not supported yet: it needs an API to run
    /// the driver's duty cycle, which this crate does not expose.
    pub fn start(self) -> Result<MediaDriver> {
        let mut inner = self.inner?;
        if self.threading_mode == ThreadingMode::Invoker {
            return Err(Error::new(
                ErrorKind::UnsupportedOperation,
                "ThreadingMode::Invoker is not supported yet",
            ));
        }
        inner.pin_mut().start()?;
        Ok(MediaDriver { inner })
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

/// Builder for Aeron channel URIs (`aeron:ipc` or `aeron:udp?key=value|...`).
///
/// # Examples
///
/// ```
/// use aeron_glide::ChannelBuilder;
///
/// let ipc = ChannelBuilder::ipc().build();
/// assert_eq!(ipc, "aeron:ipc");
///
/// let udp = ChannelBuilder::udp()
///     .endpoint("localhost:20121")
///     .mtu(8192)
///     .build();
/// assert_eq!(udp, "aeron:udp?endpoint=localhost:20121|mtu=8192");
/// ```
pub struct ChannelBuilder {
    media: &'static str,
    params: Vec<(String, String)>,
}

impl ChannelBuilder {
    /// Create an IPC (shared memory) channel builder.
    pub fn ipc() -> Self {
        Self {
            media: "ipc",
            params: Vec::new(),
        }
    }

    /// Create a UDP channel builder.
    pub fn udp() -> Self {
        Self {
            media: "udp",
            params: Vec::new(),
        }
    }

    /// Set the endpoint address (e.g., `"localhost:20121"` or `"224.0.1.1:40456"` for multicast).
    pub fn endpoint(self, value: &str) -> Self {
        self.param("endpoint", value)
    }
    pub fn control(self, value: &str) -> Self {
        self.param("control", value)
    }
    pub fn control_mode(self, value: &str) -> Self {
        self.param("control-mode", value)
    }
    pub fn interface(self, value: &str) -> Self {
        self.param("interface", value)
    }
    pub fn mtu(self, bytes: usize) -> Self {
        self.param("mtu", &bytes.to_string())
    }
    pub fn term_length(self, bytes: usize) -> Self {
        self.param("term-length", &bytes.to_string())
    }
    pub fn session_id(self, id: i32) -> Self {
        self.param("session-id", &id.to_string())
    }
    pub fn ttl(self, hops: u8) -> Self {
        self.param("ttl", &hops.to_string())
    }
    pub fn reliable(self, value: bool) -> Self {
        self.param("reliable", if value { "true" } else { "false" })
    }
    pub fn sparse(self, value: bool) -> Self {
        self.param("sparse", if value { "true" } else { "false" })
    }
    pub fn linger(self, ns: u64) -> Self {
        self.param("linger", &ns.to_string())
    }
    pub fn tether(self, value: bool) -> Self {
        self.param("tether", if value { "true" } else { "false" })
    }
    pub fn rejoin(self, value: bool) -> Self {
        self.param("rejoin", if value { "true" } else { "false" })
    }
    pub fn flow_control(self, value: &str) -> Self {
        self.param("fc", value)
    }
    pub fn congestion_control(self, value: &str) -> Self {
        self.param("cc", value)
    }
    pub fn socket_sndbuf(self, bytes: usize) -> Self {
        self.param("so-sndbuf", &bytes.to_string())
    }
    pub fn socket_rcvbuf(self, bytes: usize) -> Self {
        self.param("so-rcvbuf", &bytes.to_string())
    }
    pub fn receiver_window(self, bytes: usize) -> Self {
        self.param("rcv-wnd", &bytes.to_string())
    }

    /// Set an arbitrary channel parameter by key and value.
    pub fn param(mut self, key: &str, value: &str) -> Self {
        self.params.push((key.to_string(), value.to_string()));
        self
    }

    /// Build the channel URI string.
    pub fn build(&self) -> String {
        let mut uri = format!("aeron:{}", self.media);
        for (i, (key, value)) in self.params.iter().enumerate() {
            uri.push(if i == 0 { '?' } else { '|' });
            uri.push_str(key);
            uri.push('=');
            uri.push_str(value);
        }
        uri
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn driver_builder_reports_first_error() {
        let err = MediaDriver::builder()
            .threading_mode(ThreadingMode::Invoker)
            .start()
            .err()
            .expect("invoker mode is rejected");
        assert_eq!(err.kind(), ErrorKind::UnsupportedOperation);
    }

    #[test]
    fn thread_safety_markers() {
        fn send_sync<T: Send + Sync>() {}
        fn send<T: Send>() {}
        send_sync::<AeronClient>();
        send_sync::<Publication>();
        send_sync::<CountersReader>();
        send_sync::<MediaDriver>();
        send::<ExclusivePublication>();
        send::<Subscription>();
    }

    #[test]
    fn test_channel_builder_ipc() {
        assert_eq!(ChannelBuilder::ipc().build(), "aeron:ipc");
    }

    #[test]
    fn test_channel_builder_udp() {
        let uri = ChannelBuilder::udp().endpoint("localhost:20121").build();
        assert_eq!(uri, "aeron:udp?endpoint=localhost:20121");
    }

    #[test]
    fn test_channel_builder_multiple_params() {
        let uri = ChannelBuilder::udp()
            .endpoint("localhost:20121")
            .mtu(8192)
            .term_length(65536)
            .reliable(true)
            .build();
        assert_eq!(
            uri,
            "aeron:udp?endpoint=localhost:20121|mtu=8192|term-length=65536|reliable=true"
        );
    }

    #[test]
    fn test_channel_builder_multicast() {
        let uri = ChannelBuilder::udp()
            .endpoint("224.0.1.1:40456")
            .interface("localhost")
            .ttl(4)
            .build();
        assert_eq!(
            uri,
            "aeron:udp?endpoint=224.0.1.1:40456|interface=localhost|ttl=4"
        );
    }

    #[test]
    fn test_channel_builder_mdc() {
        let uri = ChannelBuilder::udp()
            .control("localhost:40456")
            .control_mode("dynamic")
            .build();
        assert_eq!(
            uri,
            "aeron:udp?control=localhost:40456|control-mode=dynamic"
        );
    }

    #[test]
    fn test_channel_builder_custom_param() {
        let uri = ChannelBuilder::ipc().param("alias", "my-channel").build();
        assert_eq!(uri, "aeron:ipc?alias=my-channel");
    }
}
