//! Images ([`Image`]): one publisher session as seen by a subscription.

use super::*;

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
    pub(crate) inner: cxx::UniquePtr<ffi::ImageWrapper>,
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
        F: FnMut(&[u8], &Header),
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
        F: FnMut(&[u8], &Header) -> R,
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
