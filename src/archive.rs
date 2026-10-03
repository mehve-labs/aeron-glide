#[allow(clippy::too_many_arguments, clippy::type_complexity)]
#[cxx::bridge(namespace = "aeron_rs")]
pub mod ffi {
    unsafe extern "C++" {
        include!("shim.h");

        // Cross-bridge type aliases (defined in lib.rs cxx bridge)
        type SubscriptionWrapper = crate::ffi::SubscriptionWrapper;
        type ImageWrapper = crate::ffi::ImageWrapper;

        type ArchiveWrapper;

        fn connect_archive(
            control_request_channel: &str,
            control_request_stream_id: i32,
            control_response_channel: &str,
            control_response_stream_id: i32,
        ) -> Result<UniquePtr<ArchiveWrapper>>;

        // Recording
        fn startRecording(
            self: Pin<&mut ArchiveWrapper>,
            channel: &str,
            stream_id: i32,
            source_location: i32,
            auto_stop: bool,
        ) -> Result<i64>;
        fn stopRecording(self: Pin<&mut ArchiveWrapper>, subscription_id: i64) -> Result<()>;
        fn stopRecordingByChannelAndStream(
            self: Pin<&mut ArchiveWrapper>,
            channel: &str,
            stream_id: i32,
        ) -> Result<()>;

        // Position queries
        fn getRecordingPosition(self: Pin<&mut ArchiveWrapper>, recording_id: i64) -> Result<i64>;
        fn getStartPosition(self: Pin<&mut ArchiveWrapper>, recording_id: i64) -> Result<i64>;
        fn getStopPosition(self: Pin<&mut ArchiveWrapper>, recording_id: i64) -> Result<i64>;
        fn getMaxRecordedPosition(self: Pin<&mut ArchiveWrapper>, recording_id: i64)
        -> Result<i64>;

        // Listing
        fn listRecordings(
            self: Pin<&mut ArchiveWrapper>,
            from_recording_id: i64,
            record_count: i32,
            handler: fn(
                usize,
                i64,
                i64,
                i64,
                i64,
                i64,
                i64,
                i64,
                i32,
                i32,
                i32,
                i32,
                i32,
                i32,
                &[u8],
                &[u8],
            ),
            ctx: usize,
        ) -> Result<i32>;
        fn listRecordingsForUri(
            self: Pin<&mut ArchiveWrapper>,
            from_recording_id: i64,
            record_count: i32,
            channel_fragment: &str,
            stream_id: i32,
            handler: fn(
                usize,
                i64,
                i64,
                i64,
                i64,
                i64,
                i64,
                i64,
                i32,
                i32,
                i32,
                i32,
                i32,
                i32,
                &[u8],
                &[u8],
            ),
            ctx: usize,
        ) -> Result<i32>;
        fn findLastMatchingRecording(
            self: Pin<&mut ArchiveWrapper>,
            min_recording_id: i64,
            channel_fragment: &str,
            stream_id: i32,
            session_id: i32,
        ) -> Result<i64>;

        // Replay
        fn startReplay(
            self: Pin<&mut ArchiveWrapper>,
            recording_id: i64,
            replay_channel: &str,
            replay_stream_id: i32,
            position: i64,
            length: i64,
        ) -> Result<i64>;
        fn stopReplay(self: Pin<&mut ArchiveWrapper>, replay_session_id: i64) -> Result<()>;
        fn stopAllReplays(self: Pin<&mut ArchiveWrapper>, recording_id: i64) -> Result<()>;

        // Truncate
        fn truncateRecording(
            self: Pin<&mut ArchiveWrapper>,
            recording_id: i64,
            position: i64,
        ) -> Result<i64>;

        // Error polling
        fn pollForErrorResponse(self: Pin<&mut ArchiveWrapper>) -> Result<String>;
        fn checkForErrorResponse(self: Pin<&mut ArchiveWrapper>) -> Result<()>;

        // Info
        fn archiveId(self: &ArchiveWrapper) -> i64;
        fn controlSessionId(self: &ArchiveWrapper) -> i64;

        // ReplayMerge
        type ReplayMergeWrapper;

        fn create_replay_merge(
            subscription: Pin<&mut SubscriptionWrapper>,
            archive: Pin<&mut ArchiveWrapper>,
            replay_channel: &str,
            replay_destination: &str,
            live_destination: &str,
            recording_id: i64,
            start_position: i64,
            merge_progress_timeout_ms: i64,
        ) -> Result<UniquePtr<ReplayMergeWrapper>>;

        fn doWork(self: Pin<&mut ReplayMergeWrapper>) -> Result<i32>;
        fn poll(
            self: Pin<&mut ReplayMergeWrapper>,
            fragment_limit: i32,
            handler: fn(usize, &[u8]),
            ctx: usize,
        ) -> Result<i32>;
        fn image(self: Pin<&mut ReplayMergeWrapper>) -> UniquePtr<ImageWrapper>;
        fn isMerged(self: &ReplayMergeWrapper) -> bool;
        fn hasFailed(self: &ReplayMergeWrapper) -> bool;
        fn isLiveAdded(self: &ReplayMergeWrapper) -> bool;
    }
}

