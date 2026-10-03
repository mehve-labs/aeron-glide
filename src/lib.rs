#![cfg_attr(
    not(feature = "driver"),
    doc = "[`MediaDriver`]: https://docs.rs/aeron-glide/latest/aeron_glide/struct.MediaDriver.html"
)]
#![cfg_attr(
    not(feature = "archive"),
    doc = "[`archive`]: https://docs.rs/aeron-glide/latest/aeron_glide/archive/index.html"
)]
//! Safe, idiomatic Rust API for [Aeron](https://github.com/aeron-io/aeron): the
//! client, the embedded C media driver and the archive client, built on Aeron's
//! C++ API with [`cxx`](https://cxx.rs/) (and its C API where C++ lacks a call).
//!
//! # Quick start
//!
//! ```no_run
//! use aeron_glide::AeronClient;
//!
//! let client = AeronClient::new()?; // connects to a running media driver
//! let publication = client.add_publication("aeron:ipc", 1001)?;
//! let mut subscription = client.add_subscription("aeron:ipc", 1001)?;
//!
//! // Retry while not connected or back pressured; other errors are real.
//! while let Err(e) = publication.offer(b"hello aeron") {
//!     assert!(e.is_retryable(), "offer failed: {e}");
//! }
//!
//! subscription.poll(10, |data, _header| {
//!     println!("received {}", String::from_utf8_lossy(data));
//! })?;
//! # Ok::<(), aeron_glide::Error>(())
//! ```
//!
//! # What it covers
//!
//! - **Publications** ([`Publication`], [`ExclusivePublication`]): `offer`,
//!   vectored offers and zero-copy [`Publication::try_claim`]
//! - **Subscriptions** ([`Subscription`]) with fragment reassembly
//!   ([`Subscription::poll_assembled`]), controlled polling ([`ControlledAction`])
//!   and per-session [`Image`]s
//! - **Channels**: build URIs with [`ChannelBuilder`], parse them with [`ChannelUri`]
//! - **Counters**: create your own ([`Counter`]), read the driver's
//!   ([`CountersReader`]), or map its CnC file without a client ([`CncFile`])
//! - **Embedded media driver** ([`MediaDriver`], `driver` feature, on by
//!   default): every driver setting, threaded or invoker mode, termination handlers
//! - **Agents** ([`concurrent`]): idle strategies, `AgentRunner` and `AgentInvoker`
//! - **Archive client** ([`archive`], `archive` feature): recording, replay,
//!   replication, queries, `ReplayMerge` and `PersistentSubscription`
//!
//! # Thread safety
//!
//! | Type | `Send` | `Sync` |
//! |---|---|---|
//! | [`AeronClient`], [`Publication`], [`Counter`], [`CountersReader`], [`CncFile`], [`MediaDriver`] | yes | yes |
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
//! # Handlers and panics
//!
//! A panic must not unwind through Aeron, so closures passed to it are wrapped:
//!
//! - **Poll handlers**: the panic is resumed once Aeron returns from the poll.
//!   The remaining fragments of that poll are consumed without being delivered
//!   (a controlled handler's fragment is aborted instead, and delivered again).
//! - **Reserved value suppliers**: the message is still published, then the
//!   panic is resumed.
//! - **Client, driver and archive handlers**, which run on Aeron's conductor
//!   threads: the panic is caught and printed to stderr.
//!
//! Handlers may drop what they own, including the client: the drop is moved off
//! Aeron's thread when needed. Calls Aeron cannot make from inside a handler
//! fail with [`ErrorKind::Reentrant`] instead of deadlocking.
//!
//! # Prerequisites
//!
//! - CMake 3.30+ and a C++17 compiler (Aeron is built from source automatically)
//! - A running media driver: the `mediadriver` binary (`bin` feature), a
//!   [`MediaDriver`] in your process, or Aeron's Java or C driver
//! - Java 17+, only with the `archive` feature
#![cfg_attr(docsrs, feature(doc_cfg))]

#[cfg(feature = "archive")]
#[cfg_attr(docsrs, doc(cfg(feature = "archive")))]
pub mod archive;

