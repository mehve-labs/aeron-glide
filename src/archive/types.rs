//! Archive value types: descriptors, signals, parameters and error codes.

use super::ffi;
use crate::Error;

/// The null position (C++ `NULL_POSITION`, `aeron::NULL_VALUE`), e.g. the stop
/// position of an active recording.
pub const NULL_POSITION: i64 = -1;

/// The null length (C++ `NULL_LENGTH`): replay to the end of the recording and
/// follow it while it is active (C `ARCHIVE_REPLAY_ALL_AND_FOLLOW`).
pub const NULL_LENGTH: i64 = -1;

/// A replay length that replays what is recorded when the replay starts, then
/// stops, even if the recording is active (C `ARCHIVE_REPLAY_ALL_AND_STOP`).
pub const REPLAY_ALL_AND_STOP: i64 = -2;

/// Where the recorded stream comes from, relative to the archive.
#[repr(i32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SourceLocation {
    /// The publication is on the archive's media driver: recorded with a spy
    /// subscription.
    Local = 0,
    /// The publication is remote: recorded with a network subscription.
    Remote = 1,
}

/// A recording in the archive's catalog (C++ `RecordingDescriptor`).
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct RecordingDescriptor {
    /// The control session that requested the listing.
    pub control_session_id: i64,
    /// The correlation ID of the listing request.
    pub correlation_id: i64,
    /// The recording ID.
    pub recording_id: i64,
    /// When the recording started, in milliseconds since the epoch.
    pub start_timestamp: i64,
    /// When the recording stopped (`NULL_TIMESTAMP`, -1, while active).
    pub stop_timestamp: i64,
    /// The stream position the recording starts at.
    pub start_position: i64,
    /// The stream position the recording stopped at ([`NULL_POSITION`] while
    /// active).
    pub stop_position: i64,
    /// The initial term ID of the recorded stream.
    pub initial_term_id: i32,
    /// The length of the recording's segment files.
    pub segment_file_length: i32,
    /// The term buffer length of the recorded stream.
    pub term_buffer_length: i32,
    /// The MTU of the recorded stream.
    pub mtu_length: i32,
    /// The session ID of the recorded stream.
    pub session_id: i32,
    /// The stream ID of the recorded stream.
    pub stream_id: i32,
    /// The channel the recording subscription used, without session-specific
    /// parameters.
    pub stripped_channel: String,
    /// The channel as given to start the recording.
    pub original_channel: String,
    /// The source of the recorded stream (e.g. `aeron:ipc` or a socket address).
    pub source_identity: String,
}

impl RecordingDescriptor {
    pub(crate) fn from_ffi(d: &ffi::RecordingDescriptorInfo) -> Self {
        Self {
            control_session_id: d.control_session_id,
            correlation_id: d.correlation_id,
            recording_id: d.recording_id,
            start_timestamp: d.start_timestamp,
            stop_timestamp: d.stop_timestamp,
            start_position: d.start_position,
            stop_position: d.stop_position,
            initial_term_id: d.initial_term_id,
            segment_file_length: d.segment_file_length,
            term_buffer_length: d.term_buffer_length,
            mtu_length: d.mtu_length,
            session_id: d.session_id,
            stream_id: d.stream_id,
            stripped_channel: d.stripped_channel.clone(),
            original_channel: d.original_channel.clone(),
            source_identity: d.source_identity.clone(),
        }
    }
}

/// An active recording subscription (C++ `RecordingSubscriptionDescriptor`).
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct RecordingSubscriptionDescriptor {
    /// The control session that requested the listing.
    pub control_session_id: i64,
    /// The correlation ID of the listing request.
    pub correlation_id: i64,
    /// The recording subscription's ID (as returned by `start_recording`).
    pub subscription_id: i64,
    /// The stream ID it records.
    pub stream_id: i32,
    /// The channel it records, without session-specific parameters.
    pub stripped_channel: String,
}

impl RecordingSubscriptionDescriptor {
    pub(crate) fn from_ffi(d: &ffi::RecordingSubscriptionInfo) -> Self {
        Self {
            control_session_id: d.control_session_id,
            correlation_id: d.correlation_id,
            subscription_id: d.subscription_id,
            stream_id: d.stream_id,
            stripped_channel: d.stripped_channel.clone(),
        }
    }
}

/// What happened to a recording (C++ `RecordingSignal::Value`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum RecordingSignalCode {
    /// A recording started.
    Start,
    /// A recording stopped.
    Stop,
    /// A recording was extended.
    Extend,
    /// A recording is being replicated.
    Replicate,
    /// A replication merged with the live stream.
    Merge,
    /// A replication caught up with its source.
    Sync,
    /// A recording, or some of its segments, was deleted.
    Delete,
    /// A replication ended.
    ReplicateEnd,
    /// A code this version does not know.
    Unknown(i32),
}

