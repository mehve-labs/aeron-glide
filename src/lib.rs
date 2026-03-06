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
//! let mut pub1 = client.add_publication("aeron:ipc", 1001).unwrap();
//! let mut sub1 = client.add_subscription("aeron:ipc", 1001).unwrap();
//!
//! // Publish
//! while pub1.offer(b"hello aeron") < 0 {}
//!
//! // Subscribe
//! sub1.poll(10, |data| {
//!     println!("Received: {}", std::str::from_utf8(data).unwrap());
//! });
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
//! # Prerequisites
//!
//! - CMake and a C++14 compiler (Aeron C++ is built from source automatically)
//! - A running Aeron media driver (use the included `mediadriver` binary or [`MediaDriver`])
//! - Java 17+ only if building with `--features archive`
#![cfg_attr(docsrs, feature(doc_cfg))]

#[cfg(feature = "archive")]
#[cfg_attr(docsrs, doc(cfg(feature = "archive")))]
pub mod archive;

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

        fn create_context() -> UniquePtr<ContextWrapper>;
        fn create_aeron(context: UniquePtr<ContextWrapper>) -> Result<UniquePtr<AeronWrapper>>;
        fn create_media_driver() -> Result<UniquePtr<MediaDriverWrapper>>;

        fn start(self: Pin<&mut AeronWrapper>);
        fn isClosed(self: &AeronWrapper) -> bool;
        fn addPublication(
            self: Pin<&mut AeronWrapper>,
            channel: &str,
            stream_id: i32,
        ) -> Result<UniquePtr<PublicationWrapper>>;
        fn addExclusivePublication(
            self: Pin<&mut AeronWrapper>,
            channel: &str,
            stream_id: i32,
        ) -> Result<UniquePtr<ExclusivePublicationWrapper>>;
        fn addSubscription(
            self: Pin<&mut AeronWrapper>,
            channel: &str,
            stream_id: i32,
        ) -> Result<UniquePtr<SubscriptionWrapper>>;
        fn countersReader(self: &AeronWrapper) -> UniquePtr<CountersReaderWrapper>;

        fn start(self: Pin<&mut MediaDriverWrapper>) -> Result<()>;

        fn setDir(self: Pin<&mut MediaDriverWrapper>, dir: &str) -> Result<()>;
        fn setDirDeleteOnStart(self: Pin<&mut MediaDriverWrapper>, value: bool) -> Result<()>;
        fn setDirDeleteOnShutdown(self: Pin<&mut MediaDriverWrapper>, value: bool) -> Result<()>;
        fn setThreadingMode(self: Pin<&mut MediaDriverWrapper>, mode: i32) -> Result<()>;
        fn setConductorIdleStrategy(self: Pin<&mut MediaDriverWrapper>, name: &str) -> Result<()>;
        fn setSenderIdleStrategy(self: Pin<&mut MediaDriverWrapper>, name: &str) -> Result<()>;
        fn setReceiverIdleStrategy(self: Pin<&mut MediaDriverWrapper>, name: &str) -> Result<()>;
        fn setTermBufferLength(self: Pin<&mut MediaDriverWrapper>, value: usize) -> Result<()>;
        fn setIpcTermBufferLength(self: Pin<&mut MediaDriverWrapper>, value: usize) -> Result<()>;
        fn setMtuLength(self: Pin<&mut MediaDriverWrapper>, value: usize) -> Result<()>;
        fn setIpcMtuLength(self: Pin<&mut MediaDriverWrapper>, value: usize) -> Result<()>;
        fn setSocketSoRcvbuf(self: Pin<&mut MediaDriverWrapper>, value: usize) -> Result<()>;
        fn setSocketSoSndbuf(self: Pin<&mut MediaDriverWrapper>, value: usize) -> Result<()>;
        fn setPrintConfiguration(self: Pin<&mut MediaDriverWrapper>, value: bool) -> Result<()>;
        fn setConductorCpuAffinity(self: Pin<&mut MediaDriverWrapper>, cpu_id: i32) -> Result<()>;
        fn setSenderCpuAffinity(self: Pin<&mut MediaDriverWrapper>, cpu_id: i32) -> Result<()>;
        fn setReceiverCpuAffinity(self: Pin<&mut MediaDriverWrapper>, cpu_id: i32) -> Result<()>;

        fn offer(self: Pin<&mut PublicationWrapper>, buffer: &[u8]) -> i64;
        fn tryClaim(self: Pin<&mut PublicationWrapper>, length: usize, handler_id: usize) -> i64;
        fn isConnected(self: &PublicationWrapper) -> bool;
        fn sessionId(self: &PublicationWrapper) -> i32;

        fn offer(self: Pin<&mut ExclusivePublicationWrapper>, buffer: &[u8]) -> i64;
        fn tryClaim(
            self: Pin<&mut ExclusivePublicationWrapper>,
            length: usize,
            handler_id: usize,
        ) -> i64;
        fn isConnected(self: &ExclusivePublicationWrapper) -> bool;

        fn poll(self: Pin<&mut SubscriptionWrapper>, fragment_limit: i32, handler_id: usize)
            -> i32;
        fn pollAssembled(
            self: Pin<&mut SubscriptionWrapper>,
            fragment_limit: i32,
            handler_id: usize,
        ) -> i32;
        fn controlledPollAssembled(
            self: Pin<&mut SubscriptionWrapper>,
            fragment_limit: i32,
            handler_id: usize,
        ) -> i32;
        fn isConnected(self: &SubscriptionWrapper) -> bool;
        fn imageCount(self: &SubscriptionWrapper) -> i32;
        fn imageByIndex(
            self: Pin<&mut SubscriptionWrapper>,
            index: usize,
        ) -> Result<UniquePtr<ImageWrapper>>;
        fn imageBySessionId(
            self: Pin<&mut SubscriptionWrapper>,
            session_id: i32,
        ) -> Result<UniquePtr<ImageWrapper>>;

        type ImageWrapper;
        fn sessionId(self: &ImageWrapper) -> i32;
        fn correlationId(self: &ImageWrapper) -> i64;
        fn joinPosition(self: &ImageWrapper) -> i64;
        fn sourceIdentity(self: &ImageWrapper) -> String;
        fn position(self: &ImageWrapper) -> i64;
        fn setPosition(self: Pin<&mut ImageWrapper>, new_position: i64);
        fn isClosed(self: &ImageWrapper) -> bool;
        fn isEndOfStream(self: &ImageWrapper) -> bool;
        fn endOfStreamPosition(self: &ImageWrapper) -> i64;
        fn poll(self: Pin<&mut ImageWrapper>, fragment_limit: i32, handler_id: usize) -> i32;
        fn controlledPollAssembled(
            self: Pin<&mut ImageWrapper>,
            fragment_limit: i32,
            handler_id: usize,
        ) -> i32;

        fn maxCounterId(self: &CountersReaderWrapper) -> i32;
        fn getCounterValue(self: &CountersReaderWrapper, id: i32) -> i64;
        fn getCounterState(self: &CountersReaderWrapper, id: i32) -> i32;
        fn getCounterTypeId(self: &CountersReaderWrapper, id: i32) -> i32;
        fn getCounterLabel(self: &CountersReaderWrapper, id: i32) -> String;
        fn forEach(self: &CountersReaderWrapper, handler_id: usize);
    }

    extern "Rust" {
        fn handle_fragment(handler_id: usize, buffer: &[u8]);
        fn handle_controlled_fragment(handler_id: usize, buffer: &[u8]) -> i32;
        fn handle_claim(handler_id: usize, buffer: &mut [u8]) -> bool;
        fn handle_counters_metadata(
            handler_id: usize,
            counter_id: i32,
            type_id: i32,
            key: &[u8],
            label: String,
        );
    }
}