mod callback;
pub mod channel;
mod client;
pub mod concurrent;
mod context;
pub mod counter_types;
mod counters;
#[cfg(feature = "driver")]
mod driver;
#[cfg(feature = "driver")]
mod driver_gen;
mod error;
mod handlers;
mod header;
mod image;
mod publication;
mod subscription;
use callback::Callback;
pub use channel::{ChannelBuilder, ChannelUri, ControlMode};
pub use client::{AeronClient, PendingAdd};
pub use context::Context;
pub use counters::{
    CncConstants, CncFile, Counter, CounterView, CountersReader, ErrorLogEntry, LossReportEntry,
    heartbeat_timestamp,
};
#[cfg(feature = "driver")]
#[cfg_attr(docsrs, doc(cfg(feature = "driver")))]
pub use driver::{DriverIdleStrategy, MediaDriver, MediaDriverBuilder, ThreadingMode};
#[cfg(feature = "driver")]
#[cfg_attr(docsrs, doc(cfg(feature = "driver")))]
pub use driver_gen::{InferableBoolean, ThreadNaming};
pub use error::{Error, ErrorKind, OfferError, Result};
pub use handlers::{
    CounterEvent, ImageEvent, NewPublication, NewSubscription, PublicationErrorFrame,
};
pub use header::Header;
pub use image::Image;
pub use publication::{BufferClaim, ChannelStatus, ExclusivePublication, Publication};
use std::marker::PhantomData;
pub use subscription::{ControlledAction, PollAction, Subscription};

/// The version and build of the Aeron C library this crate is built with (C++
/// `Aeron::version()`), e.g. `"aeron version=1.53.3 commit=..."`.
pub fn aeron_version() -> String {
    ffi::aeronVersion()
}

/// Milliseconds since the epoch (C `aeron_epoch_clock`), the clock of Aeron's
/// timestamps (e.g. the CnC heartbeat and the error log).
pub fn epoch_clock() -> i64 {
    ffi::epochClock()
}

/// A monotonic clock in nanoseconds (C `aeron_nano_clock`), the one the client
/// and driver use for timeouts.
pub fn nano_clock() -> i64 {
    ffi::nanoClock()
}

/// The longest timeout handed to Aeron, about 73 years. Aeron adds timeouts to
/// its clocks (`now + timeout`, in milliseconds or nanoseconds) in signed 64-bit
/// arithmetic, so larger ones overflow into a deadline in the past: a request
/// would time out at once, and the client could then free a command its
/// conductor still holds.
pub(crate) const MAX_TIMEOUT_NS: i64 = i64::MAX / 4;

/// `timeout` in milliseconds, at most [`MAX_TIMEOUT_NS`].
pub(crate) fn timeout_millis(timeout: std::time::Duration) -> i64 {
    timeout
        .as_millis()
        .min((MAX_TIMEOUT_NS / 1_000_000) as u128) as i64
}

/// `timeout` in nanoseconds, at most [`MAX_TIMEOUT_NS`].
#[cfg(feature = "archive")]
pub(crate) fn timeout_nanos(timeout: std::time::Duration) -> i64 {
    timeout.as_nanos().min(MAX_TIMEOUT_NS as u128) as i64
}

// Callbacks crossing the bridge are plain `fn` types: no aliases there.
#[allow(clippy::type_complexity)]
#[cxx::bridge(namespace = "aeron_rs")]
pub(crate) mod ffi {
    /// One part of a vectored offer: the address and length of a byte slice.
    #[derive(Clone, Copy)]
    struct OfferPart {
        ptr: usize,
        len: usize,
    }

    /// An image passed to an available/unavailable image handler.
    struct ImageInfo {
        session_id: i32,
        correlation_id: i64,
        subscription_registration_id: i64,
        join_position: i64,
        position: i64,
        initial_term_id: i32,
        term_buffer_length: i32,
        position_bits_to_shift: i32,
        source_identity: String,
    }

    /// The constants of a CnC file (`aeron_cnc_constants_t`).
    struct CncConstants {
        cnc_version: i32,
        to_driver_buffer_length: i32,
        to_clients_buffer_length: i32,
        counter_metadata_buffer_length: i32,
        counter_values_buffer_length: i32,
        error_log_buffer_length: i32,
        client_liveness_timeout_ns: i64,
        start_timestamp_ms: i64,
        pid: i64,
        file_page_size: i32,
    }

