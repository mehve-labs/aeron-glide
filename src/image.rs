//! Images ([`Image`]): one publisher session as seen by a subscription.

use super::*;

/// A single publisher session as seen by a subscriber.
///
/// Each publisher session creates one image on each matching subscription.
/// Images track their own position and can be polled independently.
///
/// Polling an image from inside a handler that is already polling the same image
/// (through another handle) fails with [`ErrorKind::Reentrant`](crate::ErrorKind::Reentrant).
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
    pub(crate) inner: cxx::UniquePtr<ffi::ImageWrapper>,
    _owner: PhantomData<&'a Subscription>,
}

impl std::fmt::Debug for Image<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Image")
            .field("session_id", &self.session_id())
            .field("correlation_id", &self.correlation_id())
            .field("source_identity", &self.source_identity())
            .finish_non_exhaustive()
    }
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

    /// The initial term ID of the stream.
    pub fn initial_term_id(&self) -> i32 {
        self.inner.initialTermId()
    }

    /// The length of each term in the image's log buffer.
    pub fn term_buffer_length(&self) -> usize {
        self.inner.termBufferLength() as usize
    }

    /// The number of bits to shift a term ID by to get a stream position.
    pub fn position_bits_to_shift(&self) -> i32 {
        self.inner.positionBitsToShift()
    }

    /// The counter ID of this image's subscriber position, for reading it from a
    /// [`CountersReader`](crate::CountersReader).
    pub fn subscriber_position_id(&self) -> i32 {
        self.inner.subscriberPositionId()
    }

    /// The registration ID of the subscription this image belongs to.
    pub fn subscription_registration_id(&self) -> i64 {
        self.inner.subscriptionRegistrationId()
    }

    /// Returns `true` if the publisher revoked the publication (see
    /// [`ExclusivePublication::revoke`](crate::ExclusivePublication::revoke)).
    pub fn is_publication_revoked(&self) -> bool {
        self.inner.isPublicationRevoked()
    }

    /// The number of network transports (connections) that recently delivered
    /// frames to this image, e.g. several for a multi-destination subscription.
    /// 0 for IPC. The media driver updates it periodically, so it lags new
    /// connections.
    pub fn active_transport_count(&self) -> Result<i32> {
        Ok(self.inner.activeTransportCount()?)
    }

    /// Ask the media driver to reject (disconnect) the remote publisher of this
    /// image, with a reason reported to it.
    pub fn reject(&self, reason: &str) -> Result<()> {
        Ok(self.inner.reject(reason)?)
    }

    /// Poll for fragments without reassembly, with flow control: the handler
    /// returns `()` (continue) or a [`ControlledAction`].
    ///
    /// # Panics
    ///
    /// If `handler` panics, the panic is resumed once Aeron returns from the poll;
    /// the fragment being handled is aborted and delivered again by the next poll.
    pub fn controlled_poll<R, F>(&mut self, limit: usize, handler: F) -> Result<usize>
    where
        R: PollAction,
        F: FnMut(&[u8], &Header) -> R,
    {
        let limit = crate::error::ffi_limit(limit);
        let mut cb = Callback::new(handler);
        let result = self.inner.pin_mut().controlledPoll(
            limit,
            callback::controlled_fragment::<F, R>,
            cb.ctx(),
        );
        Ok(crate::error::count(cb.finish(result)?))
    }

    /// Like [`poll`](Self::poll), but only delivers fragments that start before
    /// `limit_position` in the stream (a fragment straddling it is delivered).
    ///
    /// # Panics
    ///
    /// As for [`poll`](Self::poll).
    pub fn bounded_poll<F>(
        &mut self,
        limit_position: i64,
        fragment_limit: usize,
        handler: F,
    ) -> Result<usize>
    where
        F: FnMut(&[u8], &Header),
    {
        let fragment_limit = crate::error::ffi_limit(fragment_limit);
        let mut cb = Callback::new(handler);
        let result = self.inner.pin_mut().boundedPoll(
            limit_position,
            fragment_limit,
            callback::fragment::<F>,
            cb.ctx(),
        );
        Ok(crate::error::count(cb.finish(result)?))
    }

    /// Like [`controlled_poll`](Self::controlled_poll), but only delivers fragments
    /// that start before `limit_position` in the stream.
    ///
    /// # Panics
    ///
    /// As for [`controlled_poll`](Self::controlled_poll).
    pub fn bounded_controlled_poll<R, F>(
        &mut self,
        limit_position: i64,
        fragment_limit: usize,
        handler: F,
    ) -> Result<usize>
    where
        R: PollAction,
        F: FnMut(&[u8], &Header) -> R,
    {
        let fragment_limit = crate::error::ffi_limit(fragment_limit);
        let mut cb = Callback::new(handler);
        let result = self.inner.pin_mut().boundedControlledPoll(
            limit_position,
            fragment_limit,
            callback::controlled_fragment::<F, R>,
            cb.ctx(),
        );
        Ok(crate::error::count(cb.finish(result)?))
    }

    /// Like [`poll_assembled`](Self::poll_assembled), but only delivers messages
    /// whose fragments start before `limit_position` in the stream.
    ///
    /// # Panics
    ///
    /// As for [`poll_assembled`](Self::poll_assembled).
    pub fn bounded_poll_assembled<R, F>(
        &mut self,
        limit_position: i64,
        fragment_limit: usize,
        handler: F,
    ) -> Result<usize>
    where
        R: PollAction,
        F: FnMut(&[u8], &Header) -> R,
    {
        let fragment_limit = crate::error::ffi_limit(fragment_limit);
        let mut cb = Callback::new(handler);
        let result = self.inner.pin_mut().boundedControlledPollAssembled(
            limit_position,
            fragment_limit,
            callback::controlled_fragment::<F, R>,
            cb.ctx(),
        );
        Ok(crate::error::count(cb.finish(result)?))
    }

    /// Poll a block of whole frames (headers included) of up to
    /// `block_length_limit` bytes, calling `handler(block, session_id, term_id)`.
    /// Returns the number of bytes consumed.
    pub fn block_poll<F>(&mut self, block_length_limit: usize, handler: F) -> Result<usize>
    where
        F: FnMut(&[u8], i32, i32),
    {
        let block_length_limit = crate::error::ffi_limit(block_length_limit);
        let mut cb = Callback::new(handler);
        let result =
            self.inner
                .pin_mut()
                .blockPoll(block_length_limit, callback::block::<F>, cb.ctx());
        Ok(crate::error::count(cb.finish(result)?))
    }

    /// Poll this specific image for fragments. Returns the number of fragments dispatched.
    ///
    /// # Panics
    ///
    /// If `handler` panics, the panic is resumed once Aeron returns from the poll;
    /// the remaining fragments of this poll are consumed without being delivered.
    pub fn poll<F>(&mut self, limit: usize, handler: F) -> Result<usize>
    where
        F: FnMut(&[u8], &Header),
    {
        let limit = crate::error::ffi_limit(limit);
        let mut cb = Callback::new(handler);
        let result = self
            .inner
            .pin_mut()
            .poll(limit, callback::fragment::<F>, cb.ctx());
        Ok(crate::error::count(cb.finish(result)?))
    }

    /// Poll this image with automatic fragment reassembly, like
    /// [`Subscription::poll_assembled`].
    ///
    /// Reassembly state is shared with the subscription and its other `Image`
    /// handles, so a message may be completed by any of them. A raw (unassembled)
    /// poll in the middle of a partially reassembled message makes that message
    /// be dropped. Fails with
    /// [`ErrorKind::Reentrant`](crate::ErrorKind::Reentrant) if called from inside
    /// another assembled poll on the same subscription.
    ///
    /// # Panics
    ///
    /// If `handler` panics, the panic is resumed once Aeron returns from the poll;
    /// the fragment being handled is aborted and delivered again by the next poll.
    pub fn poll_assembled<R, F>(&mut self, limit: usize, handler: F) -> Result<usize>
    where
        R: PollAction,
        F: FnMut(&[u8], &Header) -> R,
    {
        let limit = crate::error::ffi_limit(limit);
        let mut cb = Callback::new(handler);
        let result = self.inner.pin_mut().controlledPollAssembled(
            limit,
            callback::controlled_fragment::<F, R>,
            cb.ctx(),
        );
        Ok(crate::error::count(cb.finish(result)?))
    }
}