impl RecordingSignalCode {
    /// The code for a C++ `RecordingSignal::Value`.
    pub fn from_code(code: i32) -> Self {
        match code {
            0 => Self::Start,
            1 => Self::Stop,
            2 => Self::Extend,
            3 => Self::Replicate,
            4 => Self::Merge,
            5 => Self::Sync,
            6 => Self::Delete,
            7 => Self::ReplicateEnd,
            other => Self::Unknown(other),
        }
    }
}

/// A signal about a recording (C++ `RecordingSignal`), delivered to the
/// [`Context::recording_signal_consumer`](super::Context::recording_signal_consumer)
/// while the archive client polls for responses (or by
/// [`AeronArchive::poll_for_recording_signals`](super::AeronArchive::poll_for_recording_signals)).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct RecordingSignal {
    /// The control session the signal is for.
    pub control_session_id: i64,
    /// The recording it is about.
    pub recording_id: i64,
    /// The recording subscription, if any.
    pub subscription_id: i64,
    /// The recording position at the signal.
    pub position: i64,
    /// What happened.
    pub signal: RecordingSignalCode,
}

impl RecordingSignal {
    pub(crate) fn from_ffi(s: &ffi::RecordingSignalInfo) -> Self {
        Self {
            control_session_id: s.control_session_id,
            recording_id: s.recording_id,
            subscription_id: s.subscription_id,
            position: s.position,
            signal: RecordingSignalCode::from_code(s.code),
        }
    }
}

/// Parameters of a replay (C++ `ReplayParams`). The defaults replay the whole
/// recording from its start, following it while it is active.
///
/// ```
/// use aeron_glide::archive::ReplayParams;
///
/// let params = ReplayParams::new().position(4096).length(1 << 20);
/// assert!(!params.is_bounded());
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReplayParams {
    pub(crate) position: i64,
    pub(crate) length: i64,
    pub(crate) bounding_limit_counter_id: i32,
    pub(crate) file_io_max_length: i32,
    pub(crate) replay_token: i64,
    pub(crate) subscription_registration_id: i64,
}

impl Default for ReplayParams {
    fn default() -> Self {
        // aeron_archive_replay_params_init
        Self {
            position: NULL_POSITION,
            length: NULL_LENGTH,
            bounding_limit_counter_id: -1,
            file_io_max_length: -1,
            replay_token: -1,
            subscription_registration_id: -1,
        }
    }
}

impl ReplayParams {
    /// The default parameters.
    pub fn new() -> Self {
        Self::default()
    }

    /// Where to start ([`NULL_POSITION`], the default: the recording's start).
    pub fn position(mut self, position: i64) -> Self {
        self.position = position;
        self
    }

    /// How much to replay ([`NULL_LENGTH`], the default: to the end, following
    /// an active recording; [`REPLAY_ALL_AND_STOP`] to stop at the recorded end;
    /// `i64::MAX` to follow it forever).
    pub fn length(mut self, length: i64) -> Self {
        self.length = length;
        self
    }

    /// Bound the replay by a counter's value, e.g. a position counter: the replay
    /// stops at the counter's position (-1, the default: unbounded).
    pub fn bounding_limit_counter_id(mut self, counter_id: i32) -> Self {
        self.bounding_limit_counter_id = counter_id;
        self
    }

    /// The maximum length of each file read (-1, the default: the archive's
    /// setting).
    pub fn file_io_max_length(mut self, length: i32) -> Self {
        self.file_io_max_length = length;
        self
    }

    /// A token from the archive authorising the replay, for replays requested
    /// without a control session.
    pub fn replay_token(mut self, token: i64) -> Self {
        self.replay_token = token;
        self
    }

    /// The registration ID of the subscription to replay to, for a response
    /// channel replay started with
    /// [`AeronArchive::start_replay`](super::AeronArchive::start_replay) (which
    /// the C archive client of Aeron 1.53.3 fails to set up, see
    /// [`AeronArchive::replay`](super::AeronArchive::replay)).
    pub fn subscription_registration_id(mut self, registration_id: i64) -> Self {
        self.subscription_registration_id = registration_id;
        self
    }

    /// Returns `true` if the replay is bounded by a counter.
    pub fn is_bounded(&self) -> bool {
        self.bounding_limit_counter_id != -1
    }

    /// See [`position`](Self::position).
    pub fn get_position(&self) -> i64 {
        self.position
    }

    /// See [`length`](Self::length).
    pub fn get_length(&self) -> i64 {
        self.length
    }

