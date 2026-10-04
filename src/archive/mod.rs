//! Aeron Archive client (the `archive` feature): record streams, query the
//! catalog, replay, replicate and merge replays with live streams. Wraps the
//! C++ `aeron::archive::client` API.
//!
//! ```no_run
//! use aeron_glide::AeronClient;
//! use aeron_glide::archive::{self, ReplayParams, SourceLocation};
//!
//! # fn main() -> aeron_glide::Result<()> {
//! let client = AeronClient::new()?;
//! let archive = archive::Context::new()
//!     .aeron(&client)
//!     .control_request_channel("aeron:udp?endpoint=localhost:8010")
//!     .control_response_channel("aeron:udp?endpoint=localhost:0")
//!     .connect()?;
//!
//! let publication = archive.add_recorded_publication("aeron:ipc", 10)?;
//! // ... offer ...
//! let recording_id =
//!     archive.find_last_matching_recording(0, "aeron:ipc", 10, publication.session_id())?;
//! let mut replay = archive.replay(
//!     recording_id,
//!     "aeron:udp?endpoint=localhost:20123",
//!     11,
//!     &ReplayParams::new().position(0),
//! )?;
//! replay.poll(10, |data, _| println!("{} bytes", data.len()))?;
//! # Ok(())
//! # }
//! ```

mod client;
mod context;
mod persistent;
pub mod recording_pos;
mod replay_merge;
mod types;

pub use client::{AeronArchive, AsyncConnect};
pub use context::{Context, ContextInfo};
pub use persistent::{PersistentSubscription, PersistentSubscriptionBuilder};
pub use replay_merge::{REPLAY_MERGE_PROGRESS_TIMEOUT_DEFAULT, ReplayMerge};
pub use types::{
    ArchiveErrorCode, NULL_LENGTH, NULL_POSITION, REPLAY_ALL_AND_STOP, RecordingDescriptor,
    RecordingSignal, RecordingSignalCode, RecordingSubscriptionDescriptor, ReplayParams,
    ReplicationParams, SourceLocation,
};

#[allow(clippy::too_many_arguments)]
#[cxx::bridge(namespace = "aeron_rs")]
pub(crate) mod ffi {
    /// A recording descriptor (C++ `RecordingDescriptor`).
    struct RecordingDescriptorInfo {
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
        source_identity: String,
    }

    /// A recording subscription descriptor (C++ `RecordingSubscriptionDescriptor`).
    struct RecordingSubscriptionInfo {
        control_session_id: i64,
        correlation_id: i64,
        subscription_id: i64,
        stream_id: i32,
        stripped_channel: String,
    }

    /// A recording signal (C++ `RecordingSignal`).
    struct RecordingSignalInfo {
        control_session_id: i64,
        recording_id: i64,
        subscription_id: i64,
        position: i64,
        code: i32,
    }

    /// The settings of a connected archive client (C++ `AeronArchive::context()`).
    struct ArchiveContextInfo {
        aeron_directory_name: String,
        control_request_channel: String,
        control_request_stream_id: i32,
        control_response_channel: String,
        control_response_stream_id: i32,
        recording_events_channel: String,
        message_timeout_ns: i64,
        message_retry_attempts: u32,
        max_error_message_length: u32,
    }

