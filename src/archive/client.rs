//! The archive client ([`AeronArchive`]).

use super::context::ContextInfo;
use super::ffi;
use super::types::{
    RecordingDescriptor, RecordingSubscriptionDescriptor, ReplayParams, ReplicationParams,
    SourceLocation,
};
use crate::callback::{self, Callback};
use crate::{ExclusivePublication, Publication, Result, Subscription};

/// A connection to an Aeron Archive (C++ `aeron::archive::client::AeronArchive`),
/// created with [`Context::connect`](super::Context::connect).
///
/// Requests wait for the archive's response, up to the context's message
/// timeout, and fail with the archive's error (see
/// [`ArchiveErrorCode`](super::ArchiveErrorCode)). `Send`; `Sync` too, as
/// requests are serialised by the archive client.
pub struct AeronArchive {
    inner: cxx::UniquePtr<ffi::ArchiveWrapper>,
}

impl Drop for AeronArchive {
    fn drop(&mut self) {
        callback::drop_outside_conductor(&mut self.inner);
    }
}

// SAFETY: the C archive client serialises requests with its own mutex, the
// shim's ArchiveWrapper methods are const, and its handlers are `Send + Sync`.
// In agent invoker mode the client's conductor lock is held for each request.
unsafe impl Send for AeronArchive {}
unsafe impl Sync for AeronArchive {}

impl std::fmt::Debug for AeronArchive {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AeronArchive")
            .field("archive_id", &self.archive_id())
            .field("control_session_id", &self.control_session_id())
            .finish_non_exhaustive()
    }
}

macro_rules! list {
    ($handler:ident, $info:ty, $out:ty) => {
        fn $handler<F: FnMut($out)>(ctx: usize, info: &$info) {
            // SAFETY: C++ only calls this with the ctx passed alongside it, during the call.
            let cb = unsafe { Callback::<F>::from_ctx(ctx) };
            // Inside the archive's response poll: archive requests from the
            // consumer would poll the same responses again.
            let _scope = callback::ConductorCallbackScope::enter();
            cb.call((), |f| f(<$out>::from_ffi(info)));
        }
    };
}

list!(
    recording_descriptor,
    ffi::RecordingDescriptorInfo,
    RecordingDescriptor
);
list!(
    recording_subscription,
    ffi::RecordingSubscriptionInfo,
    RecordingSubscriptionDescriptor
);

impl AeronArchive {
    pub(crate) fn new(inner: cxx::UniquePtr<ffi::ArchiveWrapper>) -> Self {
        Self { inner }
    }

    pub(crate) fn wrapper(&self) -> &ffi::ArchiveWrapper {
        &self.inner
    }

    /// The archive client, for a request: requests wait for the client
    /// conductor, so they cannot run inside a client (or archive) handler.
    fn request(&self) -> Result<&ffi::ArchiveWrapper> {
        callback::ensure_not_in_conductor_callback("an archive request")?;
        Ok(&self.inner)
    }

    /// Connect with default settings (C++ `AeronArchive::connect()`).
    pub fn connect_default() -> Result<Self> {
        super::Context::new().connect()
    }

    /// The settings this client connected with.
    pub fn context(&self) -> Result<ContextInfo> {
        Ok(ContextInfo::from_ffi(self.inner.context()?))
    }

    /// The ID of the archive this client is connected to.
    pub fn archive_id(&self) -> i64 {
        self.inner.archiveId()
    }

    /// The ID of this client's control session.
    pub fn control_session_id(&self) -> i64 {
        self.inner.controlSessionId()
    }

    /// Poll for recording signals, delivering them to the context's
    /// [`recording_signal_consumer`](super::Context::recording_signal_consumer).
    /// Returns the number of signals.
    pub fn poll_for_recording_signals(&self) -> Result<i32> {
        Ok(self.request()?.pollForRecordingSignals()?)
    }

    /// Poll for an error response from the archive, e.g. for an asynchronous
    /// request: the error message, if there is one.
    pub fn poll_for_error_response(&self) -> Result<Option<String>> {
        let message = self.request()?.pollForErrorResponse()?;
        Ok((!message.is_empty()).then_some(message))
    }