/// Aeron client — the main entry point for creating publications and subscriptions.
///
/// Each client maintains its own connection to the media driver. You can create
/// multiple clients in the same process (e.g., one per thread).
pub struct AeronClient {
    inner: cxx::UniquePtr<ffi::AeronWrapper>,
}

impl AeronClient {
    /// Create a new Aeron client connected to the media driver.
    pub fn new() -> Result<Self, Box<dyn std::error::Error>> {
        let ctx = ffi::create_context();
        let aeron = ffi::create_aeron(ctx)?;

        Ok(Self { inner: aeron })
    }

    /// Start the client conductor thread.
    pub fn start(&mut self) {
        self.inner.pin_mut().start();
    }

    /// Returns `true` if the client has been closed.
    pub fn is_closed(&self) -> bool {
        self.inner.isClosed()
    }

    /// Add a concurrent publication on the given channel and stream ID.
    /// Multiple publishers can share the same channel+stream.
    pub fn add_publication(
        &mut self,
        channel: &str,
        stream_id: i32,
    ) -> Result<Publication, Box<dyn std::error::Error>> {
        let pub_inner = self.inner.pin_mut().addPublication(channel, stream_id)?;
        Ok(Publication { inner: pub_inner })
    }

    /// Add an exclusive publication on the given channel and stream ID.
    /// Only one publisher is allowed per session — lower overhead than concurrent.
    pub fn add_exclusive_publication(
        &mut self,
        channel: &str,
        stream_id: i32,
    ) -> Result<ExclusivePublication, Box<dyn std::error::Error>> {
        let pub_inner = self
            .inner
            .pin_mut()
            .addExclusivePublication(channel, stream_id)?;
        Ok(ExclusivePublication { inner: pub_inner })
    }

