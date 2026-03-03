#[cxx::bridge(namespace = "aeron_rs")]
pub mod ffi {
    unsafe extern "C++" {
        include!("shim.h");

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
        fn getRecordingPosition(
            self: Pin<&mut ArchiveWrapper>,
            recording_id: i64,
        ) -> Result<i64>;
        fn getStartPosition(self: Pin<&mut ArchiveWrapper>, recording_id: i64) -> Result<i64>;
        fn getStopPosition(self: Pin<&mut ArchiveWrapper>, recording_id: i64) -> Result<i64>;
        fn getMaxRecordedPosition(
            self: Pin<&mut ArchiveWrapper>,
            recording_id: i64,
        ) -> Result<i64>;

        // Listing
        fn listRecordings(
            self: Pin<&mut ArchiveWrapper>,
            from_recording_id: i64,
            record_count: i32,
            handler_id: usize,
        ) -> Result<i32>;
        fn listRecordingsForUri(
            self: Pin<&mut ArchiveWrapper>,
            from_recording_id: i64,
            record_count: i32,
            channel_fragment: &str,
            stream_id: i32,
            handler_id: usize,
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
    }

    extern "Rust" {
        #[allow(clippy::too_many_arguments)]
        fn handle_recording_descriptor(
            handler_id: usize,
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
            stripped_channel: String,
            original_channel: String,
        );
    }
}

use std::cell::RefCell;
use std::collections::HashMap;

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

type RecordingDescriptorHandlerMap =
    RefCell<HashMap<usize, *mut dyn FnMut(RecordingDescriptor)>>;

thread_local! {
    static RECORDING_DESCRIPTOR_HANDLERS: RecordingDescriptorHandlerMap = RefCell::new(HashMap::new());
}

#[allow(clippy::too_many_arguments)]
fn handle_recording_descriptor(
    handler_id: usize,
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
    stripped_channel: String,
    original_channel: String,
) {
    RECORDING_DESCRIPTOR_HANDLERS.with(|handlers| {
        if let Some(handler_ptr) = handlers.borrow_mut().get_mut(&handler_id) {
            let descriptor = RecordingDescriptor {
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
                stripped_channel,
                original_channel,
            };
            unsafe {
                let handler = &mut **handler_ptr;
                handler(descriptor);
            }
        }
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
    ) -> Result<Self, Box<dyn std::error::Error>> {
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
    ) -> Result<i64, Box<dyn std::error::Error>> {
        Ok(self
            .inner
            .pin_mut()
            .startRecording(channel, stream_id, source as i32, auto_stop)?)
    }

    /// Stop a recording by subscription ID.
    pub fn stop_recording(
        &mut self,
        subscription_id: i64,
    ) -> Result<(), Box<dyn std::error::Error>> {
        self.inner.pin_mut().stopRecording(subscription_id)?;
        Ok(())
    }

    /// Stop recording a specific channel and stream.
    pub fn stop_recording_by_channel_and_stream(
        &mut self,
        channel: &str,
        stream_id: i32,
    ) -> Result<(), Box<dyn std::error::Error>> {
        self.inner
            .pin_mut()
            .stopRecordingByChannelAndStream(channel, stream_id)?;
        Ok(())
    }

    /// Get the current recording position for an active recording.
    pub fn get_recording_position(
        &mut self,
        recording_id: i64,
    ) -> Result<i64, Box<dyn std::error::Error>> {
        Ok(self.inner.pin_mut().getRecordingPosition(recording_id)?)
    }

    /// Get the start position of a recording.
    pub fn get_start_position(
        &mut self,
        recording_id: i64,
    ) -> Result<i64, Box<dyn std::error::Error>> {
        Ok(self.inner.pin_mut().getStartPosition(recording_id)?)
    }

    /// Get the stop position of a recording (NULL_POSITION if still active).
    pub fn get_stop_position(
        &mut self,
        recording_id: i64,
    ) -> Result<i64, Box<dyn std::error::Error>> {
        Ok(self.inner.pin_mut().getStopPosition(recording_id)?)
    }

    /// Get the max recorded position across all active recordings for a given recording ID.
    pub fn get_max_recorded_position(
        &mut self,
        recording_id: i64,
    ) -> Result<i64, Box<dyn std::error::Error>> {
        Ok(self.inner.pin_mut().getMaxRecordedPosition(recording_id)?)
    }

    /// List recordings starting from a given recording ID. The handler is called
    /// for each recording descriptor found. Returns the number of descriptors found.
    pub fn list_recordings<F>(
        &mut self,
        from_recording_id: i64,
        record_count: i32,
        mut handler: F,
    ) -> Result<i32, Box<dyn std::error::Error>>
    where
        F: FnMut(RecordingDescriptor),
    {
        let handler_id = &handler as *const _ as usize;
        let mut_ptr: *mut (dyn FnMut(RecordingDescriptor) + 'static) = unsafe {
            std::mem::transmute::<
                *mut dyn FnMut(RecordingDescriptor),
                *mut (dyn FnMut(RecordingDescriptor) + 'static),
            >(&mut handler as *mut dyn FnMut(RecordingDescriptor))
        };

        RECORDING_DESCRIPTOR_HANDLERS.with(|handlers| {
            handlers.borrow_mut().insert(handler_id, mut_ptr);
        });

        let result = self
            .inner
            .pin_mut()
            .listRecordings(from_recording_id, record_count, handler_id);

        RECORDING_DESCRIPTOR_HANDLERS.with(|handlers| {
            handlers.borrow_mut().remove(&handler_id);
        });

        Ok(result?)
    }

    /// List recordings matching a channel fragment and stream ID.
    pub fn list_recordings_for_uri<F>(
        &mut self,
        from_recording_id: i64,
        record_count: i32,
        channel_fragment: &str,
        stream_id: i32,
        mut handler: F,
    ) -> Result<i32, Box<dyn std::error::Error>>
    where
        F: FnMut(RecordingDescriptor),
    {
        let handler_id = &handler as *const _ as usize;
        let mut_ptr: *mut (dyn FnMut(RecordingDescriptor) + 'static) = unsafe {
            std::mem::transmute::<
                *mut dyn FnMut(RecordingDescriptor),
                *mut (dyn FnMut(RecordingDescriptor) + 'static),
            >(&mut handler as *mut dyn FnMut(RecordingDescriptor))
        };

        RECORDING_DESCRIPTOR_HANDLERS.with(|handlers| {
            handlers.borrow_mut().insert(handler_id, mut_ptr);
        });

        let result = self.inner.pin_mut().listRecordingsForUri(
            from_recording_id,
            record_count,
            channel_fragment,
            stream_id,
            handler_id,
        );

        RECORDING_DESCRIPTOR_HANDLERS.with(|handlers| {
            handlers.borrow_mut().remove(&handler_id);
        });

        Ok(result?)
    }

    /// Find the last recording matching the given criteria. Returns the recording ID
    /// or `aeron::NULL_VALUE` (-1) if not found.
    pub fn find_last_matching_recording(
        &mut self,
        min_recording_id: i64,
        channel_fragment: &str,
        stream_id: i32,
        session_id: i32,
    ) -> Result<i64, Box<dyn std::error::Error>> {
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
    ) -> Result<i64, Box<dyn std::error::Error>> {
        Ok(self.inner.pin_mut().startReplay(
            recording_id,
            replay_channel,
            replay_stream_id,
            position,
            length,
        )?)
    }

    /// Stop a replay by its session ID.
    pub fn stop_replay(
        &mut self,
        replay_session_id: i64,
    ) -> Result<(), Box<dyn std::error::Error>> {
        self.inner.pin_mut().stopReplay(replay_session_id)?;
        Ok(())
    }

    /// Stop all replays for a given recording ID.
    pub fn stop_all_replays(
        &mut self,
        recording_id: i64,
    ) -> Result<(), Box<dyn std::error::Error>> {
        self.inner.pin_mut().stopAllReplays(recording_id)?;
        Ok(())
    }

    /// Truncate a stopped recording to a given position.
    pub fn truncate_recording(
        &mut self,
        recording_id: i64,
        position: i64,
    ) -> Result<i64, Box<dyn std::error::Error>> {
        Ok(self
            .inner
            .pin_mut()
            .truncateRecording(recording_id, position)?)
    }

    /// Poll the archive for an error response. Returns the error message string
    /// (empty if no error).
    pub fn poll_for_error_response(&mut self) -> Result<String, Box<dyn std::error::Error>> {
        Ok(self.inner.pin_mut().pollForErrorResponse()?)
    }

    /// Check for an error response from the archive. Throws if an error is present.
    pub fn check_for_error_response(&mut self) -> Result<(), Box<dyn std::error::Error>> {
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