    /// Fail with the archive's error if an error response is waiting.
    pub fn check_for_error_response(&self) -> Result<()> {
        Ok(self.request()?.checkForErrorResponse()?)
    }

    /// Add a concurrent publication and record it (C++ `addRecordedPublication`).
    /// Fails if a publication for the channel and stream already exists in this
    /// client. Stop with [`stop_recording_publication`](Self::stop_recording_publication).
    pub fn add_recorded_publication(&self, channel: &str, stream_id: i32) -> Result<Publication> {
        Ok(Publication {
            inner: self.request()?.addRecordedPublication(channel, stream_id)?,
        })
    }

    /// Add an exclusive publication and record it.
    pub fn add_recorded_exclusive_publication(
        &self,
        channel: &str,
        stream_id: i32,
    ) -> Result<ExclusivePublication> {
        Ok(ExclusivePublication {
            inner: self
                .request()?
                .addRecordedExclusivePublication(channel, stream_id)?,
        })
    }

    /// Record a channel and stream: every publication on it becomes a recording.
    /// Returns the recording subscription's ID. With `auto_stop`, the recording
    /// stops when the stream ends.
    pub fn start_recording(
        &self,
        channel: &str,
        stream_id: i32,
        source: SourceLocation,
        auto_stop: bool,
    ) -> Result<i64> {
        Ok(self
            .request()?
            .startRecording(channel, stream_id, source as i32, auto_stop)?)
    }

    /// Extend a stopped recording with a stream that continues it (same
    /// session, term length and position). Returns the recording subscription's ID.
    pub fn extend_recording(
        &self,
        recording_id: i64,
        channel: &str,
        stream_id: i32,
        source: SourceLocation,
        auto_stop: bool,
    ) -> Result<i64> {
        Ok(self.request()?.extendRecording(
            recording_id,
            channel,
            stream_id,
            source as i32,
            auto_stop,
        )?)
    }

    /// Stop a recording subscription by the ID [`start_recording`](Self::start_recording)
    /// returned. Fails if there is no such subscription.
    pub fn stop_recording(&self, subscription_id: i64) -> Result<()> {
        Ok(self.request()?.stopRecording(subscription_id)?)
    }

    /// Like [`stop_recording`](Self::stop_recording), but returns `false` instead
    /// of failing if there is no such subscription.
    pub fn try_stop_recording(&self, subscription_id: i64) -> Result<bool> {
        Ok(self.request()?.tryStopRecording(subscription_id)?)
    }

    /// Stop recording a channel and stream.
    pub fn stop_recording_by_channel_and_stream(
        &self,
        channel: &str,
        stream_id: i32,
    ) -> Result<()> {
        Ok(self
            .request()?
            .stopRecordingByChannelAndStream(channel, stream_id)?)
    }

    /// Like [`stop_recording_by_channel_and_stream`](Self::stop_recording_by_channel_and_stream),
    /// but returns `false` instead of failing if it is not recorded.
    pub fn try_stop_recording_by_channel_and_stream(
        &self,
        channel: &str,
        stream_id: i32,
    ) -> Result<bool> {
        Ok(self
            .request()?
            .tryStopRecordingByChannelAndStream(channel, stream_id)?)
    }

    /// Stop a recording by its recording ID. Returns `false` if it was not
    /// active; fails with [`ArchiveErrorCode::UnknownRecording`](super::ArchiveErrorCode::UnknownRecording)
    /// if there is no such recording. Its recording subscription is removed (as
    /// in Java), even if it records other sessions.
    pub fn try_stop_recording_by_identity(&self, recording_id: i64) -> Result<bool> {
        Ok(self.request()?.tryStopRecordingByIdentity(recording_id)?)
    }

    /// Stop recording a publication added with
    /// [`add_recorded_publication`](Self::add_recorded_publication) (its session only).
    pub fn stop_recording_publication(&self, publication: &Publication) -> Result<()> {
        Ok(self
            .request()?
            .stopRecordingPublication(&publication.inner)?)
    }

    /// Stop recording an exclusive publication.
    pub fn stop_recording_exclusive_publication(
        &self,
        publication: &ExclusivePublication,
    ) -> Result<()> {
        Ok(self
            .request()?
            .stopRecordingExclusivePublication(&publication.inner)?)
    }

