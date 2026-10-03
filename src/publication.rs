//! Publications ([`Publication`], [`ExclusivePublication`]).

use super::*;

/// Accessors shared by [`Publication`] and [`ExclusivePublication`] (C++ `Publication` /
/// `ExclusivePublication`).
macro_rules! publication_accessors {
    () => {
        /// The channel URI this publication was added with.
        pub fn channel(&self) -> String {
            self.inner.channel()
        }

        /// The stream ID this publication was added with.
        pub fn stream_id(&self) -> i32 {
            self.inner.streamId()
        }

        /// The session ID assigned by the media driver for this publication.
        pub fn session_id(&self) -> i32 {
            self.inner.sessionId()
        }

        /// The initial term ID assigned to this publication's log.
        pub fn initial_term_id(&self) -> i32 {
            self.inner.initialTermId()
        }

        /// The registration ID of the original publication this one attached to
        /// (equal to [`registration_id`](Self::registration_id) if it is the original).
        pub fn original_registration_id(&self) -> i64 {
            self.inner.originalRegistrationId()
        }

        /// The registration ID of this publication with the media driver.
        pub fn registration_id(&self) -> i64 {
            self.inner.registrationId()
        }

        /// The maximum length of a message, which may span several fragments.
        pub fn max_message_length(&self) -> usize {
            self.inner.maxMessageLength() as usize
        }

        /// The maximum payload of a single fragment (MTU minus the frame header).
        pub fn max_payload_length(&self) -> usize {
            self.inner.maxPayloadLength() as usize
        }

        /// The length of each term in the publication's log buffer.
        pub fn term_buffer_length(&self) -> usize {
            self.inner.termBufferLength() as usize
        }

        /// The number of bits to shift a term ID by to get a stream position.
        pub fn position_bits_to_shift(&self) -> i32 {
            self.inner.positionBitsToShift()
        }

        /// Returns `true` if at least one subscriber is connected to this publication.
        pub fn is_connected(&self) -> bool {
            self.inner.isConnected()
        }

        /// Returns `true` once the publication has been closed.
        pub fn is_closed(&self) -> bool {
            self.inner.isClosed()
        }

        /// The maximum stream position this publication can reach.
        pub fn max_possible_position(&self) -> i64 {
            self.inner.maxPossiblePosition()
        }

        /// The current position the publication has written to.
        pub fn position(&self) -> Result<i64> {
            Ok(self.inner.position()?)
        }

        /// The position this publication can be written up to before back pressure
        /// applies (flow control limit).
        pub fn publication_limit(&self) -> Result<i64> {
            Ok(self.inner.publicationLimit()?)
        }

        /// The counter ID of the publication limit, for reading it from a
        /// [`CountersReader`](crate::CountersReader).
        pub fn publication_limit_id(&self) -> i32 {
            self.inner.publicationLimitId()
        }

        /// How many bytes can be written before reaching the publication limit.
        pub fn available_window(&self) -> Result<i64> {
            Ok(self.inner.availableWindow()?)
        }

        /// The counter ID of the channel status, for reading it from a
        /// [`CountersReader`](crate::CountersReader).
        pub fn channel_status_id(&self) -> i32 {
            self.inner.channelStatusId()
        }

        /// The status of the publication's channel endpoint.
        pub fn channel_status(&self) -> Result<ChannelStatus> {
            Ok(ChannelStatus::from_c(self.inner.channelStatus()?))
        }

        /// The local socket address(es) the channel is bound to, e.g. to find a
        /// port the driver chose for `endpoint=host:0`. Empty unless the channel
        /// status is [`ChannelStatus::Active`].
        pub fn local_socket_addresses(&self) -> Result<Vec<String>> {
            Ok(self.inner.localSocketAddresses()?)
        }
    };
}

/// The status of a channel endpoint (`ChannelEndpointStatus`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum ChannelStatus {
    /// The endpoint is being set up.
    Initializing,
    /// The endpoint is active.
    Active,
    /// The endpoint is closing.
    Closing,
    /// The endpoint failed, e.g. it could not bind its address.
    Errored,
    /// A status value this version of aeron-glide does not know.
    Other(i64),
}

impl ChannelStatus {
    pub(crate) fn from_c(value: i64) -> Self {
        match value {
            0 => Self::Initializing,
            1 => Self::Active,
            2 => Self::Closing,
            -1 => Self::Errored,
            other => Self::Other(other),
        }
    }
}

/// A concurrent publication for sending messages on a channel+stream.
///
/// `Send + Sync`: several threads may `offer` / `try_claim` on the same
/// publication concurrently (e.g. through an `Arc<Publication>`).
pub struct Publication {
    pub(crate) inner: cxx::UniquePtr<ffi::PublicationWrapper>,
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

    /// Returns `true` if this publication is the original one for its channel and
    /// stream, i.e. it did not attach to a publication another call already added.
    pub fn is_original(&self) -> bool {
        self.inner.isOriginal()
    }

    publication_accessors!();
}

/// An exclusive publication — single-writer, lower overhead than [`Publication`].
///
/// `Send` but not `Sync`: it can move to another thread, but only one thread may
/// use it at a time.
pub struct ExclusivePublication {
    pub(crate) inner: cxx::UniquePtr<ffi::ExclusivePublicationWrapper>,
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
        error::offer_result(self.inner.offer(buffer)?)
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
        let result = self.inner.tryClaim(length, callback::claim::<F>, cb.ctx());
        error::offer_result(cb.finish(result)?)
    }

    publication_accessors!();
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