    unsafe extern "C++" {
        include!("archive_shim.h");

        // Types from the client bridge (src/lib.rs).
        type AeronWrapper = crate::ffi::AeronWrapper;
        type SubscriptionWrapper = crate::ffi::SubscriptionWrapper;
        type PublicationWrapper = crate::ffi::PublicationWrapper;
        type ExclusivePublicationWrapper = crate::ffi::ExclusivePublicationWrapper;
        type ImageWrapper = crate::ffi::ImageWrapper;
        type CountersReaderWrapper = crate::ffi::CountersReaderWrapper;
        type CounterWrapper = crate::ffi::CounterWrapper;
        #[namespace = "aeron::concurrent::logbuffer"]
        type Header = crate::ffi::Header;

        type ArchiveContextWrapper;
        fn create_archive_context() -> Result<UniquePtr<ArchiveContextWrapper>>;
        fn setAeron(self: Pin<&mut ArchiveContextWrapper>, client: &AeronWrapper) -> Result<()>;
        fn setAeronDirectoryName(self: Pin<&mut ArchiveContextWrapper>, name: &str) -> Result<()>;
        fn setControlRequestChannel(
            self: Pin<&mut ArchiveContextWrapper>,
            channel: &str,
        ) -> Result<()>;
        fn setControlRequestStreamId(
            self: Pin<&mut ArchiveContextWrapper>,
            stream_id: i32,
        ) -> Result<()>;
        fn setControlResponseChannel(
            self: Pin<&mut ArchiveContextWrapper>,
            channel: &str,
        ) -> Result<()>;
        fn setControlResponseStreamId(
            self: Pin<&mut ArchiveContextWrapper>,
            stream_id: i32,
        ) -> Result<()>;
        fn setRecordingEventsChannel(
            self: Pin<&mut ArchiveContextWrapper>,
            channel: &str,
        ) -> Result<()>;
        fn setMessageTimeoutNs(
            self: Pin<&mut ArchiveContextWrapper>,
            timeout_ns: i64,
        ) -> Result<()>;
        fn setMessageRetryAttempts(
            self: Pin<&mut ArchiveContextWrapper>,
            attempts: u32,
        ) -> Result<()>;
        fn setMaxErrorMessageLength(
            self: Pin<&mut ArchiveContextWrapper>,
            length: u32,
        ) -> Result<()>;
        fn setIdleStrategy(
            self: Pin<&mut ArchiveContextWrapper>,
            idle: fn(usize, i32),
            release: fn(usize),
            ctx: usize,
        ) -> Result<()>;
        fn setDelegatingInvoker(
            self: Pin<&mut ArchiveContextWrapper>,
            invoker: fn(usize),
            release: fn(usize),
            ctx: usize,
        ) -> Result<()>;
        fn setErrorHandler(
            self: Pin<&mut ArchiveContextWrapper>,
            handler: fn(usize, &[u8]),
            release: fn(usize),
            ctx: usize,
        ) -> Result<()>;
        fn setRecordingSignalConsumer(
            self: Pin<&mut ArchiveContextWrapper>,
            consumer: fn(usize, &RecordingSignalInfo),
            release: fn(usize),
            ctx: usize,
        ) -> Result<()>;
        fn setCredentialsSupplier(
            self: Pin<&mut ArchiveContextWrapper>,
            credentials: fn(usize) -> Vec<u8>,
            on_challenge: fn(usize, &[u8]) -> Vec<u8>,
            release: fn(usize),
            ctx: usize,
        ) -> Result<()>;

        type ArchiveWrapper;
        fn archive_connect(
            context: Pin<&mut ArchiveContextWrapper>,
        ) -> Result<UniquePtr<ArchiveWrapper>>;

        type ArchiveAsyncConnectWrapper;
        fn archive_async_connect(
            context: UniquePtr<ArchiveContextWrapper>,
        ) -> Result<UniquePtr<ArchiveAsyncConnectWrapper>>;
        fn poll(self: Pin<&mut ArchiveAsyncConnectWrapper>) -> Result<UniquePtr<ArchiveWrapper>>;

        fn context(self: &ArchiveWrapper) -> Result<ArchiveContextInfo>;
        fn archiveId(self: &ArchiveWrapper) -> i64;
        fn controlSessionId(self: &ArchiveWrapper) -> i64;
        fn pollForRecordingSignals(self: &ArchiveWrapper) -> Result<i32>;
        fn pollForErrorResponse(self: &ArchiveWrapper) -> Result<String>;
        fn checkForErrorResponse(self: &ArchiveWrapper) -> Result<()>;

        fn addRecordedPublication(
            self: &ArchiveWrapper,
            channel: &str,
            stream_id: i32,
        ) -> Result<UniquePtr<PublicationWrapper>>;
        fn addRecordedExclusivePublication(
            self: &ArchiveWrapper,
            channel: &str,
            stream_id: i32,
        ) -> Result<UniquePtr<ExclusivePublicationWrapper>>;
        fn startRecording(
            self: &ArchiveWrapper,
            channel: &str,
            stream_id: i32,
            source_location: i32,
            auto_stop: bool,
        ) -> Result<i64>;
        fn extendRecording(
            self: &ArchiveWrapper,
            recording_id: i64,
            channel: &str,
            stream_id: i32,
            source_location: i32,
            auto_stop: bool,
        ) -> Result<i64>;
        fn stopRecording(self: &ArchiveWrapper, subscription_id: i64) -> Result<()>;
        fn tryStopRecording(self: &ArchiveWrapper, subscription_id: i64) -> Result<bool>;
        fn stopRecordingByChannelAndStream(
            self: &ArchiveWrapper,
            channel: &str,
            stream_id: i32,
        ) -> Result<()>;
        fn tryStopRecordingByChannelAndStream(
            self: &ArchiveWrapper,
            channel: &str,
            stream_id: i32,
        ) -> Result<bool>;
        fn tryStopRecordingByIdentity(self: &ArchiveWrapper, recording_id: i64) -> Result<bool>;
        fn stopRecordingPublication(
            self: &ArchiveWrapper,
            publication: &PublicationWrapper,
        ) -> Result<()>;
        fn stopRecordingExclusivePublication(
            self: &ArchiveWrapper,
            publication: &ExclusivePublicationWrapper,
        ) -> Result<()>;
        fn purgeRecording(self: &ArchiveWrapper, recording_id: i64) -> Result<i64>;
        fn updateChannel(self: &ArchiveWrapper, recording_id: i64, channel: &str) -> Result<()>;
        fn truncateRecording(
            self: &ArchiveWrapper,
            recording_id: i64,
            position: i64,
        ) -> Result<i64>;

        fn getRecordingPosition(self: &ArchiveWrapper, recording_id: i64) -> Result<i64>;
        fn getStartPosition(self: &ArchiveWrapper, recording_id: i64) -> Result<i64>;
        fn getStopPosition(self: &ArchiveWrapper, recording_id: i64) -> Result<i64>;
        fn getMaxRecordedPosition(self: &ArchiveWrapper, recording_id: i64) -> Result<i64>;
        fn findLastMatchingRecording(
            self: &ArchiveWrapper,
            min_recording_id: i64,
            channel_fragment: &str,
            stream_id: i32,
            session_id: i32,
        ) -> Result<i64>;
        fn listRecording(
            self: &ArchiveWrapper,
            recording_id: i64,
            handler: fn(usize, &RecordingDescriptorInfo),
            ctx: usize,
        ) -> Result<i32>;
        fn listRecordings(
            self: &ArchiveWrapper,
            from_recording_id: i64,
            record_count: i32,
            handler: fn(usize, &RecordingDescriptorInfo),
            ctx: usize,
        ) -> Result<i32>;
        fn listRecordingsForUri(
            self: &ArchiveWrapper,
            from_recording_id: i64,
            record_count: i32,
            channel_fragment: &str,
            stream_id: i32,
            handler: fn(usize, &RecordingDescriptorInfo),
            ctx: usize,
        ) -> Result<i32>;
        fn listRecordingSubscriptions(
            self: &ArchiveWrapper,
            pseudo_index: i32,
            subscription_count: i32,
            channel_fragment: &str,
            stream_id: i32,
            apply_stream_id: bool,
            handler: fn(usize, &RecordingSubscriptionInfo),
            ctx: usize,
        ) -> Result<i32>;

        fn startReplay(
            self: &ArchiveWrapper,
            recording_id: i64,
            channel: &str,
            stream_id: i32,
            position: i64,
            length: i64,
            bounding_limit_counter_id: i32,
            file_io_max_length: i32,
            replay_token: i64,
            subscription_registration_id: i64,
        ) -> Result<i64>;
        fn replay(
            self: &ArchiveWrapper,
            recording_id: i64,
            channel: &str,
            stream_id: i32,
            position: i64,
            length: i64,
            bounding_limit_counter_id: i32,
            file_io_max_length: i32,
            replay_token: i64,
            subscription_registration_id: i64,
        ) -> Result<UniquePtr<SubscriptionWrapper>>;
        fn stopReplay(self: &ArchiveWrapper, replay_session_id: i64) -> Result<()>;
        fn stopAllReplays(self: &ArchiveWrapper, recording_id: i64) -> Result<()>;

        fn replicate(
            self: &ArchiveWrapper,
            src_recording_id: i64,
            src_control_stream_id: i32,
            src_control_channel: &str,
            stop_position: i64,
            dst_recording_id: i64,
            live_destination: &str,
            replication_channel: &str,
            channel_tag_id: i64,
            subscription_tag_id: i64,
            file_io_max_length: i32,
            replication_session_id: i32,
            encoded_credentials: &[u8],
        ) -> Result<i64>;
        fn stopReplication(self: &ArchiveWrapper, replication_id: i64) -> Result<()>;
        fn tryStopReplication(self: &ArchiveWrapper, replication_id: i64) -> Result<bool>;

        fn detachSegments(
            self: &ArchiveWrapper,
            recording_id: i64,
            new_start_position: i64,
        ) -> Result<()>;
        fn deleteDetachedSegments(self: &ArchiveWrapper, recording_id: i64) -> Result<i64>;
        fn purgeSegments(
            self: &ArchiveWrapper,
            recording_id: i64,
            new_start_position: i64,
        ) -> Result<i64>;
        fn attachSegments(self: &ArchiveWrapper, recording_id: i64) -> Result<i64>;
        fn migrateSegments(
            self: &ArchiveWrapper,
            src_recording_id: i64,
            dst_recording_id: i64,
        ) -> Result<i64>;
        fn recordingPosFindCounterIdByRecordingId(
            reader: &CountersReaderWrapper,
            recording_id: i64,
        ) -> Result<i32>;
        fn recordingPosFindCounterIdBySessionId(
            reader: &CountersReaderWrapper,
            session_id: i32,
        ) -> Result<i32>;
        fn recordingPosGetRecordingId(
            reader: &CountersReaderWrapper,
            counter_id: i32,
        ) -> Result<i64>;
        fn recordingPosGetSourceIdentity(
            reader: &CountersReaderWrapper,
            counter_id: i32,
        ) -> Result<String>;
        fn recordingPosIsActive(
            reader: &CountersReaderWrapper,
            counter_id: i32,
            recording_id: i64,
        ) -> Result<bool>;

        type ReplayMergeWrapper;
        fn create_replay_merge(
            subscription: Pin<&mut SubscriptionWrapper>,
            archive: &ArchiveWrapper,
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
            handler: fn(usize, &[u8], &Header),
            ctx: usize,
        ) -> Result<i32>;
        fn image(self: Pin<&mut ReplayMergeWrapper>) -> UniquePtr<ImageWrapper>;
        fn isMerged(self: &ReplayMergeWrapper) -> bool;
        fn hasFailed(self: &ReplayMergeWrapper) -> bool;
        fn isLiveAdded(self: &ReplayMergeWrapper) -> bool;

        type PersistentSubscriptionContextWrapper;
        fn create_persistent_subscription_context()
        -> Result<UniquePtr<PersistentSubscriptionContextWrapper>>;
        fn setArchiveContext(
            self: Pin<&mut PersistentSubscriptionContextWrapper>,
            archive: UniquePtr<ArchiveContextWrapper>,
        ) -> Result<()>;
        fn setAeron(
            self: Pin<&mut PersistentSubscriptionContextWrapper>,
            client: &AeronWrapper,
        ) -> Result<()>;
        fn setAeronDirectoryName(
            self: Pin<&mut PersistentSubscriptionContextWrapper>,
            name: &str,
        ) -> Result<()>;
        fn setRecordingId(
            self: Pin<&mut PersistentSubscriptionContextWrapper>,
            recording_id: i64,
        ) -> Result<()>;
        fn setStartPosition(
            self: Pin<&mut PersistentSubscriptionContextWrapper>,
            position: i64,
        ) -> Result<()>;
        fn setLiveChannel(
            self: Pin<&mut PersistentSubscriptionContextWrapper>,
            channel: &str,
        ) -> Result<()>;
        fn setLiveStreamId(
            self: Pin<&mut PersistentSubscriptionContextWrapper>,
            stream_id: i32,
        ) -> Result<()>;
        fn setReplayChannel(
            self: Pin<&mut PersistentSubscriptionContextWrapper>,
            channel: &str,
        ) -> Result<()>;
        fn setReplayStreamId(
            self: Pin<&mut PersistentSubscriptionContextWrapper>,
            stream_id: i32,
        ) -> Result<()>;
        fn setCounter(
            self: Pin<&mut PersistentSubscriptionContextWrapper>,
            which: i32,
            counter: &CounterWrapper,
        ) -> Result<()>;
        fn setOnLiveJoined(
            self: Pin<&mut PersistentSubscriptionContextWrapper>,
            callback: fn(usize),
            release: fn(usize),
            ctx: usize,
        ) -> Result<()>;
        fn setOnLiveLeft(
            self: Pin<&mut PersistentSubscriptionContextWrapper>,
            callback: fn(usize),
            release: fn(usize),
            ctx: usize,
        ) -> Result<()>;
        fn setOnError(
            self: Pin<&mut PersistentSubscriptionContextWrapper>,
            callback: fn(usize, i32, &[u8]),
            release: fn(usize),
            ctx: usize,
        ) -> Result<()>;

        type PersistentSubscriptionWrapper;
        fn create_persistent_subscription(
            context: UniquePtr<PersistentSubscriptionContextWrapper>,
        ) -> Result<UniquePtr<PersistentSubscriptionWrapper>>;
        fn poll(
            self: Pin<&mut PersistentSubscriptionWrapper>,
            fragment_limit: i32,
            handler: fn(usize, &[u8], &Header),
            ctx: usize,
        ) -> Result<i32>;
        fn controlledPoll(
            self: Pin<&mut PersistentSubscriptionWrapper>,
            fragment_limit: i32,
            handler: fn(usize, &[u8], &Header) -> i32,
            ctx: usize,
        ) -> Result<i32>;
        fn isLive(self: &PersistentSubscriptionWrapper) -> bool;
        fn isReplaying(self: &PersistentSubscriptionWrapper) -> bool;
        fn hasFailed(self: &PersistentSubscriptionWrapper) -> bool;
        fn failureReason(self: &PersistentSubscriptionWrapper, code: &mut i32) -> String;
    }
}
