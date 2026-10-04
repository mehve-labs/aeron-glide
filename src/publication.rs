//! Publications ([`Publication`], [`ExclusivePublication`]).

use super::*;
use std::marker::PhantomData;

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

        /// The current position the publication has written to. Fails with
        /// [`ErrorKind::IllegalState`](crate::ErrorKind::IllegalState) once the
        /// publication is closed.
        pub fn position(&self) -> Result<i64> {
            self.open_value(self.inner.position()?)
        }

        /// The position this publication can be written up to before back pressure
        /// applies (flow control limit).
        /// Fails once the publication is closed, like [`position`](Self::position).
        pub fn publication_limit(&self) -> Result<i64> {
            self.open_value(self.inner.publicationLimit()?)
        }

        /// The counter ID of the publication limit, for reading it from a
        /// [`CountersReader`](crate::CountersReader).
        pub fn publication_limit_id(&self) -> i32 {
            self.inner.publicationLimitId()
        }

        /// How many bytes can be written before reaching the publication limit
        /// (negative once an offer has gone past it). Fails once the publication
        /// is closed, like [`position`](Self::position).
        pub fn available_window(&self) -> Result<i64> {
            self.open_value(self.inner.availableWindow()?)
        }

        /// Aeron returns `AERON_PUBLICATION_CLOSED` (-4) in place of a value once
        /// the publication is closed.
        fn open_value(&self, value: i64) -> Result<i64> {
            if value == AERON_PUBLICATION_CLOSED && self.inner.isClosed() {
                return Err(
                    Error::new(ErrorKind::IllegalState, "the publication is closed")
                        .with_code(AERON_PUBLICATION_CLOSED as i32),
                );
            }
            Ok(value)
        }

        /// The counter ID of the channel status, for reading it from a
        /// [`CountersReader`](crate::CountersReader).
        pub fn channel_status_id(&self) -> i32 {
            self.inner.channelStatusId()
        }

        /// The status of the publication's channel endpoint;
        /// [`ChannelStatus::NoStatus`] for IPC channels and closed publications.
        pub fn channel_status(&self) -> Result<ChannelStatus> {
            let status = self.inner.channelStatus()?;
            let unavailable = self.channel_status_id() < 0 || self.is_closed();
            Ok(ChannelStatus::from_c(status, unavailable))
        }

        /// Add a destination to a multi-destination-cast publication (channel with
        /// `control-mode=manual`), e.g. `"aeron:udp?endpoint=host:port"`.
        ///
        /// Returns a correlation ID; the destination is in use once
        /// [`find_destination_response`](Self::find_destination_response) returns
        /// `true`. The correlation ID is also the destination's registration ID for
        /// [`remove_destination_by_id`](Self::remove_destination_by_id).
        pub fn add_destination(&self, endpoint_channel: &str) -> Result<i64> {
            Ok(self.inner.addDestination(endpoint_channel)?)
        }

        /// Remove a destination added with [`add_destination`](Self::add_destination).
        /// Returns a correlation ID to pass to
        /// [`find_destination_response`](Self::find_destination_response).
        pub fn remove_destination(&self, endpoint_channel: &str) -> Result<i64> {
            Ok(self.inner.removeDestination(endpoint_channel)?)
        }

        /// Remove a destination by the registration ID that
        /// [`add_destination`](Self::add_destination) returned. Returns a correlation
        /// ID to pass to [`find_destination_response`](Self::find_destination_response).
        pub fn remove_destination_by_id(&self, registration_id: i64) -> Result<i64> {
            Ok(self.inner.removeDestinationById(registration_id)?)
        }

        /// Returns `true` once the media driver has applied the destination change
        /// with this correlation ID, `false` while it is pending; fails if the
        /// driver rejected it or the ID is unknown.
        pub fn find_destination_response(&self, correlation_id: i64) -> Result<bool> {
            Ok(self.inner.findDestinationResponse(correlation_id)?)
        }

        /// The local socket address the channel is bound to, e.g. to find a port
        /// the driver chose. Empty for IPC and while the channel is not active.
        pub fn local_socket_addresses(&self) -> Result<Vec<String>> {
            Ok(self.inner.localSocketAddresses()?)
        }
    };
}