    /// A claimed frame (header included): its address and length.
    #[derive(Clone, Copy)]
    struct ClaimFrame {
        ptr: usize,
        len: usize,
    }

    unsafe extern "C++" {
        include!("shim.h");

        /// The C++ `aeron::concurrent::logbuffer::Header` of a fragment.
        #[namespace = "aeron::concurrent::logbuffer"]
        type Header;
        fn initialTermId(self: &Header) -> i32;
        fn frameLength(self: &Header) -> i32;
        fn sessionId(self: &Header) -> i32;
        fn streamId(self: &Header) -> i32;
        fn termId(self: &Header) -> i32;
        fn termOffset(self: &Header) -> i32;
        #[cxx_name = "type"]
        fn headerType(self: &Header) -> u16;
        fn flags(self: &Header) -> u8;
        fn position(self: &Header) -> i64;
        fn positionBitsToShift(self: &Header) -> i32;
        fn reservedValue(self: &Header) -> i64;

        fn claimCommit(frame: ClaimFrame);
        fn claimAbort(frame: ClaimFrame);
        fn claimFlags(frame: ClaimFrame) -> u8;
        fn claimSetFlags(frame: ClaimFrame, flags: u8);
        fn claimHeaderType(frame: ClaimFrame) -> u16;
        fn claimSetHeaderType(frame: ClaimFrame, header_type: u16);
        fn claimReservedValue(frame: ClaimFrame) -> i64;
        fn claimSetReservedValue(frame: ClaimFrame, value: i64);

        type ContextWrapper;
        type AeronWrapper;
        type PublicationWrapper;
        type ExclusivePublicationWrapper;
        type SubscriptionWrapper;
        type CountersReaderWrapper;
        type CounterWrapper;
        type CncFileWrapper;

        fn create_context() -> Result<UniquePtr<ContextWrapper>>;
        fn requestDriverTermination(directory: &str, token: &[u8]) -> Result<bool>;
        fn defaultAeronPath() -> Result<String>;
        fn aeronVersion() -> String;
        fn epochClock() -> i64;
        fn nanoClock() -> i64;
        fn setAeronDir(self: Pin<&mut ContextWrapper>, dir: &str) -> Result<()>;
        fn setClientName(self: Pin<&mut ContextWrapper>, name: &str) -> Result<()>;
        fn setDriverTimeoutMs(self: Pin<&mut ContextWrapper>, value: i64) -> Result<()>;
        fn setResourceLingerTimeoutMs(self: Pin<&mut ContextWrapper>, value: i64) -> Result<()>;
        fn setIdleSleepDurationMs(self: Pin<&mut ContextWrapper>, value: i64) -> Result<()>;
        fn setPreTouchMappedMemory(self: Pin<&mut ContextWrapper>, value: bool) -> Result<()>;
        fn setUseConductorAgentInvoker(self: Pin<&mut ContextWrapper>, value: bool) -> Result<()>;
        fn setErrorHandler(
            self: Pin<&mut ContextWrapper>,
            handler: fn(usize, &[u8]),
            release: fn(usize),
            ctx: usize,
        );
        fn setAvailableImageHandler(
            self: Pin<&mut ContextWrapper>,
            handler: fn(usize, &ImageInfo),
            release: fn(usize),
            ctx: usize,
        );
        fn setUnavailableImageHandler(
            self: Pin<&mut ContextWrapper>,
            handler: fn(usize, &ImageInfo),
            release: fn(usize),
            ctx: usize,
        );
        fn setNewPublicationHandler(
            self: Pin<&mut ContextWrapper>,
            handler: fn(usize, &[u8], i32, i32, i64),
            release: fn(usize),
            ctx: usize,
        );
        fn setNewExclusivePublicationHandler(
            self: Pin<&mut ContextWrapper>,
            handler: fn(usize, &[u8], i32, i32, i64),
            release: fn(usize),
            ctx: usize,
        );
        fn setNewSubscriptionHandler(
            self: Pin<&mut ContextWrapper>,
            handler: fn(usize, &[u8], i32, i64),
            release: fn(usize),
            ctx: usize,
        );
        fn setAvailableCounterHandler(
            self: Pin<&mut ContextWrapper>,
            handler: fn(usize, i64, i32),
            release: fn(usize),
            ctx: usize,
        );
        fn setUnavailableCounterHandler(
            self: Pin<&mut ContextWrapper>,
            handler: fn(usize, i64, i32),
            release: fn(usize),
            ctx: usize,
        );
        fn setCloseClientHandler(
            self: Pin<&mut ContextWrapper>,
            handler: fn(usize),
            release: fn(usize),
            ctx: usize,
        );
        fn setErrorFrameHandler(
            self: Pin<&mut ContextWrapper>,
            handler: fn(usize, i64, i32, i32, i64, u16, i16, &[u8]),
            release: fn(usize),
            ctx: usize,
        );
        fn create_aeron(context: UniquePtr<ContextWrapper>) -> Result<UniquePtr<AeronWrapper>>;

        fn isClosed(self: &AeronWrapper) -> bool;
        fn addPublication(self: &AeronWrapper, channel: &str, stream_id: i32) -> Result<i64>;
        fn addExclusivePublication(
            self: &AeronWrapper,
            channel: &str,
            stream_id: i32,
        ) -> Result<i64>;
        fn addSubscription(self: &AeronWrapper, channel: &str, stream_id: i32) -> Result<i64>;
        fn findPublication(
            self: &AeronWrapper,
            registration_id: i64,
        ) -> Result<UniquePtr<PublicationWrapper>>;
        fn findExclusivePublication(
            self: &AeronWrapper,
            registration_id: i64,
        ) -> Result<UniquePtr<ExclusivePublicationWrapper>>;
        fn findSubscription(
            self: &AeronWrapper,
            registration_id: i64,
        ) -> Result<UniquePtr<SubscriptionWrapper>>;
        fn addCounter(self: &AeronWrapper, type_id: i32, key: &[u8], label: &str) -> Result<i64>;
        fn addStaticCounter(
            self: &AeronWrapper,
            type_id: i32,
            key: &[u8],
            label: &str,
            registration_id: i64,
        ) -> Result<i64>;
        fn findCounter(
            self: &AeronWrapper,
            registration_id: i64,
        ) -> Result<UniquePtr<CounterWrapper>>;
        fn clientId(self: &AeronWrapper) -> i64;
        fn nextCorrelationId(self: &AeronWrapper) -> i64;
        fn aeronDir(self: &AeronWrapper) -> String;
        fn cncFileName(self: &AeronWrapper) -> Result<String>;
        fn driverTimeoutMs(self: &AeronWrapper) -> i64;
        fn clientName(self: &AeronWrapper) -> String;
        fn idleSleepDurationMs(self: &AeronWrapper) -> i64;
        fn usesAgentInvoker(self: &AeronWrapper) -> bool;
        fn invokeConductor(self: &AeronWrapper) -> Result<i32>;
        #[allow(clippy::too_many_arguments)]
        fn addSubscriptionWithImageHandlers(
            self: &AeronWrapper,
            channel: &str,
            stream_id: i32,
            on_available: fn(usize, &ImageInfo),
            release_available: fn(usize),
            available_ctx: usize,
            on_unavailable: fn(usize, &ImageInfo),
            release_unavailable: fn(usize),
            unavailable_ctx: usize,
        ) -> Result<i64>;
        fn addAvailableCounterHandler(
            self: &AeronWrapper,
            handler: fn(usize, i64, i32),
            release: fn(usize),
            ctx: usize,
        ) -> Result<i64>;
        fn removeAvailableCounterHandler(self: &AeronWrapper, registration_id: i64) -> Result<()>;
        fn addUnavailableCounterHandler(
            self: &AeronWrapper,
            handler: fn(usize, i64, i32),
            release: fn(usize),
            ctx: usize,
        ) -> Result<i64>;
        fn removeUnavailableCounterHandler(self: &AeronWrapper, registration_id: i64)
        -> Result<()>;
        fn addCloseClientHandler(
            self: &AeronWrapper,
            handler: fn(usize),
            release: fn(usize),
            ctx: usize,
        ) -> Result<i64>;
        fn removeCloseClientHandler(self: &AeronWrapper, registration_id: i64) -> Result<()>;
        fn countersReader(self: &AeronWrapper) -> UniquePtr<CountersReaderWrapper>;

        /// # Safety
        ///
        /// `data` must point to `length` readable bytes.
        unsafe fn offerRaw(self: &PublicationWrapper, data: *const u8, length: usize) -> i64;
        fn raiseOfferError(self: &PublicationWrapper) -> Result<()>;
        fn offerParts(
            self: &PublicationWrapper,
            parts: &[OfferPart],
            supplier: fn(usize, &[u8]) -> i64,
            ctx: usize,
            use_supplier: bool,
        ) -> Result<i64>;
        fn tryClaim(
            self: &PublicationWrapper,
            length: usize,
            frame: &mut ClaimFrame,
        ) -> Result<i64>;
        fn channel(self: &PublicationWrapper) -> String;
        fn streamId(self: &PublicationWrapper) -> i32;
        fn sessionId(self: &PublicationWrapper) -> i32;
        fn initialTermId(self: &PublicationWrapper) -> i32;
        fn originalRegistrationId(self: &PublicationWrapper) -> i64;
        fn registrationId(self: &PublicationWrapper) -> i64;
        fn maxMessageLength(self: &PublicationWrapper) -> i32;
        fn maxPayloadLength(self: &PublicationWrapper) -> i32;
        fn termBufferLength(self: &PublicationWrapper) -> i32;
        fn positionBitsToShift(self: &PublicationWrapper) -> i32;
        fn isConnected(self: &PublicationWrapper) -> bool;
        fn isClosed(self: &PublicationWrapper) -> bool;
        fn maxPossiblePosition(self: &PublicationWrapper) -> i64;
        fn position(self: &PublicationWrapper) -> Result<i64>;
        fn publicationLimit(self: &PublicationWrapper) -> Result<i64>;
        fn publicationLimitId(self: &PublicationWrapper) -> i32;
        fn availableWindow(self: &PublicationWrapper) -> Result<i64>;
        fn channelStatusId(self: &PublicationWrapper) -> i32;
        fn channelStatus(self: &PublicationWrapper) -> Result<i64>;
        fn localSocketAddresses(self: &PublicationWrapper) -> Result<Vec<String>>;
        fn addDestination(self: &PublicationWrapper, endpoint: &str) -> Result<i64>;
        fn removeDestination(self: &PublicationWrapper, endpoint: &str) -> Result<i64>;
        fn removeDestinationById(self: &PublicationWrapper, registration_id: i64) -> Result<i64>;
        fn findDestinationResponse(self: &PublicationWrapper, correlation_id: i64) -> Result<bool>;
        fn isOriginal(self: &PublicationWrapper) -> bool;
        /// # Safety
        ///
        /// `data` must point to `length` readable bytes.
        unsafe fn offerRaw(
            self: &ExclusivePublicationWrapper,
            data: *const u8,
            length: usize,
        ) -> i64;
        fn raiseOfferError(self: &ExclusivePublicationWrapper) -> Result<()>;
        /// # Safety
        ///
        /// `data` must point to `length` readable bytes holding valid frames.
        unsafe fn offerBlockRaw(
            self: &ExclusivePublicationWrapper,
            data: *const u8,
            length: usize,
        ) -> i64;
        fn appendPaddingRaw(self: &ExclusivePublicationWrapper, length: usize) -> i64;
        fn offerParts(
            self: &ExclusivePublicationWrapper,
            parts: &[OfferPart],
            supplier: fn(usize, &[u8]) -> i64,
            ctx: usize,
            use_supplier: bool,
        ) -> Result<i64>;
        fn tryClaim(
            self: &ExclusivePublicationWrapper,
            length: usize,
            frame: &mut ClaimFrame,
        ) -> Result<i64>;
        fn channel(self: &ExclusivePublicationWrapper) -> String;
        fn streamId(self: &ExclusivePublicationWrapper) -> i32;
        fn sessionId(self: &ExclusivePublicationWrapper) -> i32;
        fn initialTermId(self: &ExclusivePublicationWrapper) -> i32;
        fn originalRegistrationId(self: &ExclusivePublicationWrapper) -> i64;
        fn registrationId(self: &ExclusivePublicationWrapper) -> i64;
        fn maxMessageLength(self: &ExclusivePublicationWrapper) -> i32;
        fn maxPayloadLength(self: &ExclusivePublicationWrapper) -> i32;
        fn termBufferLength(self: &ExclusivePublicationWrapper) -> i32;
        fn positionBitsToShift(self: &ExclusivePublicationWrapper) -> i32;
        fn isConnected(self: &ExclusivePublicationWrapper) -> bool;
        fn isClosed(self: &ExclusivePublicationWrapper) -> bool;
        fn maxPossiblePosition(self: &ExclusivePublicationWrapper) -> i64;
        fn position(self: &ExclusivePublicationWrapper) -> Result<i64>;
        fn publicationLimit(self: &ExclusivePublicationWrapper) -> Result<i64>;
        fn publicationLimitId(self: &ExclusivePublicationWrapper) -> i32;
        fn availableWindow(self: &ExclusivePublicationWrapper) -> Result<i64>;
        fn channelStatusId(self: &ExclusivePublicationWrapper) -> i32;
        fn channelStatus(self: &ExclusivePublicationWrapper) -> Result<i64>;
        fn localSocketAddresses(self: &ExclusivePublicationWrapper) -> Result<Vec<String>>;
        fn addDestination(self: &ExclusivePublicationWrapper, endpoint: &str) -> Result<i64>;
        fn removeDestination(self: &ExclusivePublicationWrapper, endpoint: &str) -> Result<i64>;
        fn removeDestinationById(
            self: &ExclusivePublicationWrapper,
            registration_id: i64,
        ) -> Result<i64>;
        fn findDestinationResponse(
            self: &ExclusivePublicationWrapper,
            correlation_id: i64,
        ) -> Result<bool>;
        fn revoke(self: &ExclusivePublicationWrapper) -> Result<()>;
        fn revokeOnClose(self: &ExclusivePublicationWrapper);

        fn poll(
            self: Pin<&mut SubscriptionWrapper>,
            fragment_limit: i32,
            handler: fn(usize, &[u8], &Header),
            ctx: usize,
        ) -> Result<i32>;
        fn controlledPollAssembled(
            self: Pin<&mut SubscriptionWrapper>,
            fragment_limit: i32,
            handler: fn(usize, &[u8], &Header) -> i32,
            ctx: usize,
        ) -> Result<i32>;
        fn isConnected(self: &SubscriptionWrapper) -> bool;
        fn deleteSessionBuffer(self: Pin<&mut SubscriptionWrapper>, session_id: i32) -> bool;
        fn controlledPoll(
            self: Pin<&mut SubscriptionWrapper>,
            fragment_limit: i32,
            handler: fn(usize, &[u8], &Header) -> i32,
            ctx: usize,
        ) -> Result<i32>;
        fn blockPoll(
            self: Pin<&mut SubscriptionWrapper>,
            block_length_limit: i32,
            handler: fn(usize, &[u8], i32, i32),
            ctx: usize,
        ) -> Result<i64>;
        fn channel(self: &SubscriptionWrapper) -> String;
        fn streamId(self: &SubscriptionWrapper) -> i32;
        fn registrationId(self: &SubscriptionWrapper) -> i64;
        fn channelStatusId(self: &SubscriptionWrapper) -> i32;
        fn channelStatus(self: &SubscriptionWrapper) -> Result<i64>;
        fn isClosed(self: &SubscriptionWrapper) -> bool;
        fn localSocketAddresses(self: &SubscriptionWrapper) -> Result<Vec<String>>;
        fn resolvedEndpoint(self: &SubscriptionWrapper) -> Result<String>;
        fn tryResolveChannelEndpointPort(self: &SubscriptionWrapper) -> Result<String>;
        fn copyOfImageList(self: &SubscriptionWrapper) -> UniquePtr<ImageListWrapper>;

        type ImageListWrapper;
        fn count(self: &ImageListWrapper) -> usize;
        fn get(self: &ImageListWrapper, index: usize) -> UniquePtr<ImageWrapper>;
        fn addDestination(self: &SubscriptionWrapper, endpoint: &str) -> Result<i64>;
        fn removeDestination(self: &SubscriptionWrapper, endpoint: &str) -> Result<i64>;
        fn findDestinationResponse(self: &SubscriptionWrapper, correlation_id: i64)
        -> Result<bool>;
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
            handler: fn(usize, &[u8], &Header),
            ctx: usize,
        ) -> Result<i32>;
        fn controlledPollAssembled(
            self: Pin<&mut ImageWrapper>,
            fragment_limit: i32,
            handler: fn(usize, &[u8], &Header) -> i32,
            ctx: usize,
        ) -> Result<i32>;
        fn controlledPoll(
            self: Pin<&mut ImageWrapper>,
            fragment_limit: i32,
            handler: fn(usize, &[u8], &Header) -> i32,
            ctx: usize,
        ) -> Result<i32>;
        fn boundedPoll(
            self: Pin<&mut ImageWrapper>,
            limit_position: i64,
            fragment_limit: i32,
            handler: fn(usize, &[u8], &Header),
            ctx: usize,
        ) -> Result<i32>;
        fn boundedControlledPoll(
            self: Pin<&mut ImageWrapper>,
            limit_position: i64,
            fragment_limit: i32,
            handler: fn(usize, &[u8], &Header) -> i32,
            ctx: usize,
        ) -> Result<i32>;
        fn boundedControlledPollAssembled(
            self: Pin<&mut ImageWrapper>,
            limit_position: i64,
            fragment_limit: i32,
            handler: fn(usize, &[u8], &Header) -> i32,
            ctx: usize,
        ) -> Result<i32>;
        fn blockPoll(
            self: Pin<&mut ImageWrapper>,
            block_length_limit: i32,
            handler: fn(usize, &[u8], i32, i32),
            ctx: usize,
        ) -> Result<i32>;
        fn initialTermId(self: &ImageWrapper) -> i32;
        fn termBufferLength(self: &ImageWrapper) -> i32;
        fn positionBitsToShift(self: &ImageWrapper) -> i32;
        fn subscriberPositionId(self: &ImageWrapper) -> i32;
        fn subscriptionRegistrationId(self: &ImageWrapper) -> i64;
        fn isPublicationRevoked(self: &ImageWrapper) -> bool;
        fn activeTransportCount(self: &ImageWrapper) -> Result<i32>;
        fn reject(self: &ImageWrapper, reason: &str) -> Result<()>;

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
        fn findByRegistrationId(self: &CountersReaderWrapper, registration_id: i64) -> Result<i32>;
        fn findByTypeIdAndRegistrationId(
            self: &CountersReaderWrapper,
            type_id: i32,
            registration_id: i64,
        ) -> Result<i32>;
        fn getCounterRegistrationId(self: &CountersReaderWrapper, id: i32) -> Result<i64>;
        fn getCounterOwnerId(self: &CountersReaderWrapper, id: i32) -> Result<i64>;
        fn getFreeForReuseDeadline(self: &CountersReaderWrapper, id: i32) -> Result<i64>;
        fn getCounterKey(self: &CountersReaderWrapper, id: i32) -> Result<Vec<u8>>;
        fn findHeartbeatCounterId(
            self: &CountersReaderWrapper,
            type_id: i32,
            registration_id: i64,
        ) -> Result<i32>;
        fn isHeartbeatActive(
            self: &CountersReaderWrapper,
            counter_id: i32,
            type_id: i32,
            registration_id: i64,
        ) -> Result<bool>;

