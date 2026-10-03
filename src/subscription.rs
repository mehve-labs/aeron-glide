//! Subscriptions ([`Subscription`]) and controlled polling.

use super::*;

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

/// A subscription for receiving messages on a channel+stream.
///
/// `Send` but not `Sync`: it can move to another thread, but polling is
/// single-threaded. [`Image`]s borrow the subscription, so they stay on its thread.
pub struct Subscription {
    pub(crate) inner: cxx::UniquePtr<ffi::SubscriptionWrapper>,
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
    pub fn poll<F>(&mut self, limit: i32, handler: F) -> Result<i32>
    where
        F: FnMut(&[u8]),
    {
        let mut cb = Callback::new(handler);
        let result = self
            .inner
            .pin_mut()
            .poll(limit, callback::fragment::<F>, cb.ctx());
        Ok(cb.finish(result)?)
    }

    /// Poll with automatic fragment reassembly. Messages that span multiple fragments
    /// are reassembled before being delivered to the handler, which always receives
    /// complete messages.
    ///
    /// The handler can return `()` (maps to Continue) or a `ControlledAction` for
    /// flow-control (Abort to retry, Break to stop, Commit to checkpoint, Continue to proceed).
    ///
    /// # Panics
    ///
    /// If `handler` panics, the panic is resumed once Aeron returns from the poll;
    /// the fragment being handled is aborted and delivered again by the next poll.
    pub fn poll_assembled<R, F>(&mut self, limit: i32, handler: F) -> Result<i32>
    where
        R: PollAction,
        F: FnMut(&[u8]) -> R,
    {
        let mut cb = Callback::new(handler);
        let result = self.inner.pin_mut().controlledPollAssembled(
            limit,
            callback::controlled_fragment::<F, R>,
            cb.ctx(),
        );
        Ok(cb.finish(result)?)
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
        Ok(self.inner.findDestinationResponse(correlation_id)?)
    }

    #[cfg(feature = "archive")]
    pub(crate) fn inner_pin_mut(&mut self) -> std::pin::Pin<&mut ffi::SubscriptionWrapper> {
        self.inner.pin_mut()
    }

    /// The number of active images (one per publisher session) on this subscription.
    pub fn image_count(&self) -> i32 {
        self.inner.imageCount()
    }

    /// Get an image by its index (0-based), or `None` if there is no image at that index.
    /// Images appear in the order they were connected.
    ///
    /// The image borrows this subscription, so the subscription cannot be polled
    /// (or dropped) while the image is alive. Each call returns a new handle with
    /// its own reassembly state (see [`Image::poll_assembled`]).
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
