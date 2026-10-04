//! Lifecycle handlers: events reported by the client conductor
//! ([`Context`](crate::Context) handlers and the `AeronClient::add_*_handler`s).
//!
//! A handler is moved into an `Arc` owned by the C++ client and released when the
//! last C++ copy of it is destroyed. Each call takes its own reference, so a
//! handler that drops the client cannot free itself mid-call; panics are caught
//! (they cannot cross the conductor thread) and printed to stderr.

use crate::callback::ConductorCallbackScope;
use crate::ffi;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::panic::{self, AssertUnwindSafe};
use std::sync::Arc;

/// An image becoming available or unavailable (C++ `on_available_image_t` /
/// `on_unavailable_image_t`). A snapshot: poll images through their
/// [`Subscription`](crate::Subscription), not from the handler.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct ImageEvent {
    /// The publisher's session ID.
    pub session_id: i32,
    /// The correlation ID the driver assigned to the image.
    pub correlation_id: i64,
    /// The registration ID of the subscription the image belongs to.
    pub subscription_registration_id: i64,
    /// The stream position at which the image joined.
    pub join_position: i64,
    /// The subscriber position: where an unavailable image ended.
    pub position: i64,
    /// The initial term ID of the stream.
    pub initial_term_id: i32,
    /// The term buffer length of the stream.
    pub term_buffer_length: usize,
    /// The number of bits to shift a term ID by to get a stream position.
    pub position_bits_to_shift: i32,
    /// The source of the image, e.g. `"192.168.1.1:40123"` or `"aeron:ipc"`.
    pub source_identity: String,
}

/// A publication added by this client (C++ `on_new_publication_t`).
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct NewPublication {
    /// The channel the publication was added with.
    pub channel: String,
    /// The stream ID.
    pub stream_id: i32,
    /// The session ID the driver assigned.
    pub session_id: i32,
    /// The registration (correlation) ID of the add.
    pub correlation_id: i64,
}

/// A subscription added by this client (C++ `on_new_subscription_t`).
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct NewSubscription {
    /// The channel the subscription was added with.
    pub channel: String,
    /// The stream ID.
    pub stream_id: i32,
    /// The registration (correlation) ID of the add.
    pub correlation_id: i64,
}

/// A counter becoming available or unavailable (C++ `on_available_counter_t` /
/// `on_unavailable_counter_t`). Read it through a
/// [`CountersReader`](crate::CountersReader).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct CounterEvent {
    /// The registration ID of the counter.
    pub registration_id: i64,
    /// The counter ID.
    pub counter_id: i32,
}

/// An error frame sent to one of this client's publications, e.g. when a
/// subscriber rejects its image (C++ `status::PublicationErrorFrame`).
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct PublicationErrorFrame {
    /// The registration ID of the publication.
    pub registration_id: i64,
    /// The registration ID of the destination the error came through, for a
    /// multi-destination publication (or `NULL_VALUE`, -1).
    pub destination_registration_id: i64,
    /// The receiver that sent the error frame.
    pub receiver_id: i64,
    /// The error code the receiver reported, e.g. from `Image::reject`.
    pub error_code: i32,
    /// The receiver's message, e.g. the reason passed to `Image::reject`
    /// (invalid UTF-8 replaced with `U+FFFD`).
    pub error_message: String,
    /// The session ID of the publication.
    pub session_id: i32,
    /// The stream ID of the publication.
    pub stream_id: i32,
    /// The receiver group tag, if any.
    pub group_tag: i64,
    /// The address the error frame came from, if known.
    pub source: Option<SocketAddr>,
}

/// Hand a handler to C++: returns the context to pass with `release::<H>`.
pub(crate) fn into_ctx<H: Send + Sync + 'static>(handler: H) -> usize {
    Arc::into_raw(Arc::new(handler)).expose_provenance()
}

/// Free a handler handed to C++ with [`into_ctx`]. Called once, by C++, possibly
/// on the conductor thread (e.g. when a subscription with its own image handlers
/// closes), so drops of clients or resources the handler owns are moved off it,
/// and a panicking `Drop` is contained.
pub(crate) fn release<H>(ctx: usize) {
    // SAFETY: `ctx` comes from `into_ctx::<H>` and C++ releases it exactly once.
    let handler = unsafe { Arc::from_raw(std::ptr::with_exposed_provenance::<H>(ctx)) };
    let _scope = ConductorCallbackScope::enter();
    if panic::catch_unwind(AssertUnwindSafe(move || drop(handler))).is_err() {
        eprintln!("aeron-glide: dropping a client handler panicked");
    }
}