/// The status of a channel endpoint (`ChannelEndpointStatus`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum ChannelStatus {
    /// There is no status: the channel has no status counter (IPC) or the
    /// publication or subscription is closed. (Aeron reports these with the same
    /// value as [`Errored`](Self::Errored).)
    NoStatus,
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
    /// `unavailable`: the channel has no status counter, or its owner is closed.
    pub(crate) fn from_c(value: i64, unavailable: bool) -> Self {
        match value {
            -1 if unavailable => Self::NoStatus,
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

impl std::fmt::Debug for Publication {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Publication")
            .field("channel", &self.channel())
            .field("stream_id", &self.stream_id())
            .field("session_id", &self.session_id())
            .field("registration_id", &self.registration_id())
            .finish_non_exhaustive()
    }
}

impl Drop for Publication {
    fn drop(&mut self) {
        callback::drop_outside_conductor(&mut self.inner);
    }
}

// SAFETY: a concurrent publication is designed for use from multiple threads:
// offer / offerv / try_claim are thread-safe in the C client, the destination
// methods serialise on the C++ wrapper's `m_adminLock`, the other bridged methods
// only read state, and every method bridged as `&self` is a const C++ method.
unsafe impl Send for Publication {}
unsafe impl Sync for Publication {}

impl Publication {
    /// Publish a message. Returns the new stream position on success.
    ///
    /// On failure, [`OfferError::is_retryable`] tells whether retrying can succeed
    /// (not connected, back pressured, admin action).
    pub fn offer(&self, buffer: &[u8]) -> std::result::Result<i64, OfferError> {
        // SAFETY: `buffer` is a live slice of `buffer.len()` bytes.
        let position = unsafe { self.inner.offerRaw(buffer.as_ptr(), buffer.len()) };
        if position == AERON_PUBLICATION_ERROR {
            self.inner.raiseOfferError()?;
        }
        error::offer_result(position)
    }

    /// Publish `parts` as one message, without first copying them into one buffer
    /// (C++ vectored `offer`). Returns the new stream position on success.
    pub fn offer_vectored(&self, parts: &[&[u8]]) -> std::result::Result<i64, OfferError> {
        offer_parts(&*self.inner, parts, None::<fn(&[u8]) -> i64>)
    }

    /// Publish a message, letting `supplier` choose the 64-bit reserved value
    /// written into each fragment's header (e.g. a checksum or timestamp).
    ///
    /// `supplier` is called once per fragment with the frame (header and payload)
    /// before it is published; subscribers read the value from the fragment header.
    ///
    /// If `supplier` panics, the message is still published (with 0 as the
    /// reserved value from then on) and the panic is resumed afterwards.
    pub fn offer_with_reserved_value<F>(
        &self,
        buffer: &[u8],
        supplier: F,
    ) -> std::result::Result<i64, OfferError>
    where
        F: FnMut(&[u8]) -> i64,
    {
        offer_parts(&*self.inner, &[buffer], Some(supplier))
    }

    /// [`offer_vectored`](Self::offer_vectored) with a reserved value supplier, as in
    /// [`offer_with_reserved_value`](Self::offer_with_reserved_value).
    pub fn offer_vectored_with_reserved_value<F>(
        &self,
        parts: &[&[u8]],
        supplier: F,
    ) -> std::result::Result<i64, OfferError>
    where
        F: FnMut(&[u8]) -> i64,
    {
        offer_parts(&*self.inner, parts, Some(supplier))
    }

    /// Zero-copy publish: claim `length` bytes in the log buffer and write the
    /// message directly into them through the returned [`BufferClaim`], then
    /// [`commit`](BufferClaim::commit) it. Dropping the claim without committing
    /// aborts it.
    ///
    /// `length` must not exceed [`max_payload_length`](Self::max_payload_length):
    /// a claim is a single fragment.
    pub fn try_claim(&self, length: usize) -> std::result::Result<BufferClaim<'_>, OfferError> {
        let length = claim_length(length)?;
        let mut frame = ffi::ClaimFrame { ptr: 0, len: 0 };
        let position = error::offer_result(self.inner.tryClaim(length, &mut frame)?)?;
        Ok(BufferClaim::new(frame, position))
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

impl std::fmt::Debug for ExclusivePublication {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ExclusivePublication")
            .field("channel", &self.channel())
            .field("stream_id", &self.stream_id())
            .field("session_id", &self.session_id())
            .field("registration_id", &self.registration_id())
            .finish_non_exhaustive()
    }
}

impl Drop for ExclusivePublication {
    fn drop(&mut self) {
        callback::drop_outside_conductor(&mut self.inner);
    }
}

// SAFETY: an exclusive publication has no thread affinity; it only requires a
// single writer at a time, which `&mut self` on every mutating method enforces.
// It is not `Sync`: its destination methods (`&self`) use an unlocked map.
unsafe impl Send for ExclusivePublication {}

impl ExclusivePublication {
    /// Publish a message. Returns the new stream position on success.
    ///
    /// On failure, [`OfferError::is_retryable`] tells whether retrying can succeed
    /// (not connected, back pressured, admin action).
    pub fn offer(&mut self, buffer: &[u8]) -> std::result::Result<i64, OfferError> {
        // SAFETY: `buffer` is a live slice of `buffer.len()` bytes.
        let position = unsafe { self.inner.offerRaw(buffer.as_ptr(), buffer.len()) };
        if position == AERON_PUBLICATION_ERROR {
            self.inner.raiseOfferError()?;
        }
        error::offer_result(position)
    }

    /// Publish a block of complete data frames, headers included, in one copy
    /// (C `aeron_exclusive_publication_offer_block`; Java `offerBlock`). For
    /// forwarding a stream already in Aeron's format, e.g. blocks read with
    /// [`Subscription::block_poll`](crate::Subscription::block_poll) whose frame
    /// headers are rewritten for this publication.
    ///
    /// The block must start at this publication's current position: its first
    /// frame's term ID and term offset are those of
    /// [`position`](Self::position) (term ID `initial_term_id + (position >>
    /// log2(term_buffer_length))`, term offset `position & (term_buffer_length -
    /// 1)`), and every frame carries this publication's session and stream IDs.
    /// It must fit in the rest of the current term. Returns the new position.
    ///
    /// Fails with [`ErrorKind::IllegalArgument`](crate::ErrorKind::IllegalArgument)
    /// (as [`OfferError::Error`]) unless the whole block is well formed: Aeron
    /// itself only checks the first frame, and malformed frames in the log
    /// would be read by every subscriber.
    pub fn offer_block(&mut self, block: &[u8]) -> std::result::Result<i64, OfferError> {
        validate_block(block, self.session_id(), self.stream_id()).map_err(OfferError::Error)?;
        // SAFETY: `block` is a live slice of `block.len()` bytes of valid frames.
        let position = unsafe { self.inner.offerBlockRaw(block.as_ptr(), block.len()) };
        if position == AERON_PUBLICATION_ERROR {
            self.inner.raiseOfferError()?;
        }
        error::offer_result(position)
    }

    /// Append a padding frame of `length` bytes (C
    /// `aeron_exclusive_publication_append_padding`; Java `appendPadding`):
    /// subscribers skip it, and the stream position advances past it, e.g. to
    /// keep a copied stream's positions aligned with the original. `length` is
    /// at most [`max_message_length`](Self::max_message_length). Returns the
    /// new position.
    pub fn append_padding(&mut self, length: usize) -> std::result::Result<i64, OfferError> {
        let position = self.inner.appendPaddingRaw(length);
        if position == AERON_PUBLICATION_ERROR {
            self.inner.raiseOfferError()?;
        }
        error::offer_result(position)
    }

    /// Publish `parts` as one message, without first copying them into one buffer
    /// (C++ vectored `offer`). Returns the new stream position on success.
    pub fn offer_vectored(&mut self, parts: &[&[u8]]) -> std::result::Result<i64, OfferError> {
        offer_parts(&*self.inner, parts, None::<fn(&[u8]) -> i64>)
    }

    /// Publish a message, letting `supplier` choose the 64-bit reserved value
    /// written into each fragment's header (e.g. a checksum or timestamp).
    ///
    /// `supplier` is called once per fragment with the frame (header and payload)
    /// before it is published; subscribers read the value from the fragment header.
    ///
    /// If `supplier` panics, the message is still published (with 0 as the
    /// reserved value from then on) and the panic is resumed afterwards.
    pub fn offer_with_reserved_value<F>(
        &mut self,
        buffer: &[u8],
        supplier: F,
    ) -> std::result::Result<i64, OfferError>
    where
        F: FnMut(&[u8]) -> i64,
    {
        offer_parts(&*self.inner, &[buffer], Some(supplier))
    }

    /// [`offer_vectored`](Self::offer_vectored) with a reserved value supplier, as in
    /// [`offer_with_reserved_value`](Self::offer_with_reserved_value).
    pub fn offer_vectored_with_reserved_value<F>(
        &mut self,
        parts: &[&[u8]],
        supplier: F,
    ) -> std::result::Result<i64, OfferError>
    where
        F: FnMut(&[u8]) -> i64,
    {
        offer_parts(&*self.inner, parts, Some(supplier))
    }

    /// Zero-copy publish: claim `length` bytes in the log buffer and write the
    /// message directly into them through the returned [`BufferClaim`], then
    /// [`commit`](BufferClaim::commit) it. Dropping the claim without committing
    /// aborts it.
    ///
    /// `length` must not exceed [`max_payload_length`](Self::max_payload_length):
    /// a claim is a single fragment.
    pub fn try_claim(&mut self, length: usize) -> std::result::Result<BufferClaim<'_>, OfferError> {
        let length = claim_length(length)?;
        let mut frame = ffi::ClaimFrame { ptr: 0, len: 0 };
        let position = error::offer_result(self.inner.tryClaim(length, &mut frame)?)?;
        Ok(BufferClaim::new(frame, position))
    }

    /// Always `true`: an exclusive publication never shares its log (C++
    /// `ExclusivePublication::isOriginal`).
    pub fn is_original(&self) -> bool {
        true
    }

    /// Revoke and close the publication now: subscribers see the stream end
    /// immediately, without the usual linger, and their images report
    /// `is_publication_revoked`.
    pub fn revoke(self) -> Result<()> {
        Ok(self.inner.revoke()?)
    }

    /// Revoke the publication when it is closed (dropped) instead of letting it
    /// linger, as [`revoke`](Self::revoke) does immediately.
    pub fn revoke_on_close(&mut self) {
        self.inner.revokeOnClose();
    }

    publication_accessors!();
}