    /// See [`bounding_limit_counter_id`](Self::bounding_limit_counter_id).
    pub fn get_bounding_limit_counter_id(&self) -> i32 {
        self.bounding_limit_counter_id
    }

    /// See [`file_io_max_length`](Self::file_io_max_length).
    pub fn get_file_io_max_length(&self) -> i32 {
        self.file_io_max_length
    }

    /// See [`replay_token`](Self::replay_token).
    pub fn get_replay_token(&self) -> i64 {
        self.replay_token
    }

    /// See [`subscription_registration_id`](Self::subscription_registration_id).
    pub fn get_subscription_registration_id(&self) -> i64 {
        self.subscription_registration_id
    }
}

/// Parameters of a replication (C++ `ReplicationParams`). The defaults
/// replicate the whole recording into a new one and stop.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReplicationParams {
    pub(crate) stop_position: i64,
    pub(crate) dst_recording_id: i64,
    pub(crate) live_destination: String,
    pub(crate) replication_channel: String,
    pub(crate) channel_tag_id: i64,
    pub(crate) subscription_tag_id: i64,
    pub(crate) file_io_max_length: i32,
    pub(crate) replication_session_id: i32,
    pub(crate) encoded_credentials: Vec<u8>,
}

impl Default for ReplicationParams {
    fn default() -> Self {
        // aeron_archive_replication_params_init
        Self {
            stop_position: NULL_POSITION,
            dst_recording_id: -1,
            live_destination: String::new(),
            replication_channel: String::new(),
            channel_tag_id: -1,
            subscription_tag_id: -1,
            file_io_max_length: -1,
            replication_session_id: -1,
            encoded_credentials: Vec::new(),
        }
    }
}

impl ReplicationParams {
    /// The default parameters.
    pub fn new() -> Self {
        Self::default()
    }

    /// Stop at this position ([`NULL_POSITION`], the default: replicate
    /// everything and, if the source is active, keep following it).
    pub fn stop_position(mut self, position: i64) -> Self {
        self.stop_position = position;
        self
    }

    /// Extend this existing recording instead of creating a new one (-1, the
    /// default).
    pub fn dst_recording_id(mut self, recording_id: i64) -> Self {
        self.dst_recording_id = recording_id;
        self
    }

    /// Merge with the live stream at this destination once caught up (empty,
    /// the default: don't).
    pub fn live_destination(mut self, destination: &str) -> Self {
        self.live_destination = destination.to_string();
        self
    }

    /// The channel to replicate over (empty, the default: the archive's
    /// replication channel).
    pub fn replication_channel(mut self, channel: &str) -> Self {
        self.replication_channel = channel.to_string();
        self
    }

    /// The tag of the replication channel (-1, the default: none).
    pub fn channel_tag_id(mut self, tag: i64) -> Self {
        self.channel_tag_id = tag;
        self
    }

    /// The tag of the replication subscription (-1, the default: none).
    pub fn subscription_tag_id(mut self, tag: i64) -> Self {
        self.subscription_tag_id = tag;
        self
    }

    /// The maximum length of each file read (-1, the default: the archive's
    /// setting).
    pub fn file_io_max_length(mut self, length: i32) -> Self {
        self.file_io_max_length = length;
        self
    }

    /// The session ID of the replicated stream (-1, the default: the archive
    /// picks one).
    pub fn replication_session_id(mut self, session_id: i32) -> Self {
        self.replication_session_id = session_id;
        self
    }

    /// Credentials for the source archive, if it authenticates.
    pub fn encoded_credentials(mut self, credentials: &[u8]) -> Self {
        self.encoded_credentials = credentials.to_vec();
        self
    }

    /// See [`stop_position`](Self::stop_position).
    pub fn get_stop_position(&self) -> i64 {
        self.stop_position
    }

    /// See [`dst_recording_id`](Self::dst_recording_id).
    pub fn get_dst_recording_id(&self) -> i64 {
        self.dst_recording_id
    }

    /// See [`live_destination`](Self::live_destination).
    pub fn get_live_destination(&self) -> &str {
        &self.live_destination
    }

    /// See [`replication_channel`](Self::replication_channel).
    pub fn get_replication_channel(&self) -> &str {
        &self.replication_channel
    }

    /// See [`channel_tag_id`](Self::channel_tag_id).
    pub fn get_channel_tag_id(&self) -> i64 {
        self.channel_tag_id
    }

    /// See [`subscription_tag_id`](Self::subscription_tag_id).
    pub fn get_subscription_tag_id(&self) -> i64 {
        self.subscription_tag_id
    }

