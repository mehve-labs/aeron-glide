//! Subscriptions ([`Subscription`]) and controlled polling.

use super::*;

/// Flow-control actions returned by the handlers of `controlled_poll`,
/// `poll_assembled` and the bounded controlled polls.
/// The values are Aeron's `ControlledPollAction` (`AERON_ACTION_*`, 1 to 4).
#[repr(i32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ControlledAction {
    /// Abort polling — rewind position, re-deliver this fragment next poll.
    Abort = 1,
    /// Stop polling this image, commit position up to this fragment.
    Break = 2,
    /// Checkpoint position for flow control, continue polling.
    Commit = 3,
    /// Continue processing (default behavior).
    Continue = 4,
}

mod sealed {
    pub trait Sealed {}
    impl Sealed for () {}
    impl Sealed for super::ControlledAction {}
}

/// What an assembled poll handler returns: `()` (continue) or a
/// [`ControlledAction`]. Implemented for those two types only.
pub trait PollAction: sealed::Sealed {
    /// The action this return value stands for (`()` is `Continue`).
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
    pub(crate) inner: cxx::UniquePtr<ffi::SubscriptionWrapper>,
}

impl std::fmt::Debug for Subscription {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Subscription")
            .field("channel", &self.channel())
            .field("stream_id", &self.stream_id())
            .field("registration_id", &self.registration_id())
            .finish_non_exhaustive()
    }
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

    /// Poll with automatic fragment reassembly. Messages that span multiple fragments
    /// are reassembled before being delivered to the handler, which always receives
    /// complete messages.
    ///
    /// The handler can return `()` (maps to Continue) or a `ControlledAction` for
    /// flow-control (Abort to retry, Break to stop, Commit to checkpoint, Continue to proceed).
    ///
    /// Reassembly buffers are kept per publisher session and shared with this
    /// subscription's [`Image`] handles; free one with
    /// [`delete_session_buffer`](Self::delete_session_buffer).
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

    /// Poll for fragments without reassembly, with flow control: the handler returns
    /// `()` (continue) or a [`ControlledAction`] (abort to re-deliver, break, commit).
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

    /// Poll each image for a block of whole frames (headers included) of up to
    /// `block_length_limit` bytes, calling `handler(block, session_id, term_id)`.
    /// Returns the number of bytes consumed.
    ///
    /// For relaying or recording streams without parsing individual fragments.
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

    /// The channel URI this subscription was added with.
    pub fn channel(&self) -> String {
        self.inner.channel()
    }

    /// The stream ID this subscription was added with.
    pub fn stream_id(&self) -> i32 {
        self.inner.streamId()
    }

    /// The registration ID of this subscription with the media driver.
    pub fn registration_id(&self) -> i64 {
        self.inner.registrationId()
    }

    /// The status of the subscription's channel endpoint;
    /// [`ChannelStatus::NoStatus`] for IPC channels and closed subscriptions.
    pub fn channel_status(&self) -> Result<ChannelStatus> {
        let status = self.inner.channelStatus()?;
        let unavailable = self.channel_status_id() < 0 || self.is_closed();
        Ok(ChannelStatus::from_c(status, unavailable))
    }

    /// The counter ID of the channel status, for reading it from a
    /// [`CountersReader`](crate::CountersReader).
    pub fn channel_status_id(&self) -> i32 {
        self.inner.channelStatusId()
    }

    /// Returns `true` once the subscription has been closed.
    pub fn is_closed(&self) -> bool {
        self.inner.isClosed()
    }

    /// The local socket addresses the channel is bound to (several for a
    /// multi-destination subscription). Empty unless the channel is active.
    pub fn local_socket_addresses(&self) -> Result<Vec<String>> {
        Ok(self.inner.localSocketAddresses()?)
    }

    /// The endpoint the subscription is bound to, with a wildcard port (`:0`)
    /// resolved, or `None` if it is not bound yet.
    pub fn resolved_endpoint(&self) -> Result<Option<String>> {
        let endpoint = self.inner.resolvedEndpoint()?;
        Ok((!endpoint.is_empty()).then_some(endpoint))
    }

    /// The channel URI with a wildcard endpoint port (`:0`) replaced by the bound
    /// port, e.g. to hand to publishers. Returns `None` while the port is not bound
    /// yet; channels without a wildcard port are returned unchanged.
    pub fn try_resolve_channel_endpoint_port(&self) -> Result<Option<String>> {
        let channel = self.inner.tryResolveChannelEndpointPort()?;
        Ok((!channel.is_empty()).then_some(channel))
    }

    /// A snapshot of the subscription's current images (C++ `copyOfImageList`).
    /// Each image borrows this subscription.
    pub fn images(&self) -> Vec<Image<'_>> {
        let list = self.inner.copyOfImageList();
        (0..list.count())
            .filter_map(|i| Image::from_raw(list.get(i)))
            .collect()
    }

    /// Call `f` for each of the subscription's current images (C++ `forEachImage`).
    /// Returns the number of images visited.
    pub fn for_each_image<F>(&self, mut f: F) -> usize
    where
        F: FnMut(&Image<'_>),
    {
        let images = self.images();
        images.iter().for_each(&mut f);
        images.len()
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

    /// Add a destination to a multi-destination subscription (channel with
    /// `control-mode=manual`), e.g. `"aeron:udp?endpoint=host:port"`.
    ///
    /// Returns a correlation ID; the destination is in use once
    /// [`find_destination_response`](Self::find_destination_response) returns `true`.
    pub fn add_destination(&self, endpoint_channel: &str) -> Result<i64> {
        Ok(self.inner.addDestination(endpoint_channel)?)
    }

    /// Remove a destination added with [`add_destination`](Self::add_destination).
    /// Returns a correlation ID to pass to
    /// [`find_destination_response`](Self::find_destination_response).
    pub fn remove_destination(&self, endpoint_channel: &str) -> Result<i64> {
        Ok(self.inner.removeDestination(endpoint_channel)?)
    }

    /// Returns `true` once the media driver has applied the destination change with
    /// this correlation ID, `false` while it is pending; fails if the driver
    /// rejected it or the ID is unknown.
    pub fn find_destination_response(&self, correlation_id: i64) -> Result<bool> {
        self.inner
            .findDestinationResponse(correlation_id)
            .map_err(|e| Error::from(e).as_registration())
    }

    #[cfg(feature = "archive")]
    pub(crate) fn inner_pin_mut(&mut self) -> std::pin::Pin<&mut ffi::SubscriptionWrapper> {
        self.inner.pin_mut()
    }

    /// The number of active images (one per publisher session) on this subscription.
    pub fn image_count(&self) -> usize {
        crate::error::count(self.inner.imageCount())
    }

    /// Get an image by its index (0-based), or `None` if there is no image at that index.
    /// Images appear in the order they were connected.
    ///
    /// The image borrows this subscription, so the subscription cannot be polled
    /// (or dropped) while the image is alive. Handles share the subscription's
    /// reassembly state (see [`Image::poll_assembled`]).
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