/// A claimed region of a publication's log buffer (C++ `BufferClaim`), returned by
/// `try_claim`.
///
/// Write the message into [`buffer_mut`](Self::buffer_mut), optionally set header
/// fields, then [`commit`](Self::commit). Dropping the claim without committing
/// aborts it, so subscribers skip it. Commit or abort promptly: later messages on
/// the publication wait behind an open claim, and the media driver pads over a
/// claim left open longer than its `publication_unblock_timeout`.
#[must_use = "a claim is aborted when dropped; call `commit` to publish it"]
pub struct BufferClaim<'a> {
    frame: ffi::ClaimFrame,
    position: i64,
    finished: bool,
    _publication: PhantomData<&'a ()>,
}

/// Length of the data frame header that precedes the claimed bytes.
const DATA_HEADER_LENGTH: usize = 32;
/// Frames are aligned to 32 bytes (`AERON_LOGBUFFER_FRAME_ALIGNMENT`).
const FRAME_ALIGNMENT: usize = 32;
/// `AERON_HDR_TYPE_PAD` and `AERON_HDR_TYPE_DATA`.
const HDR_TYPE_PAD: i16 = 0;
const HDR_TYPE_DATA: i16 = 1;

/// Check that `block` is a sequence of well-formed frames for one term of one
/// stream (`aeron_data_header_t`, little-endian): each frame's length is at
/// least a header, the aligned frames exactly fill the block, the frames are
/// data (the first) or padding, and their term offsets follow on from the
/// first one's, with the same term, session and stream IDs throughout.
fn validate_block(block: &[u8], session_id: i32, stream_id: i32) -> Result<()> {
    let invalid = |reason: String| {
        Err(Error::new(
            ErrorKind::IllegalArgument,
            format!("invalid block: {reason}"),
        ))
    };
    let i32_at = |at: usize| i32::from_le_bytes(block[at..at + 4].try_into().unwrap());
    if block.len() < DATA_HEADER_LENGTH || !block.len().is_multiple_of(FRAME_ALIGNMENT) {
        return invalid(format!(
            "length {} is not a positive multiple of {FRAME_ALIGNMENT}",
            block.len()
        ));
    }
    let (first_term_offset, term_id) = (i32_at(8), i32_at(20));
    let mut offset = 0;
    while offset < block.len() {
        let frame_length = i32_at(offset);
        let frame_type = i16::from_le_bytes([block[offset + 6], block[offset + 7]]);
        let aligned = usize::try_from(frame_length)
            .ok()
            .filter(|&length| length >= DATA_HEADER_LENGTH)
            .map(|length| length.next_multiple_of(FRAME_ALIGNMENT));
        let Some(aligned) = aligned.filter(|&aligned| aligned <= block.len() - offset) else {
            return invalid(format!("frame at {offset} has length {frame_length}"));
        };
        let expected_type = if offset == 0 {
            frame_type == HDR_TYPE_DATA
        } else {
            matches!(frame_type, HDR_TYPE_DATA | HDR_TYPE_PAD)
        };
        if !expected_type {
            return invalid(format!("frame at {offset} has type {frame_type}"));
        }
        let expected_term_offset = i64::from(first_term_offset) + offset as i64;
        if i64::from(i32_at(offset + 8)) != expected_term_offset
            || i32_at(offset + 12) != session_id
            || i32_at(offset + 16) != stream_id
            || i32_at(offset + 20) != term_id
        {
            return invalid(format!(
                "frame at {offset} is not at term offset {expected_term_offset} of term {term_id} for session {session_id}, stream {stream_id}"
            ));
        }
        offset += aligned;
    }
    Ok(())
}