        fn mapCncFile(directory: &str, timeout_ms: i64) -> Result<UniquePtr<CncFileWrapper>>;
        fn countersReader(self: &CncFileWrapper) -> UniquePtr<CountersReaderWrapper>;
        fn toDriverHeartbeat(self: &CncFileWrapper) -> i64;
        fn constants(self: &CncFileWrapper) -> Result<CncConstants>;
        fn fileName(self: &CncFileWrapper) -> String;
        fn readErrorLog(
            self: &CncFileWrapper,
            handler: fn(usize, i32, i64, i64, &[u8]),
            ctx: usize,
            since_timestamp: i64,
        ) -> Result<i32>;
        fn readLossReport(
            self: &CncFileWrapper,
            handler: fn(usize, i64, i64, i64, i64, i32, i32, &[u8], &[u8]),
            ctx: usize,
        ) -> Result<i32>;

        fn counter(
            self: &CountersReaderWrapper,
            registration_id: i64,
            counter_id: i32,
            checked: bool,
        ) -> Result<UniquePtr<CounterWrapper>>;
        fn counterView(
            self: &CountersReaderWrapper,
            counter_id: i32,
        ) -> Result<UniquePtr<CounterWrapper>>;
        fn isValid(self: &CounterWrapper) -> bool;

        fn id(self: &CounterWrapper) -> i32;
        fn registrationId(self: &CounterWrapper) -> i64;
        fn state(self: &CounterWrapper) -> i32;
        fn label(self: &CounterWrapper) -> String;
        fn isClosed(self: &CounterWrapper) -> bool;
        fn get(self: &CounterWrapper) -> i64;
        fn getWeak(self: &CounterWrapper) -> i64;
        fn set(self: &CounterWrapper, value: i64);
        fn setOrdered(self: &CounterWrapper, value: i64);
        fn setWeak(self: &CounterWrapper, value: i64);
        fn increment(self: &CounterWrapper);
        fn incrementOrdered(self: &CounterWrapper);
        fn getAndAdd(self: &CounterWrapper, value: i64) -> i64;
        fn getAndAddOrdered(self: &CounterWrapper, value: i64) -> i64;
        fn getAndSet(self: &CounterWrapper, value: i64) -> i64;
        fn compareAndSet(self: &CounterWrapper, expected: i64, update: i64) -> bool;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(feature = "driver")]
    #[test]
    fn driver_builder_reports_first_error() {
        let err = MediaDriver::builder()
            .mtu_length(7)
            .threading_mode(ThreadingMode::Shared)
            .start()
            .expect_err("invalid MTU");
        assert_eq!(err.kind(), ErrorKind::IllegalArgument, "{err}");
    }

    #[test]
    fn thread_safety_markers() {
        fn send_sync<T: Send + Sync>() {}
        fn send<T: Send>() {}
        send_sync::<AeronClient>();
        send_sync::<Publication>();
        send_sync::<CountersReader>();
        send_sync::<Counter>();
        send_sync::<CounterView>();
        send_sync::<CncFile>();
        #[cfg(feature = "driver")]
        send_sync::<MediaDriver>();
        send::<ExclusivePublication>();
        send::<Subscription>();
        #[cfg(feature = "archive")]
        {
            send_sync::<archive::AeronArchive>();
            send::<archive::AsyncConnect>();
            send::<archive::Context>();
            send::<archive::ReplayMerge<'static>>();
            send::<archive::PersistentSubscription>();
            send::<archive::PersistentSubscriptionBuilder>();
        }
    }

    #[test]
    fn public_types_are_debug() {
        fn debug<T: std::fmt::Debug>() {}
        debug::<AeronClient>();
        debug::<PendingAdd<'static, Publication>>();
        debug::<Context>();
        debug::<ChannelBuilder>();
        debug::<Publication>();
        debug::<ExclusivePublication>();
        debug::<BufferClaim<'static>>();
        debug::<Subscription>();
        debug::<Image<'static>>();
        debug::<Header>();
        debug::<Counter>();
        debug::<CounterView>();
        debug::<CountersReader>();
        debug::<CncFile>();
        #[cfg(feature = "driver")]
        debug::<MediaDriver>();
        #[cfg(feature = "driver")]
        debug::<MediaDriverBuilder>();
        debug::<Error>();
        debug::<OfferError>();
        #[cfg(feature = "archive")]
        {
            debug::<archive::AeronArchive>();
            debug::<archive::ReplayMerge<'static>>();
            debug::<archive::AsyncConnect>();
            debug::<archive::Context>();
            debug::<archive::ContextInfo>();
            debug::<archive::PersistentSubscription>();
            debug::<archive::PersistentSubscriptionBuilder>();
            debug::<archive::ReplayParams>();
            debug::<archive::ReplicationParams>();
            debug::<archive::RecordingSignal>();
        }
    }
}