    /// See [`file_io_max_length`](Self::file_io_max_length).
    pub fn get_file_io_max_length(&self) -> i32 {
        self.file_io_max_length
    }

    /// See [`replication_session_id`](Self::replication_session_id).
    pub fn get_replication_session_id(&self) -> i32 {
        self.replication_session_id
    }

    /// See [`encoded_credentials`](Self::encoded_credentials).
    pub fn get_encoded_credentials(&self) -> &[u8] {
        &self.encoded_credentials
    }
}

/// The error codes an archive returns (C++ `archive::client::error`).
///
/// An archive's error response fails a request with an [`Error`] whose
/// [`code`](Error::code) is the negated [`code`](Self::code) (e.g. -205 for
/// [`UnknownRecording`](Self::UnknownRecording)); errors passed to an error
/// handler, or a persistent subscription's `on_error`, carry it positive.
/// [`of`](Self::of) recovers it from both. (A listing request rejected by the
/// archive reports the archive's raw code, 0 to 16, which `of` does not
/// recognise.)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum ArchiveErrorCode {
    /// A generic error.
    Generic,
    /// A listing is already active for the session.
    ActiveListing,
    /// The recording is active.
    ActiveRecording,
    /// The subscription is already recording.
    ActiveSubscription,
    /// No such recording subscription.
    UnknownSubscription,
    /// No such recording.
    UnknownRecording,
    /// No such replay.
    UnknownReplay,
    /// Too many concurrent replays.
    MaxReplays,
    /// Too many concurrent recordings.
    MaxRecordings,
    /// The extension does not match the recording.
    InvalidExtension,
    /// Authentication was rejected.
    AuthenticationRejected,
    /// Not enough storage space.
    StorageSpace,
    /// No such replication.
    UnknownReplication,
    /// The action is not authorised.
    UnauthorisedAction,
    /// The replication could not connect to the source archive.
    ReplicationConnectionFailure,
    /// The recording is empty.
    EmptyRecording,
    /// The position is not valid for the recording.
    InvalidPosition,
}

impl ArchiveErrorCode {
    const ALL: [Self; 17] = [
        Self::Generic,
        Self::ActiveListing,
        Self::ActiveRecording,
        Self::ActiveSubscription,
        Self::UnknownSubscription,
        Self::UnknownRecording,
        Self::UnknownReplay,
        Self::MaxReplays,
        Self::MaxRecordings,
        Self::InvalidExtension,
        Self::AuthenticationRejected,
        Self::StorageSpace,
        Self::UnknownReplication,
        Self::UnauthorisedAction,
        Self::ReplicationConnectionFailure,
        Self::EmptyRecording,
        Self::InvalidPosition,
    ];

    /// The C++ constant (`error::GENERIC` = 200, ..., `error::INVALID_POSITION` = 216).
    pub fn code(self) -> i32 {
        200 + Self::ALL.iter().position(|&c| c == self).unwrap_or(0) as i32
    }

    /// The archive error code for a C++ constant.
    pub fn from_code(code: i32) -> Option<Self> {
        let index = usize::try_from(code.checked_sub(200)?).ok()?;
        Self::ALL.get(index).copied()
    }

    /// The archive error code of a failed request, if the archive rejected it.
    pub fn of(error: &Error) -> Option<Self> {
        Self::from_code(error.code().checked_abs()?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ErrorKind;

    #[test]
    fn error_code_conventions() {
        let error = |kind, code| Error::new(kind, "x").with_code(code);
        // Request errors carry the code negated, handler errors positive.
        for e in [
            error(ErrorKind::Archive, -205),
            error(ErrorKind::Aeron, 205),
        ] {
            assert_eq!(
                ArchiveErrorCode::of(&e),
                Some(ArchiveErrorCode::UnknownRecording)
            );
        }
        assert_eq!(
            ArchiveErrorCode::of(&error(ErrorKind::IllegalState, 5)),
            None
        );
        assert_eq!(
            ArchiveErrorCode::of(&error(ErrorKind::Aeron, i32::MIN)),
            None
        );
        for code in 200..=216 {
            assert_eq!(ArchiveErrorCode::from_code(code).unwrap().code(), code);
        }
    }

    #[test]
    fn segment_file_base_position_matches_c() {
        use super::super::AeronArchive;
        assert_eq!(
            AeronArchive::segment_file_base_position(0, 300_000, 65536, 262144),
            262144
        );
        assert_eq!(
            AeronArchive::segment_file_base_position(65536 + 32, 65536 + 300_000, 65536, 262144),
            65536 + 262144
        );
        // Invalid lengths do not overflow.
        let _ = AeronArchive::segment_file_base_position(i64::MIN, i64::MAX, i32::MIN, i32::MIN);
    }
}