/// C `AERON_PUBLICATION_CLOSED`.
const AERON_PUBLICATION_CLOSED: i64 = -4;

/// C `AERON_PUBLICATION_ERROR`: the offer failed with an Aeron error (e.g. a
/// message longer than the maximum message length).
const AERON_PUBLICATION_ERROR: i64 = -6;

impl BufferClaim<'_> {
    fn new(frame: ffi::ClaimFrame, position: i64) -> Self {
        Self {
            frame,
            position,
            finished: false,
            _publication: PhantomData,
        }
    }

    /// The stream position the publication reaches once this claim is committed.
    pub fn position(&self) -> i64 {
        self.position
    }

    /// The claimed bytes, to write the message into.
    pub fn buffer_mut(&mut self) -> &mut [u8] {
        // SAFETY: Aeron reserved `len` bytes at `ptr` (header first) for this claim
        // until it is committed or aborted, and the borrowed publication keeps the
        // log buffer mapped.
        unsafe {
            std::slice::from_raw_parts_mut(
                (self.frame.ptr as *mut u8).add(DATA_HEADER_LENGTH),
                self.len(),
            )
        }
    }

    /// The claimed bytes.
    pub fn buffer(&self) -> &[u8] {
        // SAFETY: as in `buffer_mut`.
        unsafe {
            std::slice::from_raw_parts(
                (self.frame.ptr as *const u8).add(DATA_HEADER_LENGTH),
                self.len(),
            )
        }
    }

    /// The number of claimed bytes.
    pub fn len(&self) -> usize {
        self.frame.len - DATA_HEADER_LENGTH
    }

    /// Returns `true` for a zero-length claim.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// The frame header flags.
    pub fn flags(&self) -> u8 {
        ffi::claimFlags(self.frame)
    }

    /// Set the frame header flags.
    pub fn set_flags(&mut self, flags: u8) -> &mut Self {
        ffi::claimSetFlags(self.frame, flags);
        self
    }

    /// The frame header type.
    pub fn header_type(&self) -> u16 {
        ffi::claimHeaderType(self.frame)
    }

    /// Set the frame header type.
    pub fn set_header_type(&mut self, header_type: u16) -> &mut Self {
        ffi::claimSetHeaderType(self.frame, header_type);
        self
    }

    /// The reserved value in the frame header.
    pub fn reserved_value(&self) -> i64 {
        ffi::claimReservedValue(self.frame)
    }

    /// Set the reserved value in the frame header (e.g. a checksum or timestamp).
    pub fn set_reserved_value(&mut self, value: i64) -> &mut Self {
        ffi::claimSetReservedValue(self.frame, value);
        self
    }

    /// Publish the claimed bytes. Returns the new stream position.
    pub fn commit(mut self) -> i64 {
        self.finished = true;
        ffi::claimCommit(self.frame);
        self.position
    }

    /// Abandon the claim; subscribers skip it.
    pub fn abort(mut self) {
        self.finished = true;
        ffi::claimAbort(self.frame);
    }
}