use crate::Result;
use crate::callback::{self, Callback};
use std::marker::PhantomData;

/// Source location for recording — whether the stream being recorded originates
/// locally (via a spy subscription) or remotely (via network subscription).
#[repr(i32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceLocation {
    Local = 0,
    Remote = 1,
}

/// Describes a single recording in the Aeron Archive catalog.
#[derive(Debug, Clone)]
pub struct RecordingDescriptor {
    pub control_session_id: i64,
    pub correlation_id: i64,
    pub recording_id: i64,
    pub start_timestamp: i64,
    pub stop_timestamp: i64,
    pub start_position: i64,
    pub stop_position: i64,
    pub initial_term_id: i32,
    pub segment_file_length: i32,
    pub term_buffer_length: i32,
    pub mtu_length: i32,
    pub session_id: i32,
    pub stream_id: i32,
    pub stripped_channel: String,
    pub original_channel: String,
}

#[allow(clippy::too_many_arguments)]
fn recording_descriptor<F: FnMut(RecordingDescriptor)>(
    ctx: usize,
    control_session_id: i64,
    correlation_id: i64,
    recording_id: i64,
    start_timestamp: i64,
    stop_timestamp: i64,
    start_position: i64,
    stop_position: i64,
    initial_term_id: i32,
    segment_file_length: i32,
    term_buffer_length: i32,
    mtu_length: i32,
    session_id: i32,
    stream_id: i32,
    stripped_channel: &[u8],
    original_channel: &[u8],
) {
    // SAFETY: C++ only calls this with the ctx passed alongside it, during the call.
    let cb = unsafe { Callback::<F>::from_ctx(ctx) };
    cb.call((), |f| {
        f(RecordingDescriptor {
            control_session_id,
            correlation_id,
            recording_id,
            start_timestamp,
            stop_timestamp,
            start_position,
            stop_position,
            initial_term_id,
            segment_file_length,
            term_buffer_length,
            mtu_length,
            session_id,
            stream_id,
            stripped_channel: String::from_utf8_lossy(stripped_channel).into_owned(),
            original_channel: String::from_utf8_lossy(original_channel).into_owned(),
        })
    });
}

/// Safe Rust wrapper for the Aeron Archive client.
pub struct AeronArchive {
    inner: cxx::UniquePtr<ffi::ArchiveWrapper>,
}

impl AeronArchive {
    /// Connect to an Aeron Archive using the specified control channels.
    ///
    /// Default channels if using the Aeron Archive defaults:
    /// - Control request: `aeron:udp?endpoint=localhost:8010`
    /// - Control response: `aeron:udp?endpoint=localhost:0`
    pub fn connect(
        control_request_channel: &str,
        control_request_stream_id: i32,
        control_response_channel: &str,
        control_response_stream_id: i32,
    ) -> Result<Self> {
        let inner = ffi::connect_archive(
            control_request_channel,
            control_request_stream_id,
            control_response_channel,
            control_response_stream_id,
        )?;
        Ok(Self { inner })
    }

    /// Start recording a channel/stream. Returns the subscription ID.
    pub fn start_recording(
        &mut self,
        channel: &str,
        stream_id: i32,
        source: SourceLocation,
        auto_stop: bool,
    ) -> Result<i64> {
        Ok(self
            .inner
            .pin_mut()
            .startRecording(channel, stream_id, source as i32, auto_stop)?)
    }

    /// Stop a recording by subscription ID.
    pub fn stop_recording(&mut self, subscription_id: i64) -> Result<()> {
        self.inner.pin_mut().stopRecording(subscription_id)?;
        Ok(())
    }

    /// Stop recording a specific channel and stream.
    pub fn stop_recording_by_channel_and_stream(
        &mut self,
        channel: &str,
        stream_id: i32,
    ) -> Result<()> {
        self.inner
            .pin_mut()
            .stopRecordingByChannelAndStream(channel, stream_id)?;
        Ok(())
    }

    /// Get the current recording position for an active recording.
    pub fn get_recording_position(&mut self, recording_id: i64) -> Result<i64> {
        Ok(self.inner.pin_mut().getRecordingPosition(recording_id)?)
    }

