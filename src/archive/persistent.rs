//! A subscription that replays a recording, then follows the live stream
//! ([`PersistentSubscription`]).

use super::context::Context;
use super::ffi;
use crate::callback::{self, Callback};
use crate::handlers::{self, into_ctx, release};
use crate::{AeronClient, Counter, Error, ErrorKind, Result};
use std::pin::Pin;

/// A subscription to a recorded stream that replays the recording from a
/// position, then joins the live stream (C++
/// `archive::client::PersistentSubscription`), falling back to a replay if it
/// loses the live stream. Created with [`PersistentSubscriptionBuilder`].
///
/// Poll it like a subscription; the callbacks set on the builder report when it
/// joins or leaves the live stream. `Send`.
pub struct PersistentSubscription {
    inner: cxx::UniquePtr<ffi::PersistentSubscriptionWrapper>,
}

impl Drop for PersistentSubscription {
    fn drop(&mut self) {
        callback::drop_outside_conductor(&mut self.inner);
    }
}

// SAFETY: only used through `&mut self` (or const accessors); its callbacks are
// `Send + Sync`, and it closes under the client's conductor lock.
unsafe impl Send for PersistentSubscription {}

impl std::fmt::Debug for PersistentSubscription {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PersistentSubscription")
            .field("is_live", &self.is_live())
            .field("is_replaying", &self.is_replaying())
            .field("has_failed", &self.has_failed())
            .finish_non_exhaustive()
    }
}

impl PersistentSubscription {
    /// Start from the beginning of the recording (C++ `FROM_START`).
    pub const FROM_START: i64 = -1;
    /// Start with the live stream (C++ `FROM_LIVE`, the default).
    pub const FROM_LIVE: i64 = -2;

    /// Poll for messages, replayed or live, driving the replay and the switch to
    /// the live stream. Returns the amount of work done: fragments read plus
    /// other progress (e.g. replay requests), so it can be positive with no
    /// message delivered; 0 when idle.
    ///
    /// # Panics
    ///
    /// If `handler` panics, the panic is resumed once Aeron returns from the poll;
    /// the remaining fragments of this poll are consumed without being delivered.
    pub fn poll<F>(&mut self, fragment_limit: usize, handler: F) -> Result<usize>
    where
        F: FnMut(&[u8], &crate::Header),
    {
        let fragment_limit = crate::error::ffi_limit(fragment_limit);
        let mut cb = Callback::new(handler);
        let result = self
            .inner
            .pin_mut()
            .poll(fragment_limit, callback::fragment::<F>, cb.ctx());
        Ok(crate::error::count(cb.finish(result)?))
    }

    /// Poll with flow control: `handler` returns a [`ControlledAction`](crate::ControlledAction).
    pub fn controlled_poll<F, R>(&mut self, fragment_limit: usize, handler: F) -> Result<usize>
    where
        F: FnMut(&[u8], &crate::Header) -> R,
        R: crate::PollAction,
    {
        let fragment_limit = crate::error::ffi_limit(fragment_limit);
        let mut cb = Callback::new(handler);
        let result = self.inner.pin_mut().controlledPoll(
            fragment_limit,
            callback::controlled_fragment::<F, R>,
            cb.ctx(),
        );
        Ok(crate::error::count(cb.finish(result)?))
    }

    /// Returns `true` while it follows the live stream.
    pub fn is_live(&self) -> bool {
        self.inner.isLive()
    }

    /// Returns `true` while it replays the recording.
    pub fn is_replaying(&self) -> bool {
        self.inner.isReplaying()
    }

    /// Returns `true` if it failed; [`failure_reason`](Self::failure_reason)
    /// says why (and the `on_error` callback was told).
    pub fn has_failed(&self) -> bool {
        self.inner.hasFailed()
    }

    /// Why it failed (C `aeron_archive_persistent_subscription_failure_reason`),
    /// or `None` while it hasn't: an [`ErrorKind::Archive`] error with Aeron's
    /// error code.
    pub fn failure_reason(&self) -> Option<Error> {
        let mut code = 0;
        let message = self.inner.failureReason(&mut code);
        (!message.is_empty()).then(|| Error::new(ErrorKind::Archive, message).with_code(code))
    }
}

/// Configuration for a [`PersistentSubscription`] (C++
/// `PersistentSubscription::Context`). Required: the archive context, the
/// recording ID, and the live and replay channels and stream IDs. Of the
/// archive context, only its connection settings are used (not its idle
/// strategy, delegating invoker or recording signal consumer).
///
/// Settings are applied as they are set; the first invalid one is reported by
/// [`create`](Self::create).
pub struct PersistentSubscriptionBuilder {
    inner: Result<cxx::UniquePtr<ffi::PersistentSubscriptionContextWrapper>>,
}

// SAFETY: the C++ context is not shared until `create`, and its callbacks are
// `Send + Sync`.
unsafe impl Send for PersistentSubscriptionBuilder {}

impl Drop for PersistentSubscriptionBuilder {
    fn drop(&mut self) {
        // Its archive context may hold the last reference to a client.
        if let Ok(inner) = &mut self.inner {
            callback::drop_outside_conductor(inner);
        }
    }
}

impl Default for PersistentSubscriptionBuilder {
    fn default() -> Self {
        Self {
            inner: ffi::create_persistent_subscription_context().map_err(Error::from),
        }
    }
}

impl std::fmt::Debug for PersistentSubscriptionBuilder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PersistentSubscriptionBuilder")
            .field("error", &self.inner.as_ref().err())
            .finish_non_exhaustive()
    }
}

impl PersistentSubscriptionBuilder {
    /// A builder with the defaults.
    pub fn new() -> Self {
        Self::default()
    }