impl Drop for BufferClaim<'_> {
    fn drop(&mut self) {
        if !self.finished {
            ffi::claimAbort(self.frame);
        }
    }
}

impl std::fmt::Debug for BufferClaim<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BufferClaim")
            .field("position", &self.position)
            .field("len", &self.len())
            .finish()
    }
}

/// The two publication wrappers, for the shared vectored-offer helper.
trait OfferParts {
    fn offer_parts(
        &self,
        parts: &[ffi::OfferPart],
        supplier: fn(usize, &[u8]) -> i64,
        ctx: usize,
        use_supplier: bool,
    ) -> std::result::Result<i64, cxx::Exception>;
}

impl OfferParts for ffi::PublicationWrapper {
    fn offer_parts(
        &self,
        parts: &[ffi::OfferPart],
        supplier: fn(usize, &[u8]) -> i64,
        ctx: usize,
        use_supplier: bool,
    ) -> std::result::Result<i64, cxx::Exception> {
        self.offerParts(parts, supplier, ctx, use_supplier)
    }
}

impl OfferParts for ffi::ExclusivePublicationWrapper {
    fn offer_parts(
        &self,
        parts: &[ffi::OfferPart],
        supplier: fn(usize, &[u8]) -> i64,
        ctx: usize,
        use_supplier: bool,
    ) -> std::result::Result<i64, cxx::Exception> {
        self.offerParts(parts, supplier, ctx, use_supplier)
    }
}

