//! Merging a replay with a live stream ([`ReplayMerge`]).

use super::client::AeronArchive;
use super::ffi;
use crate::Result;
use crate::callback::{self, Callback};
use std::marker::PhantomData;

/// Default timeout for replay merge progress (C++
/// `REPLAY_MERGE_PROGRESS_TIMEOUT_DEFAULT_MS`).
pub const REPLAY_MERGE_PROGRESS_TIMEOUT_DEFAULT: std::time::Duration =
    std::time::Duration::from_secs(5);

/// Merges a replay of a recording with the live stream it records (C++
/// `ReplayMerge`): a late joiner catches up from the archive, then switches to
/// the live stream without a gap. UDP only.
///
/// The subscription must use `control-mode=manual`: the merge adds the replay
/// and live destinations to it. The merge mutably borrows the subscription and
/// the archive client for its whole life: it polls the subscription, and uses
/// the archive client's connection without its lock. In agent invoker mode the
/// subscription must belong to the archive client's client, and the client's
/// conductor must still be run ([`AeronClient::invoke`](crate::AeronClient::invoke))
/// between polls of the merge.
///
/// Dropping the merge closes it, removing its destinations and stopping its
/// replay. Drop it outside Aeron handlers: dropped inside one, it is leaked and
/// its replay runs until the archive client closes.
///
/// ```compile_fail,E0505
/// # use aeron_glide::{AeronClient, archive::{AeronArchive, ReplayMerge}};
/// # let client = AeronClient::new().unwrap();
/// # let mut archive = AeronArchive::connect_default().unwrap();
/// let mut sub = client.add_subscription("aeron:udp?control-mode=manual", 1).unwrap();
/// let mut merge = ReplayMerge::new(&mut sub, &mut archive, "", "", "", 0, 0).unwrap();
/// std::thread::spawn(move || sub.poll(10, |_, _| {})); // error: `sub` is borrowed
/// merge.do_work().unwrap();
/// ```
///
/// ```compile_fail,E0505
/// # use aeron_glide::{AeronClient, archive::{AeronArchive, ReplayMerge}};
/// # let client = AeronClient::new().unwrap();
/// # let mut archive = AeronArchive::connect_default().unwrap();
/// # let mut sub = client.add_subscription("aeron:udp?control-mode=manual", 1).unwrap();
/// let mut merge = ReplayMerge::new(&mut sub, &mut archive, "", "", "", 0, 0).unwrap();
/// drop(archive); // error: `archive` is borrowed
/// merge.do_work().unwrap();
/// ```
pub struct ReplayMerge<'a> {
    inner: cxx::UniquePtr<ffi::ReplayMergeWrapper>,
    _borrows: PhantomData<(&'a mut crate::Subscription, &'a mut AeronArchive)>,
}

impl Drop for ReplayMerge<'_> {
    fn drop(&mut self) {
        // Closing must finish before the borrows end, so it is never moved to
        // another thread. Inside an Aeron handler (possibly on the conductor
        // thread that closing would wait for), the merge is leaked instead: its
        // replay keeps running until the archive client closes.
        if callback::in_conductor_callback() {
            std::mem::forget(std::mem::replace(&mut self.inner, cxx::UniquePtr::null()));
        }
    }
}

// SAFETY: it holds the subscription (borrowed mutably) and the archive client
// (`Send + Sync`) through `shared_ptr`s, and is only used through `&mut self`.
unsafe impl Send for ReplayMerge<'_> {}

impl std::fmt::Debug for ReplayMerge<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ReplayMerge")
            .field("is_merged", &self.is_merged())
            .field("has_failed", &self.has_failed())
            .finish_non_exhaustive()
    }
}