/// Call the handler behind `ctx`, catching panics.
pub(crate) fn invoke<H>(ctx: usize, what: &str, call: impl FnOnce(&H)) {
    let ptr = std::ptr::with_exposed_provenance::<H>(ctx);
    // SAFETY: C++ holds the reference created by `into_ctx` until `release`; taking
    // our own keeps the handler alive even if this call drops the client.
    let handler = unsafe {
        Arc::increment_strong_count(ptr);
        Arc::from_raw(ptr)
    };
    let _scope = ConductorCallbackScope::enter();
    if panic::catch_unwind(AssertUnwindSafe(|| call(&handler))).is_err() {
        eprintln!("aeron-glide: the {what} handler panicked");
    }
}

pub(crate) fn image_event<F: Fn(&ImageEvent) + Send + Sync + 'static>(
    ctx: usize,
    info: &ffi::ImageInfo,
) {
    let event = ImageEvent {
        session_id: info.session_id,
        correlation_id: info.correlation_id,
        subscription_registration_id: info.subscription_registration_id,
        join_position: info.join_position,
        position: info.position,
        initial_term_id: info.initial_term_id,
        term_buffer_length: info.term_buffer_length as usize,
        position_bits_to_shift: info.position_bits_to_shift,
        source_identity: info.source_identity.clone(),
    };
    invoke::<F>(ctx, "image", |f| f(&event));
}

pub(crate) fn new_publication<F: Fn(&NewPublication) + Send + Sync + 'static>(
    ctx: usize,
    channel: &[u8],
    stream_id: i32,
    session_id: i32,
    correlation_id: i64,
) {
    let event = NewPublication {
        channel: String::from_utf8_lossy(channel).into_owned(),
        stream_id,
        session_id,
        correlation_id,
    };
    invoke::<F>(ctx, "new publication", |f| f(&event));
}

pub(crate) fn new_subscription<F: Fn(&NewSubscription) + Send + Sync + 'static>(
    ctx: usize,
    channel: &[u8],
    stream_id: i32,
    correlation_id: i64,
) {
    let event = NewSubscription {
        channel: String::from_utf8_lossy(channel).into_owned(),
        stream_id,
        correlation_id,
    };
    invoke::<F>(ctx, "new subscription", |f| f(&event));
}

pub(crate) fn counter_event<F: Fn(CounterEvent) + Send + Sync + 'static>(
    ctx: usize,
    registration_id: i64,
    counter_id: i32,
) {
    let event = CounterEvent {
        registration_id,
        counter_id,
    };
    invoke::<F>(ctx, "counter", |f| f(event));
}

pub(crate) fn close_client<F: Fn() + Send + Sync + 'static>(ctx: usize) {
    invoke::<F>(ctx, "close client", |f| f());
}

/// `AERON_RESPONSE_ADDRESS_TYPE_IPV4` / `_IPV6`.
const ADDRESS_TYPE_IPV4: i16 = 1;
const ADDRESS_TYPE_IPV6: i16 = 2;

pub(crate) fn error_frame<F: Fn(&PublicationErrorFrame) + Send + Sync + 'static>(
    ctx: usize,
    info: &crate::ffi::ErrorFrameInfo,
    address: &[u8],
    message: &[u8],
) {
    let ip = match info.address_type {
        ADDRESS_TYPE_IPV4 if address.len() >= 4 => Some(IpAddr::V4(Ipv4Addr::new(
            address[0], address[1], address[2], address[3],
        ))),
        ADDRESS_TYPE_IPV6 if address.len() >= 16 => {
            let mut octets = [0u8; 16];
            octets.copy_from_slice(&address[..16]);
            Some(IpAddr::V6(Ipv6Addr::from(octets)))
        }
        _ => None,
    };
    let event = PublicationErrorFrame {
        registration_id: info.registration_id,
        destination_registration_id: info.destination_registration_id,
        receiver_id: info.receiver_id,
        error_code: info.error_code,
        error_message: String::from_utf8_lossy(message).into_owned(),
        session_id: info.session_id,
        stream_id: info.stream_id,
        group_tag: info.group_tag,
        source: ip.map(|ip| SocketAddr::new(ip, info.source_port)),
    };
    invoke::<F>(ctx, "publication error frame", |f| f(&event));
}

/// Client error handler: `encoded` is an exception encoded by the C++ shim.
pub(crate) fn error<F: Fn(&crate::Error) + Send + Sync + 'static>(ctx: usize, encoded: &[u8]) {
    let error = crate::Error::from_encoded(&String::from_utf8_lossy(encoded));
    invoke::<F>(ctx, "client error", |f| f(&error));
}