/// Vectored offer with an optional reserved value supplier.
fn offer_parts<W, F>(
    wrapper: &W,
    parts: &[&[u8]],
    supplier: Option<F>,
) -> std::result::Result<i64, OfferError>
where
    W: OfferParts,
    F: FnMut(&[u8]) -> i64,
{
    const STACK_PARTS: usize = 16;
    let mut stack = [ffi::OfferPart { ptr: 0, len: 0 }; STACK_PARTS];
    let mut heap = Vec::new();
    let raw: &mut [ffi::OfferPart] = if parts.len() <= STACK_PARTS {
        &mut stack[..parts.len()]
    } else {
        heap.resize(parts.len(), ffi::OfferPart { ptr: 0, len: 0 });
        &mut heap
    };
    for (raw, part) in raw.iter_mut().zip(parts) {
        // Aeron buffer lengths are `int32`; reject longer parts instead of truncating them.
        claim_length(part.len())?;
        *raw = ffi::OfferPart {
            ptr: part.as_ptr() as usize,
            len: part.len(),
        };
    }
    let result = match supplier {
        None => wrapper.offer_parts(raw, callback::reserved_value::<F>, 0, false),
        Some(supplier) => {
            let mut cb = Callback::new(supplier);
            let result = wrapper.offer_parts(raw, callback::reserved_value::<F>, cb.ctx(), true);
            cb.finish(result)
        }
    };
    error::offer_result(result?)
}

/// Aeron claim lengths are `int32`; reject longer ones instead of truncating them.
fn claim_length(length: usize) -> std::result::Result<usize, OfferError> {
    if length > i32::MAX as usize {
        return Err(OfferError::Error(Error::new(
            ErrorKind::IllegalArgument,
            format!("length {length} exceeds the maximum of {}", i32::MAX),
        )));
    }
    Ok(length)
}
