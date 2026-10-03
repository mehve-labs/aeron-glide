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
mod channel;
mod client;
mod context;
mod counters;
mod driver;
mod driver_gen;
mod error;
mod image;
mod publication;
mod subscription;
use callback::Callback;
pub use channel::ChannelBuilder;
pub use client::AeronClient;
pub use context::Context;
pub use counters::CountersReader;
pub use driver::{IdleStrategy, MediaDriver, MediaDriverBuilder, ThreadingMode};
pub use driver_gen::{InferableBoolean, ThreadNaming};
pub use error::{Error, ErrorKind, OfferError, Result};
pub use image::Image;
pub use publication::{ChannelStatus, ExclusivePublication, Publication};
use std::marker::PhantomData;
pub use subscription::{ControlledAction, PollAction, Subscription};

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
        fn isOriginal(self: &PublicationWrapper) -> bool;
        fn offer(self: &ExclusivePublicationWrapper, buffer: &[u8]) -> Result<i64>;
        fn tryClaim(
            self: &ExclusivePublicationWrapper,
            length: usize,
            handler: fn(usize, &mut [u8]) -> bool,
            ctx: usize,
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
