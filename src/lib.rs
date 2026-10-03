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
//! sub1.poll(10, |data, _| {
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
mod channel;
mod client;
mod context;
mod counters;
mod driver;
mod driver_gen;
mod error;
mod handlers;
mod header;
mod image;
mod publication;
mod subscription;
use callback::Callback;
pub use channel::ChannelBuilder;
pub use client::{AeronClient, PendingAdd};
pub use context::Context;
pub use counters::CountersReader;
pub use driver::{IdleStrategy, MediaDriver, MediaDriverBuilder, ThreadingMode};
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
        type MediaDriverWrapper;
        type CountersReaderWrapper;

        fn create_context() -> Result<UniquePtr<ContextWrapper>>;
        fn requestDriverTermination(directory: &str, token: &[u8]) -> Result<bool>;
        fn defaultAeronPath() -> Result<String>;
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
        fn create_media_driver() -> Result<UniquePtr<MediaDriverWrapper>>;

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

        fn start(self: Pin<&mut MediaDriverWrapper>) -> Result<()>;

        fn setThreadingMode(self: Pin<&mut MediaDriverWrapper>, mode: i32) -> Result<()>;

        fn offer(self: &PublicationWrapper, buffer: &[u8]) -> Result<i64>;
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
        fn offer(self: &ExclusivePublicationWrapper, buffer: &[u8]) -> Result<i64>;
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