    /// Delete a stopped recording and its files. Returns the number of segment
    /// files deleted.
    pub fn purge_recording(&self, recording_id: i64) -> Result<i64> {
        Ok(self.request()?.purgeRecording(recording_id)?)
    }

    /// Change the original channel of a recording in the catalog.
    pub fn update_channel(&self, recording_id: i64, channel: &str) -> Result<()> {
        Ok(self.request()?.updateChannel(recording_id, channel)?)
    }

    /// Truncate a stopped recording to `position`. Returns the number of segment
    /// files deleted.
    pub fn truncate_recording(&self, recording_id: i64, position: i64) -> Result<i64> {
        Ok(self.request()?.truncateRecording(recording_id, position)?)
    }

    /// The position an active recording has reached, or
    /// [`NULL_POSITION`](super::NULL_POSITION) if it is not active.
    pub fn get_recording_position(&self, recording_id: i64) -> Result<i64> {
        Ok(self.request()?.getRecordingPosition(recording_id)?)
    }

    /// The position a recording starts at.
    pub fn get_start_position(&self, recording_id: i64) -> Result<i64> {
        Ok(self.request()?.getStartPosition(recording_id)?)
    }

    /// The position a recording stopped at, or
    /// [`NULL_POSITION`](super::NULL_POSITION) while it is active.
    pub fn get_stop_position(&self, recording_id: i64) -> Result<i64> {
        Ok(self.request()?.getStopPosition(recording_id)?)
    }

    /// The position recorded so far: the recording position while active,
    /// otherwise the stop position.
    pub fn get_max_recorded_position(&self, recording_id: i64) -> Result<i64> {
        Ok(self.request()?.getMaxRecordedPosition(recording_id)?)
    }

    /// The ID of the last recording at or after `min_recording_id` whose
    /// channel contains `channel_fragment`, with the stream ID and session ID.
    /// Returns -1 if there is none.
    pub fn find_last_matching_recording(
        &self,
        min_recording_id: i64,
        channel_fragment: &str,
        stream_id: i32,
        session_id: i32,
    ) -> Result<i64> {
        Ok(self.request()?.findLastMatchingRecording(
            min_recording_id,
            channel_fragment,
            stream_id,
            session_id,
        )?)
    }

    /// Describe one recording. Returns the number found (0 or 1).
    pub fn list_recording<F>(&self, recording_id: i64, handler: F) -> Result<i32>
    where
        F: FnMut(RecordingDescriptor),
    {
        let mut cb = Callback::new(handler);
        let result =
            self.request()?
                .listRecording(recording_id, recording_descriptor::<F>, cb.ctx());
        Ok(cb.finish(result)?)
    }

    /// Describe up to `record_count` recordings from `from_recording_id`.
    /// Returns the number found.
    pub fn list_recordings<F>(
        &self,
        from_recording_id: i64,
        record_count: i32,
        handler: F,
    ) -> Result<i32>
    where
        F: FnMut(RecordingDescriptor),
    {
        if !check_count(record_count)? {
            return Ok(0);
        }
        let mut cb = Callback::new(handler);
        let result = self.request()?.listRecordings(
            from_recording_id,
            record_count,
            recording_descriptor::<F>,
            cb.ctx(),
        );
        Ok(cb.finish(result)?)
    }

    /// Like [`list_recordings`](Self::list_recordings), only recordings whose
    /// channel contains `channel_fragment`, with the stream ID.
    pub fn list_recordings_for_uri<F>(
        &self,
        from_recording_id: i64,
        record_count: i32,
        channel_fragment: &str,
        stream_id: i32,
        handler: F,
    ) -> Result<i32>
    where
        F: FnMut(RecordingDescriptor),
    {
        if !check_count(record_count)? {
            return Ok(0);
        }
        let mut cb = Callback::new(handler);
        let result = self.request()?.listRecordingsForUri(
            from_recording_id,
            record_count,
            channel_fragment,
            stream_id,
            recording_descriptor::<F>,
            cb.ctx(),
        );
        Ok(cb.finish(result)?)
    }