    fn set(
        mut self,
        setting: impl FnOnce(Pin<&mut ffi::PersistentSubscriptionContextWrapper>) -> Result<()>,
    ) -> Self {
        if let Ok(ctx) = &mut self.inner
            && let Err(e) = setting(ctx.pin_mut())
        {
            self.inner = Err(e);
        }
        self
    }

    /// How to reach the archive that holds the recording (required).
    pub fn archive_context(self, archive: Context) -> Self {
        self.set(move |ctx| Ok(ctx.setArchiveContext(archive.build()?)?))
    }

    /// The client to subscribe with. Without one, the archive context's client
    /// is used, or an internal client in agent invoker mode, driven by `poll`.
    pub fn aeron(self, client: &AeronClient) -> Self {
        self.set(|ctx| Ok(ctx.setAeron(&client.inner)?))
    }

    /// The Aeron directory of the internal client.
    pub fn aeron_directory_name(self, dir: impl AsRef<std::path::Path>) -> Self {
        self.set(|ctx| Ok(ctx.setAeronDirectoryName(crate::error::path_str(dir.as_ref())?)?))
    }

    /// The recording to replay.
    pub fn recording_id(self, recording_id: i64) -> Self {
        self.set(|ctx| Ok(ctx.setRecordingId(recording_id)?))
    }

    /// Where to start: a recorded position,
    /// [`PersistentSubscription::FROM_START`] or
    /// [`PersistentSubscription::FROM_LIVE`] (the default).
    pub fn start_position(self, position: i64) -> Self {
        self.set(|ctx| Ok(ctx.setStartPosition(position)?))
    }

    /// The live stream's channel (required).
    pub fn live_channel(self, channel: &str) -> Self {
        self.set(|ctx| Ok(ctx.setLiveChannel(channel)?))
    }

    /// The live stream's stream ID.
    pub fn live_stream_id(self, stream_id: i32) -> Self {
        self.set(|ctx| Ok(ctx.setLiveStreamId(stream_id)?))
    }

    /// The channel to replay to (required).
    pub fn replay_channel(self, channel: &str) -> Self {
        self.set(|ctx| Ok(ctx.setReplayChannel(channel)?))
    }

    /// The stream ID to replay to.
    pub fn replay_stream_id(self, stream_id: i32) -> Self {
        self.set(|ctx| Ok(ctx.setReplayStreamId(stream_id)?))
    }

    /// A counter to keep the subscription's state in (one is allocated
    /// otherwise).
    ///
    /// The counters given to the builder must be added by the subscription's
    /// client ([`aeron`](Self::aeron)), not handles from a
    /// [`CountersReader`](crate::CountersReader). The subscription takes them
    /// over and closes them when it is dropped (or if `create` fails).
    pub fn state_counter(self, counter: Counter) -> Self {
        self.set(|ctx| Ok(ctx.setCounter(0, &counter.inner)?))
    }

    /// A counter to keep the difference between the replay and live positions
    /// in, while joining.
    pub fn join_difference_counter(self, counter: Counter) -> Self {
        self.set(|ctx| Ok(ctx.setCounter(1, &counter.inner)?))
    }

    /// A counter of how many times it left the live stream.
    pub fn live_left_counter(self, counter: Counter) -> Self {
        self.set(|ctx| Ok(ctx.setCounter(2, &counter.inner)?))
    }

    /// A counter of how many times it joined the live stream.
    pub fn live_joined_counter(self, counter: Counter) -> Self {
        self.set(|ctx| Ok(ctx.setCounter(3, &counter.inner)?))
    }

    /// Called (from `poll`) when it joins the live stream.
    pub fn on_live_joined<F>(self, callback: F) -> Self
    where
        F: Fn() + Send + Sync + 'static,
    {
        self.set(move |ctx| {
            Ok(ctx.setOnLiveJoined(live_trampoline::<F>, release::<F>, into_ctx(callback))?)
        })
    }

    /// Called (from `poll`) when it leaves the live stream.
    pub fn on_live_left<F>(self, callback: F) -> Self
    where
        F: Fn() + Send + Sync + 'static,
    {
        self.set(move |ctx| {
            Ok(ctx.setOnLiveLeft(live_trampoline::<F>, release::<F>, into_ctx(callback))?)
        })
    }

    /// Called (from `poll`) with an error, e.g. a failed replay; the
    /// [`Error::code`] is the Aeron error code.
    pub fn on_error<F>(self, callback: F) -> Self
    where
        F: Fn(&Error) + Send + Sync + 'static,
    {
        self.set(move |ctx| {
            Ok(ctx.setOnError(error_trampoline::<F>, release::<F>, into_ctx(callback))?)
        })
    }

    /// Create the subscription. It starts replaying (or joins the live stream)
    /// as it is polled.
    pub fn create(mut self) -> Result<PersistentSubscription> {
        callback::ensure_not_in_conductor_callback("creating a persistent subscription")?;
        let inner = std::mem::replace(&mut self.inner, Err(super::context::used()))?;
        Ok(PersistentSubscription {
            inner: ffi::create_persistent_subscription(inner)?,
        })
    }
}

fn live_trampoline<F: Fn() + Send + Sync + 'static>(ctx: usize) {
    handlers::invoke::<F>(ctx, "persistent subscription live", |f| f());
}

fn error_trampoline<F: Fn(&Error) + Send + Sync + 'static>(ctx: usize, code: i32, message: &[u8]) {
    let error = Error::new(ErrorKind::Aeron, String::from_utf8_lossy(message)).with_code(code);
    handlers::invoke::<F>(ctx, "persistent subscription error", |f| f(&error));
}