    /// Get the start position of a recording.
    pub fn get_start_position(&mut self, recording_id: i64) -> Result<i64> {
        Ok(self.inner.pin_mut().getStartPosition(recording_id)?)
    }

    /// Get the stop position of a recording (NULL_POSITION if still active).
    pub fn get_stop_position(&mut self, recording_id: i64) -> Result<i64> {
        Ok(self.inner.pin_mut().getStopPosition(recording_id)?)
    }

    /// Get the max recorded position across all active recordings for a given recording ID.
    pub fn get_max_recorded_position(&mut self, recording_id: i64) -> Result<i64> {
        Ok(self.inner.pin_mut().getMaxRecordedPosition(recording_id)?)
    }

    /// List recordings starting from a given recording ID. The handler is called
    /// for each recording descriptor found. Returns the number of descriptors found.
    pub fn list_recordings<F>(
        &mut self,
        from_recording_id: i64,
        record_count: i32,
        handler: F,
    ) -> Result<i32>
    where
        F: FnMut(RecordingDescriptor),
    {
        let mut cb = Callback::new(handler);
        let result = self.inner.pin_mut().listRecordings(
            from_recording_id,
            record_count,
            recording_descriptor::<F>,
            cb.ctx(),
        );
        Ok(cb.finish(result)?)
    }

    /// List recordings matching a channel fragment and stream ID.
    pub fn list_recordings_for_uri<F>(
        &mut self,
        from_recording_id: i64,
        record_count: i32,
        channel_fragment: &str,
        stream_id: i32,
        handler: F,
    ) -> Result<i32>
    where
        F: FnMut(RecordingDescriptor),
    {
        let mut cb = Callback::new(handler);
        let result = self.inner.pin_mut().listRecordingsForUri(
            from_recording_id,
            record_count,
            channel_fragment,
            stream_id,
            recording_descriptor::<F>,
            cb.ctx(),
        );
        Ok(cb.finish(result)?)
    }

    /// Find the last recording matching the given criteria. Returns the recording ID
    /// or `aeron::NULL_VALUE` (-1) if not found.
    pub fn find_last_matching_recording(
        &mut self,
        min_recording_id: i64,
        channel_fragment: &str,
        stream_id: i32,
        session_id: i32,
    ) -> Result<i64> {
        Ok(self.inner.pin_mut().findLastMatchingRecording(
            min_recording_id,
            channel_fragment,
            stream_id,
            session_id,
        )?)
    }

    /// Start a replay of a recording. Returns the replay session ID.
    ///
    /// Use `NULL_POSITION` (i64::MIN) for position to replay from the start.
    /// Use `NULL_LENGTH` (i64::MIN) for length to replay the entire recording.
    pub fn start_replay(
        &mut self,
        recording_id: i64,
        replay_channel: &str,
        replay_stream_id: i32,
        position: i64,
        length: i64,
    ) -> Result<i64> {
        Ok(self.inner.pin_mut().startReplay(
            recording_id,
            replay_channel,
            replay_stream_id,
            position,
            length,
        )?)
    }

    /// Stop a replay by its session ID.
    pub fn stop_replay(&mut self, replay_session_id: i64) -> Result<()> {
        self.inner.pin_mut().stopReplay(replay_session_id)?;
        Ok(())
    }

    /// Stop all replays for a given recording ID.
    pub fn stop_all_replays(&mut self, recording_id: i64) -> Result<()> {
        self.inner.pin_mut().stopAllReplays(recording_id)?;
        Ok(())
    }

    /// Truncate a stopped recording to a given position.
    pub fn truncate_recording(&mut self, recording_id: i64, position: i64) -> Result<i64> {
        Ok(self
            .inner
            .pin_mut()
            .truncateRecording(recording_id, position)?)
    }

    /// Poll the archive for an error response. Returns the error message string
    /// (empty if no error).
    pub fn poll_for_error_response(&mut self) -> Result<String> {
        Ok(self.inner.pin_mut().pollForErrorResponse()?)
    }

    /// Check for an error response from the archive. Throws if an error is present.
    pub fn check_for_error_response(&mut self) -> Result<()> {
        self.inner.pin_mut().checkForErrorResponse()?;
        Ok(())
    }

    /// Get the archive ID for this connection.
    pub fn archive_id(&self) -> i64 {
        self.inner.archiveId()
    }

    /// Get the control session ID for this connection.
    pub fn control_session_id(&self) -> i64 {
        self.inner.controlSessionId()
    }
}

/// Sentinel value for null positions (same as `aeron::NULL_VALUE`).
pub const NULL_POSITION: i64 = i64::MIN;