    /// Describe up to `subscription_count` active recording subscriptions from
    /// `pseudo_index`, whose channel contains `channel_fragment` (and, if
    /// `apply_stream_id`, with the stream ID). Returns the number found.
    pub fn list_recording_subscriptions<F>(
        &self,
        pseudo_index: i32,
        subscription_count: i32,
        channel_fragment: &str,
        stream_id: i32,
        apply_stream_id: bool,
        handler: F,
    ) -> Result<i32>
    where
        F: FnMut(RecordingSubscriptionDescriptor),
    {
        if !check_count(subscription_count)? {
            return Ok(0);
        }
        let mut cb = Callback::new(handler);
        let result = self.request()?.listRecordingSubscriptions(
            pseudo_index,
            subscription_count,
            channel_fragment,
            stream_id,
            apply_stream_id,
            recording_subscription::<F>,
            cb.ctx(),
        );
        Ok(cb.finish(result)?)
    }

    /// Start replaying a recording to `channel` and `stream_id`, where you
    /// subscribe to it. Returns the replay session ID; the replay's images
    /// have the session ID `replay_session_id as i32`.
    pub fn start_replay(
        &self,
        recording_id: i64,
        channel: &str,
        stream_id: i32,
        params: &ReplayParams,
    ) -> Result<i64> {
        Ok(self.request()?.startReplay(
            recording_id,
            channel,
            stream_id,
            params.position,
            params.length,
            params.bounding_limit_counter_id,
            params.file_io_max_length,
            params.replay_token,
            params.subscription_registration_id,
        )?)
    }

    /// Subscribe to a replay of a recording and start it (C++ `replay`).
    ///
    /// With a response channel (`control-mode=response`) the archive would
    /// replay to this subscription only, but the C archive client of Aeron
    /// 1.53.3 fails to set that up (a session ID clash); this fails until it is
    /// fixed upstream.
    pub fn replay(
        &self,
        recording_id: i64,
        channel: &str,
        stream_id: i32,
        params: &ReplayParams,
    ) -> Result<Subscription> {
        Ok(Subscription {
            inner: self.request()?.replay(
                recording_id,
                channel,
                stream_id,
                params.position,
                params.length,
                params.bounding_limit_counter_id,
                params.file_io_max_length,
                params.replay_token,
                params.subscription_registration_id,
            )?,
        })
    }

    /// Stop a replay by its replay session ID.
    pub fn stop_replay(&self, replay_session_id: i64) -> Result<()> {
        Ok(self.request()?.stopReplay(replay_session_id)?)
    }

    /// Stop all replays of a recording (-1: of every recording).
    pub fn stop_all_replays(&self, recording_id: i64) -> Result<()> {
        Ok(self.request()?.stopAllReplays(recording_id)?)
    }

    /// Replicate a recording from another archive (the source, reached at
    /// `src_control_channel` / `src_control_stream_id`) into this one. Returns
    /// the replication ID.
    pub fn replicate(
        &self,
        src_recording_id: i64,
        src_control_stream_id: i32,
        src_control_channel: &str,
        params: &ReplicationParams,
    ) -> Result<i64> {
        Ok(self.request()?.replicate(
            src_recording_id,
            src_control_stream_id,
            src_control_channel,
            params.stop_position,
            params.dst_recording_id,
            &params.live_destination,
            &params.replication_channel,
            params.channel_tag_id,
            params.subscription_tag_id,
            params.file_io_max_length,
            params.replication_session_id,
            &params.encoded_credentials,
        )?)
    }

    /// Stop a replication. Fails if there is no such replication.
    pub fn stop_replication(&self, replication_id: i64) -> Result<()> {
        Ok(self.request()?.stopReplication(replication_id)?)
    }

    /// Like [`stop_replication`](Self::stop_replication), but returns `false`
    /// instead of failing if there is no such replication.
    pub fn try_stop_replication(&self, replication_id: i64) -> Result<bool> {
        Ok(self.request()?.tryStopReplication(replication_id)?)
    }

    /// Detach the segments before `new_start_position` from a recording (it then
    /// starts there), keeping the files.
    pub fn detach_segments(&self, recording_id: i64, new_start_position: i64) -> Result<()> {
        Ok(self
            .request()?
            .detachSegments(recording_id, new_start_position)?)
    }