    /// Add a subscription on the given channel and stream ID.
    pub fn add_subscription(
        &mut self,
        channel: &str,
        stream_id: i32,
    ) -> Result<Subscription, Box<dyn std::error::Error>> {
        let sub_inner = self.inner.pin_mut().addSubscription(channel, stream_id)?;
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
/// Returns negative values from [`offer`](Publication::offer) on back-pressure or when closed.
pub struct Publication {
    inner: cxx::UniquePtr<ffi::PublicationWrapper>,
}

impl Publication {
    /// Publish a message. Returns the new stream position on success,
    /// or a negative value on back-pressure / not connected / closed.
    pub fn offer(&mut self, buffer: &[u8]) -> i64 {
        self.inner.pin_mut().offer(buffer)
    }

    /// Zero-copy publish: claims a region of the log buffer, calls `handler` with a mutable
    /// slice pointing directly into shared memory, then commits or aborts based on the return value.
    /// Returns the stream position (>0 on success, negative on back-pressure/closed).
    pub fn try_claim<F>(&mut self, length: usize, mut handler: F) -> i64
    where
        F: FnMut(&mut [u8]) -> bool,
    {
        let handler_id = &handler as *const _ as usize;
        let mut_ptr: *mut (dyn FnMut(&mut [u8]) -> bool + 'static) = unsafe {
            std::mem::transmute::<
                *mut dyn FnMut(&mut [u8]) -> bool,
                *mut (dyn FnMut(&mut [u8]) -> bool + 'static),
            >(&mut handler as *mut dyn FnMut(&mut [u8]) -> bool)
        };

        CLAIM_HANDLERS.with(|handlers| {
            handlers.borrow_mut().insert(handler_id, mut_ptr);
        });

        let result = self.inner.pin_mut().tryClaim(length, handler_id);

        CLAIM_HANDLERS.with(|handlers| {
            handlers.borrow_mut().remove(&handler_id);
        });

        result
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
pub struct ExclusivePublication {
    inner: cxx::UniquePtr<ffi::ExclusivePublicationWrapper>,
}

impl ExclusivePublication {
    /// Publish a message. Returns the new stream position on success,
    /// or a negative value on back-pressure / not connected / closed.
    pub fn offer(&mut self, buffer: &[u8]) -> i64 {
        self.inner.pin_mut().offer(buffer)
    }

    /// Zero-copy publish: claims a region of the log buffer, calls `handler` with a mutable
    /// slice pointing directly into shared memory, then commits or aborts based on the return value.
    /// Returns the stream position (>0 on success, negative on back-pressure/closed).
    pub fn try_claim<F>(&mut self, length: usize, mut handler: F) -> i64
    where
        F: FnMut(&mut [u8]) -> bool,
    {
        let handler_id = &handler as *const _ as usize;
        let mut_ptr: *mut (dyn FnMut(&mut [u8]) -> bool + 'static) = unsafe {
            std::mem::transmute::<
                *mut dyn FnMut(&mut [u8]) -> bool,
                *mut (dyn FnMut(&mut [u8]) -> bool + 'static),
            >(&mut handler as *mut dyn FnMut(&mut [u8]) -> bool)
        };

        CLAIM_HANDLERS.with(|handlers| {
            handlers.borrow_mut().insert(handler_id, mut_ptr);
        });

        let result = self.inner.pin_mut().tryClaim(length, handler_id);

        CLAIM_HANDLERS.with(|handlers| {
            handlers.borrow_mut().remove(&handler_id);
        });

        result
    }

    /// Returns `true` if there is at least one subscriber connected to this publication.
    pub fn is_connected(&self) -> bool {
        self.inner.isConnected()
    }
}

use std::cell::RefCell;
use std::collections::HashMap;

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

type FragmentHandlerMap = RefCell<HashMap<usize, *mut dyn FnMut(&[u8])>>;
type ClaimHandlerMap = RefCell<HashMap<usize, *mut dyn FnMut(&mut [u8]) -> bool>>;
type ControlledHandlerMap = RefCell<HashMap<usize, *mut dyn FnMut(&[u8]) -> ControlledAction>>;

// Thread-local registries for closures passed across the cxx boundary.
// We use pointer-based handler IDs since cxx doesn't support passing trait objects directly.
thread_local! {
    pub(crate) static HANDLERS: FragmentHandlerMap = RefCell::new(HashMap::new());
    static CLAIM_HANDLERS: ClaimHandlerMap = RefCell::new(HashMap::new());
    static CONTROLLED_HANDLERS: ControlledHandlerMap = RefCell::new(HashMap::new());
}

fn handle_fragment(handler_id: usize, buffer: &[u8]) {
    HANDLERS.with(|handlers| {
        if let Some(handler_ptr) = handlers.borrow_mut().get_mut(&handler_id) {
            unsafe {
                let handler = &mut **handler_ptr;
                handler(buffer);
            }
        }
    });
}

fn handle_controlled_fragment(handler_id: usize, buffer: &[u8]) -> i32 {
    CONTROLLED_HANDLERS.with(|handlers| {
        if let Some(handler_ptr) = handlers.borrow_mut().get_mut(&handler_id) {
            unsafe {
                let handler = &mut **handler_ptr;
                handler(buffer) as i32
            }
        } else {
            ControlledAction::Abort as i32
        }
    })
}

fn handle_claim(handler_id: usize, buffer: &mut [u8]) -> bool {
    CLAIM_HANDLERS.with(|handlers| {
        if let Some(handler_ptr) = handlers.borrow_mut().get_mut(&handler_id) {
            unsafe {
                let handler = &mut **handler_ptr;
                handler(buffer)
            }
        } else {
            false // abort if handler not found
        }
    })
}

/// A subscription for receiving messages on a channel+stream.
pub struct Subscription {
    inner: cxx::UniquePtr<ffi::SubscriptionWrapper>,
}

impl Subscription {
    /// Poll for new messages, calling `handler` for each fragment received.
    /// Returns the number of fragments dispatched.
    pub fn poll<F>(&mut self, limit: i32, mut handler: F) -> i32
    where
        F: FnMut(&[u8]),
    {
        let handler_id = &handler as *const _ as usize;
        let mut_ptr: *mut (dyn FnMut(&[u8]) + 'static) = unsafe {
            std::mem::transmute::<*mut dyn FnMut(&[u8]), *mut (dyn FnMut(&[u8]) + 'static)>(
                &mut handler as *mut dyn FnMut(&[u8]),
            )
        };

        HANDLERS.with(|handlers| {
            handlers.borrow_mut().insert(handler_id, mut_ptr);
        });

        let result = self.inner.pin_mut().poll(limit, handler_id);

        HANDLERS.with(|handlers| {
            handlers.borrow_mut().remove(&handler_id);
        });

        result
    }

    /// Poll with automatic fragment reassembly. Messages that span multiple fragments
    /// are reassembled before being delivered to the handler, which always receives
    /// complete messages.
    ///
    /// The handler can return `()` (maps to Continue) or a `ControlledAction` for
    /// flow-control (Abort to retry, Break to stop, Commit to checkpoint, Continue to proceed).
    pub fn poll_assembled<R, F>(&mut self, limit: i32, mut handler: F) -> i32
    where
        R: PollAction,
        F: FnMut(&[u8]) -> R,
    {
        // Wrap the user's handler to always produce a ControlledAction
        let mut controlled = |data: &[u8]| -> ControlledAction { handler(data).into_action() };

        let handler_id = &controlled as *const _ as usize;
        let mut_ptr: *mut (dyn FnMut(&[u8]) -> ControlledAction + 'static) = unsafe {
            std::mem::transmute::<
                *mut dyn FnMut(&[u8]) -> ControlledAction,
                *mut (dyn FnMut(&[u8]) -> ControlledAction + 'static),
            >(&mut controlled as *mut dyn FnMut(&[u8]) -> ControlledAction)
        };

        CONTROLLED_HANDLERS.with(|handlers| {
            handlers.borrow_mut().insert(handler_id, mut_ptr);
        });

        let result = self
            .inner
            .pin_mut()
            .controlledPollAssembled(limit, handler_id);

        CONTROLLED_HANDLERS.with(|handlers| {
            handlers.borrow_mut().remove(&handler_id);
        });

        result
    }

    /// Returns `true` if there is at least one publisher connected to this subscription.
    pub fn is_connected(&self) -> bool {
        self.inner.isConnected()
    }

    #[cfg(feature = "archive")]
    pub(crate) fn inner_pin_mut(&mut self) -> std::pin::Pin<&mut ffi::SubscriptionWrapper> {
        self.inner.pin_mut()
    }

    /// The number of active images (one per publisher session) on this subscription.
    pub fn image_count(&self) -> i32 {
        self.inner.imageCount()
    }

    /// Get an image by its index (0-based). Images appear in the order they were connected.
    pub fn image_by_index(&mut self, index: usize) -> Result<Image, Box<dyn std::error::Error>> {
        let img = self.inner.pin_mut().imageByIndex(index)?;
        Ok(Image { inner: img })
    }

    /// Get an image by the publisher's session ID.
    pub fn image_by_session_id(
        &mut self,
        session_id: i32,
    ) -> Result<Image, Box<dyn std::error::Error>> {
        let img = self.inner.pin_mut().imageBySessionId(session_id)?;
        Ok(Image { inner: img })
    }
}

/// A single publisher session as seen by a subscriber.
///
/// Each publisher session creates one image on each matching subscription.
/// Images track their own position and can be polled independently.
pub struct Image {
    inner: cxx::UniquePtr<ffi::ImageWrapper>,
}

impl Image {
    #[cfg(feature = "archive")]
    pub(crate) fn from_raw(inner: cxx::UniquePtr<ffi::ImageWrapper>) -> Self {
        Self { inner }
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

    /// The current consumption position within the stream.
    pub fn position(&self) -> i64 {
        self.inner.position()
    }

    /// Set the subscriber position (e.g., to skip ahead or rewind within the term buffer).
    pub fn set_position(&mut self, new_position: i64) {
        self.inner.pin_mut().setPosition(new_position);
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
    pub fn poll<F>(&mut self, limit: i32, mut handler: F) -> i32
    where
        F: FnMut(&[u8]),
    {
        let handler_id = &handler as *const _ as usize;
        let mut_ptr: *mut (dyn FnMut(&[u8]) + 'static) = unsafe {
            std::mem::transmute::<*mut dyn FnMut(&[u8]), *mut (dyn FnMut(&[u8]) + 'static)>(
                &mut handler as *mut dyn FnMut(&[u8]),
            )
        };

        HANDLERS.with(|handlers| {
            handlers.borrow_mut().insert(handler_id, mut_ptr);
        });

        let result = self.inner.pin_mut().poll(limit, handler_id);

        HANDLERS.with(|handlers| {
            handlers.borrow_mut().remove(&handler_id);
        });

        result
    }

    pub fn poll_assembled<R, F>(&mut self, limit: i32, mut handler: F) -> i32
    where
        R: PollAction,
        F: FnMut(&[u8]) -> R,
    {
        let mut controlled = |data: &[u8]| -> ControlledAction { handler(data).into_action() };

        let handler_id = &controlled as *const _ as usize;
        let mut_ptr: *mut (dyn FnMut(&[u8]) -> ControlledAction + 'static) = unsafe {
            std::mem::transmute::<
                *mut dyn FnMut(&[u8]) -> ControlledAction,
                *mut (dyn FnMut(&[u8]) -> ControlledAction + 'static),
            >(&mut controlled as *mut dyn FnMut(&[u8]) -> ControlledAction)
        };

        CONTROLLED_HANDLERS.with(|handlers| {
            handlers.borrow_mut().insert(handler_id, mut_ptr);
        });

        let result = self
            .inner
            .pin_mut()
            .controlledPollAssembled(limit, handler_id);

        CONTROLLED_HANDLERS.with(|handlers| {
            handlers.borrow_mut().remove(&handler_id);
        });

        result
    }
}

/// Reader for the media driver's CNC (Command and Control) counters.
///
/// Provides access to real-time statistics like bytes sent/received, NAKs,
/// errors, and heartbeats.
pub struct CountersReader {
    inner: cxx::UniquePtr<ffi::CountersReaderWrapper>,
}

type MetadataHandlerMap = RefCell<HashMap<usize, *mut dyn FnMut(i32, i32, &[u8], &str)>>;

thread_local! {
    static METADATA_HANDLERS: MetadataHandlerMap = RefCell::new(HashMap::new());
}

fn handle_counters_metadata(
    handler_id: usize,
    counter_id: i32,
    type_id: i32,
    key: &[u8],
    label: String,
) {
    METADATA_HANDLERS.with(|handlers| {
        if let Some(handler_ptr) = handlers.borrow_mut().get_mut(&handler_id) {
            unsafe {
                let handler = &mut **handler_ptr;
                handler(counter_id, type_id, key, &label);
            }
        }
    });
}

impl CountersReader {
    /// The highest counter ID currently allocated.
    pub fn max_counter_id(&self) -> i32 {
        self.inner.maxCounterId()
    }

    /// Read the current value of a counter by ID.
    pub fn get_counter_value(&self, id: i32) -> i64 {
        self.inner.getCounterValue(id)
    }

    /// Get the state of a counter (e.g., active, inactive).
    pub fn get_counter_state(&self, id: i32) -> i32 {
        self.inner.getCounterState(id)
    }

    /// Get the type ID of a counter.
    pub fn get_counter_type_id(&self, id: i32) -> i32 {
        self.inner.getCounterTypeId(id)
    }

    /// Get the human-readable label of a counter.
    pub fn get_counter_label(&self, id: i32) -> String {
        self.inner.getCounterLabel(id)
    }

    /// Iterate over all counters, calling `handler(counter_id, type_id, key_bytes, label)` for each.
    pub fn for_each<F>(&self, mut handler: F)
    where
        F: FnMut(i32, i32, &[u8], &str),
    {
        let handler_id = &handler as *const _ as usize;
        #[allow(clippy::type_complexity)]
        let mut_ptr: *mut (dyn FnMut(i32, i32, &[u8], &str) + 'static) = unsafe {
            std::mem::transmute::<
                *mut dyn FnMut(i32, i32, &[u8], &str),
                *mut (dyn FnMut(i32, i32, &[u8], &str) + 'static),
            >(&mut handler as *mut dyn FnMut(i32, i32, &[u8], &str))
        };

        METADATA_HANDLERS.with(|handlers| {
            handlers.borrow_mut().insert(handler_id, mut_ptr);
        });

        self.inner.forEach(handler_id);

        METADATA_HANDLERS.with(|handlers| {
            handlers.borrow_mut().remove(&handler_id);
        });
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
pub struct MediaDriver {
    inner: cxx::UniquePtr<ffi::MediaDriverWrapper>,
}

impl MediaDriver {
    /// Create a new media driver with default settings.
    pub fn new() -> Result<Self, Box<dyn std::error::Error>> {
        let inner = ffi::create_media_driver()?;
        Ok(Self { inner })
    }

    /// Start the media driver. Must be called before any clients can connect.
    pub fn start(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        self.inner.pin_mut().start()?;
        Ok(())
    }

    /// Set the Aeron directory for shared memory files.
    pub fn set_dir(&mut self, dir: &str) -> Result<(), Box<dyn std::error::Error>> {
        self.inner.pin_mut().setDir(dir)?;
        Ok(())
    }

    pub fn set_dir_delete_on_start(
        &mut self,
        value: bool,
    ) -> Result<(), Box<dyn std::error::Error>> {
        self.inner.pin_mut().setDirDeleteOnStart(value)?;
        Ok(())
    }

    pub fn set_dir_delete_on_shutdown(
        &mut self,
        value: bool,
    ) -> Result<(), Box<dyn std::error::Error>> {
        self.inner.pin_mut().setDirDeleteOnShutdown(value)?;
        Ok(())
    }

    pub fn set_threading_mode(
        &mut self,
        mode: ThreadingMode,
    ) -> Result<(), Box<dyn std::error::Error>> {
        self.inner.pin_mut().setThreadingMode(mode as i32)?;
        Ok(())
    }

    pub fn set_conductor_idle_strategy(
        &mut self,
        strategy: IdleStrategy,
    ) -> Result<(), Box<dyn std::error::Error>> {
        self.inner
            .pin_mut()
            .setConductorIdleStrategy(strategy.as_str())?;
        Ok(())
    }

    pub fn set_sender_idle_strategy(
        &mut self,
        strategy: IdleStrategy,
    ) -> Result<(), Box<dyn std::error::Error>> {
        self.inner
            .pin_mut()
            .setSenderIdleStrategy(strategy.as_str())?;
        Ok(())
    }

    pub fn set_receiver_idle_strategy(
        &mut self,
        strategy: IdleStrategy,
    ) -> Result<(), Box<dyn std::error::Error>> {
        self.inner
            .pin_mut()
            .setReceiverIdleStrategy(strategy.as_str())?;
        Ok(())
    }

    pub fn set_term_buffer_length(
        &mut self,
        value: usize,
    ) -> Result<(), Box<dyn std::error::Error>> {
        self.inner.pin_mut().setTermBufferLength(value)?;
        Ok(())
    }

    pub fn set_ipc_term_buffer_length(
        &mut self,
        value: usize,
    ) -> Result<(), Box<dyn std::error::Error>> {
        self.inner.pin_mut().setIpcTermBufferLength(value)?;
        Ok(())
    }

    pub fn set_mtu_length(&mut self, value: usize) -> Result<(), Box<dyn std::error::Error>> {
        self.inner.pin_mut().setMtuLength(value)?;
        Ok(())
    }

    pub fn set_ipc_mtu_length(&mut self, value: usize) -> Result<(), Box<dyn std::error::Error>> {
        self.inner.pin_mut().setIpcMtuLength(value)?;
        Ok(())
    }

    pub fn set_socket_so_rcvbuf(&mut self, value: usize) -> Result<(), Box<dyn std::error::Error>> {
        self.inner.pin_mut().setSocketSoRcvbuf(value)?;
        Ok(())
    }

    pub fn set_socket_so_sndbuf(&mut self, value: usize) -> Result<(), Box<dyn std::error::Error>> {
        self.inner.pin_mut().setSocketSoSndbuf(value)?;
        Ok(())
    }

    pub fn set_print_configuration(
        &mut self,
        value: bool,
    ) -> Result<(), Box<dyn std::error::Error>> {
        self.inner.pin_mut().setPrintConfiguration(value)?;
        Ok(())
    }

    pub fn set_conductor_cpu_affinity(
        &mut self,
        cpu_id: i32,
    ) -> Result<(), Box<dyn std::error::Error>> {
        self.inner.pin_mut().setConductorCpuAffinity(cpu_id)?;
        Ok(())
    }

    pub fn set_sender_cpu_affinity(
        &mut self,
        cpu_id: i32,
    ) -> Result<(), Box<dyn std::error::Error>> {
        self.inner.pin_mut().setSenderCpuAffinity(cpu_id)?;
        Ok(())
    }

    pub fn set_receiver_cpu_affinity(
        &mut self,
        cpu_id: i32,
    ) -> Result<(), Box<dyn std::error::Error>> {
        self.inner.pin_mut().setReceiverCpuAffinity(cpu_id)?;
        Ok(())
    }
}

impl Default for MediaDriver {
    fn default() -> Self {
        Self::new().expect("Failed to create MediaDriver")
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
    fn test_aeron_creation_with_driver() {
        // 1. Start embedded driver
        let mut driver = MediaDriver::new().expect("Failed to create MediaDriver");
        driver.start().expect("Failed to start MediaDriver");

        // Wait a tiny bit for the driver to spin up its files in /dev/shm
        std::thread::sleep(std::time::Duration::from_millis(100));

        // 2. Connect client
        let mut client = AeronClient::new().expect("Failed to connect to media driver");
        client.start();
        assert!(!client.is_closed());

        // 3. Test Pub/Sub creation
        let mut publ = client
            .add_publication("aeron:ipc", 10)
            .expect("add pub failed");
        let mut sub = client
            .add_subscription("aeron:ipc", 10)
            .expect("add sub failed");

        // 4. Wait for connection then test Image API
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while !sub.is_connected() && std::time::Instant::now() < deadline {
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert!(sub.is_connected(), "subscription should connect");

        // Publish a message so the image is active
        while publ.offer(b"hello") < 0 {
            std::thread::yield_now();
        }

        assert_eq!(sub.image_count(), 1);
        let image = sub.image_by_index(0).expect("image_by_index failed");
        assert!(image.session_id() != 0);
        assert!(image.position() >= 0);
        assert!(!image.is_closed());
        assert!(!image.is_end_of_stream());

        // Test image_by_session_id
        let sid = image.session_id();
        let image2 = sub
            .image_by_session_id(sid)
            .expect("image_by_session_id failed");
        assert_eq!(image2.session_id(), sid);
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