/// Sentinel value for null length (replay entire recording).
pub const NULL_LENGTH: i64 = i64::MIN;

/// Default timeout for replay merge progress (10 seconds).
pub const REPLAY_MERGE_PROGRESS_TIMEOUT_DEFAULT_MS: i64 = 10_000;

/// Seamlessly merges an archived replay with a live stream for gap-fill scenarios.
///
/// ReplayMerge coordinates a replay from the archive with a live subscription,
/// transitioning through states: REPLAY → CATCHUP → ATTEMPT_LIVE_JOIN → MERGED.
/// UDP only — does not work with IPC channels.
///
/// Requires a subscription created with `control-mode=manual`.
///
/// The merge mutably borrows the subscription for its whole life: the C++
/// `ReplayMerge` polls the same subscription internally, so the subscription
/// cannot be moved to another thread while the merge is alive.
///
/// ```compile_fail,E0505
/// # use aeron_glide::{AeronClient, archive::{AeronArchive, ReplayMerge}};
/// # let client = AeronClient::new().unwrap();
/// # let mut archive = AeronArchive::connect("", 0, "", 0).unwrap();
/// let mut sub = client.add_subscription("aeron:udp?control-mode=manual", 1).unwrap();
/// let mut merge = ReplayMerge::new(&mut sub, &mut archive, "", "", "", 0, 0).unwrap();
/// std::thread::spawn(move || sub.poll(10, |_| {})); // error: `sub` is borrowed
/// merge.do_work().unwrap();
/// ```
pub struct ReplayMerge<'a> {
    inner: cxx::UniquePtr<ffi::ReplayMergeWrapper>,
    _subscription: PhantomData<&'a mut crate::Subscription>,
}

impl<'a> ReplayMerge<'a> {
    /// Create a new ReplayMerge.
    ///
    /// - `subscription`: Must use `control-mode=manual` in its channel URI.
    /// - `archive`: Connected archive client.
    /// - `replay_channel`: Channel for the replay stream (e.g., `"aeron:udp?endpoint=localhost:6666"`).
    /// - `replay_destination`: Destination for replay data on the subscription.
    /// - `live_destination`: Destination for live data on the subscription.
    /// - `recording_id`: The archive recording ID to replay from.
    /// - `start_position`: Position within the recording to start replay.
    pub fn new(
        subscription: &'a mut crate::Subscription,
        archive: &mut AeronArchive,
        replay_channel: &str,
        replay_destination: &str,
        live_destination: &str,
        recording_id: i64,
        start_position: i64,
    ) -> Result<Self> {
        let inner = ffi::create_replay_merge(
            subscription.inner_pin_mut(),
            archive.inner.pin_mut(),
            replay_channel,
            replay_destination,
            live_destination,
            recording_id,
            start_position,
            REPLAY_MERGE_PROGRESS_TIMEOUT_DEFAULT_MS,
        )?;
        Ok(Self {
            inner,
            _subscription: PhantomData,
        })
    }

    /// Drive the replay merge state machine. Call this regularly in your event loop.
    /// Returns the number of work items processed.
    pub fn do_work(&mut self) -> Result<i32> {
        Ok(self.inner.pin_mut().doWork()?)
    }

    /// Poll for fragments from the replay/merged stream.
    ///
    /// # Panics
    ///
    /// If `handler` panics, the panic is resumed once Aeron returns from the poll;
    /// the remaining fragments of this poll are consumed without being delivered.
    pub fn poll<F>(&mut self, fragment_limit: i32, handler: F) -> Result<i32>
    where
        F: FnMut(&[u8]),
    {
        let mut cb = Callback::new(handler);
        let result = self
            .inner
            .pin_mut()
            .poll(fragment_limit, callback::fragment::<F>, cb.ctx());
        Ok(cb.finish(result)?)
    }

    /// Get the merged Image, or `None` if it is not available yet. Available after
    /// `is_merged()` returns true; can also be used during the merge.
    ///
    /// The image borrows this `ReplayMerge`, so it cannot be polled while the image is alive.
    pub fn image(&mut self) -> Option<crate::Image<'_>> {
        crate::Image::from_raw(self.inner.pin_mut().image())
    }

    /// Returns true when the replay and live streams have been successfully merged.
    pub fn is_merged(&self) -> bool {
        self.inner.isMerged()
    }

    /// Returns true if the replay merge operation has failed.
    pub fn has_failed(&self) -> bool {
        self.inner.hasFailed()
    }

    /// Returns true if the live destination has been added to the subscription.
    pub fn is_live_added(&self) -> bool {
        self.inner.isLiveAdded()
    }
}