    /// Delete the files of detached segments. Returns the number deleted.
    pub fn delete_detached_segments(&self, recording_id: i64) -> Result<i64> {
        Ok(self.request()?.deleteDetachedSegments(recording_id)?)
    }

    /// Detach and delete the segments before `new_start_position`. Returns the
    /// number deleted.
    pub fn purge_segments(&self, recording_id: i64, new_start_position: i64) -> Result<i64> {
        Ok(self
            .request()?
            .purgeSegments(recording_id, new_start_position)?)
    }

    /// Attach detached segment files back to the start of a recording. Returns
    /// the number attached.
    pub fn attach_segments(&self, recording_id: i64) -> Result<i64> {
        Ok(self.request()?.attachSegments(recording_id)?)
    }

    /// Move the segments of `src_recording_id` to the start of
    /// `dst_recording_id`, which must continue it. Returns the number moved.
    pub fn migrate_segments(&self, src_recording_id: i64, dst_recording_id: i64) -> Result<i64> {
        Ok(self
            .request()?
            .migrateSegments(src_recording_id, dst_recording_id)?)
    }

    /// The position of the start of the segment file containing `position`, for
    /// a recording starting at `start_position` (C++
    /// `segmentFileBasePosition`). The lengths are powers of two.
    pub fn segment_file_base_position(
        start_position: i64,
        position: i64,
        term_buffer_length: i32,
        segment_file_length: i32,
    ) -> i64 {
        // As the C function, with wrapping arithmetic (it overflows for bad
        // lengths in C).
        let start_term_base = start_position
            .wrapping_sub(start_position & i64::from(term_buffer_length.wrapping_sub(1)));
        let from_base = position.wrapping_sub(start_term_base);
        let segments =
            from_base.wrapping_sub(from_base & i64::from(segment_file_length.wrapping_sub(1)));
        start_term_base.wrapping_add(segments)
    }
}

/// Listings of 0 entries are answered without asking the archive (which never
/// answers them); negative counts are refused (the archive's listing would
/// never end, blocking later listings of the session).
fn check_count(count: i32) -> Result<bool> {
    if count < 0 {
        return Err(crate::Error::new(
            crate::ErrorKind::IllegalArgument,
            format!("negative listing count: {count}"),
        ));
    }
    Ok(count > 0)
}

/// An archive connection in progress, from
/// [`Context::connect_async`](super::Context::connect_async) (C++
/// `AeronArchive::AsyncConnect`). Dropping it abandons the connection; the
/// archive client in Aeron 1.53.3 cannot release an abandoned connection, whose
/// publication and subscription then stay open until the client closes.
///
/// After [`poll`](Self::poll) fails, the connection is over: polling again
/// returns an error (upstream would use freed memory). With an agent invoker
/// client, `poll` also runs the client's conductor.
pub struct AsyncConnect {
    inner: cxx::UniquePtr<ffi::ArchiveAsyncConnectWrapper>,
    done: bool,
}

impl Drop for AsyncConnect {
    fn drop(&mut self) {
        callback::drop_outside_conductor(&mut self.inner);
    }
}

// SAFETY: as for AeronArchive; it is only used through `&mut self`.
unsafe impl Send for AsyncConnect {}

impl std::fmt::Debug for AsyncConnect {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AsyncConnect")
            .field("done", &self.done)
            .finish_non_exhaustive()
    }
}

impl AsyncConnect {
    pub(crate) fn new(inner: cxx::UniquePtr<ffi::ArchiveAsyncConnectWrapper>) -> Self {
        Self { inner, done: false }
    }

    /// Advance the connection: returns the archive client once connected, `None`
    /// while connecting. Fails if the connection failed or already completed.
    pub fn poll(&mut self) -> Result<Option<AeronArchive>> {
        if self.done {
            return Err(crate::Error::new(
                crate::ErrorKind::IllegalState,
                "the archive connection already completed",
            ));
        }
        let archive = self
            .inner
            .pin_mut()
            .poll()
            // A failed poll frees the C connection: never poll it again.
            .inspect_err(|_| self.done = true)?;
        if archive.is_null() {
            return Ok(None);
        }
        self.done = true;
        Ok(Some(AeronArchive::new(archive)))
    }
}