impl<'a> ReplayMerge<'a> {
    /// Start merging, with the default progress timeout.
    ///
    /// - `replay_channel`: the channel the archive replays to, carrying the
    ///   live publication's `session-id` (e.g. `aeron:udp?session-id=5`); the
    ///   merge sets its endpoint from `replay_destination`.
    /// - `replay_destination`: the destination added to `subscription` for the
    ///   replay: a UDP channel with an endpoint, e.g. `aeron:udp?endpoint=localhost:0`.
    /// - `live_destination`: the destination added for the live stream.
    /// - `recording_id` and `start_position`: what to replay from.
    pub fn new(
        subscription: &'a mut crate::Subscription,
        archive: &'a mut AeronArchive,
        replay_channel: &str,
        replay_destination: &str,
        live_destination: &str,
        recording_id: i64,
        start_position: i64,
    ) -> Result<Self> {
        Self::with_progress_timeout(
            subscription,
            archive,
            replay_channel,
            replay_destination,
            live_destination,
            recording_id,
            start_position,
            REPLAY_MERGE_PROGRESS_TIMEOUT_DEFAULT,
        )
    }

    /// Like [`new`](Self::new), failing the merge if it makes no progress for
    /// `merge_progress_timeout`.
    #[allow(clippy::too_many_arguments)]
    pub fn with_progress_timeout(
        subscription: &'a mut crate::Subscription,
        archive: &'a mut AeronArchive,
        replay_channel: &str,
        replay_destination: &str,
        live_destination: &str,
        recording_id: i64,
        start_position: i64,
        merge_progress_timeout: std::time::Duration,
    ) -> Result<Self> {
        crate::callback::ensure_not_in_conductor_callback("starting a replay merge")?;
        // The C merge dereferences the replay destination's endpoint unchecked.
        let destination = crate::ChannelUri::parse(replay_destination)?;
        if destination.media() != crate::channel::UDP_MEDIA
            || destination
                .get(crate::channel::ENDPOINT_PARAM_NAME)
                .is_none_or(|endpoint| endpoint.len() < 2)
        {
            return Err(crate::Error::new(
                crate::ErrorKind::IllegalArgument,
                format!(
                    "the replay destination must be a UDP channel with an endpoint: {replay_destination}"
                ),
            ));
        }
        let inner = ffi::create_replay_merge(
            subscription.inner_pin_mut(),
            archive.wrapper(),
            replay_channel,
            replay_destination,
            live_destination,
            recording_id,
            start_position,
            // C++ adds it to the clock: clamp far below overflow.
            crate::timeout_millis(merge_progress_timeout),
        )?;
        Ok(Self {
            inner,
            _borrows: PhantomData,
        })
    }

    /// Drive the merge. Call it regularly (or use [`poll`](Self::poll)). Returns
    /// the amount of work done.
    pub fn do_work(&mut self) -> Result<i32> {
        Ok(self.inner.pin_mut().doWork()?)
    }

    /// Drive the merge and poll the merged stream (C++ `ReplayMerge::poll`).
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

    /// Drive the merge and poll for reassembled messages:
    /// [`do_work`](Self::do_work), then `Image::poll_assembled` on the image
    /// being merged, if there is one yet.
    pub fn poll_assembled<R, F>(&mut self, fragment_limit: usize, handler: F) -> Result<usize>
    where
        R: crate::PollAction,
        F: FnMut(&[u8], &crate::Header) -> R,
    {
        self.do_work()?;
        match self.image() {
            Some(mut image) => image.poll_assembled(fragment_limit, handler),
            None => Ok(0),
        }
    }

    /// The image being merged, once the replay has started. It borrows the
    /// merge, so the merge cannot be polled while it is alive.
    pub fn image(&mut self) -> Option<crate::Image<'_>> {
        crate::Image::from_raw(self.inner.pin_mut().image())
    }

    /// Returns `true` once the replay has merged with the live stream.
    pub fn is_merged(&self) -> bool {
        self.inner.isMerged()
    }

    /// Returns `true` if the merge failed.
    pub fn has_failed(&self) -> bool {
        self.inner.hasFailed()
    }

    /// Returns `true` once the live destination has been added.
    pub fn is_live_added(&self) -> bool {
        self.inner.isLiveAdded()
    }
}
