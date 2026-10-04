//! Finding the position counters of active recordings (C++
//! `archive::client::RecordingPos`), e.g. to follow how far a recording has
//! got without asking the archive.
//!
//! The archive's recording position counters are on its media driver: read them
//! through a client of that driver ([`AeronClient::counters_reader`](crate::AeronClient::counters_reader))
//! or its [`CncFile`](crate::CncFile).

use super::ffi;
use crate::error::Ffi;
use crate::{CountersReader, Result};

/// The ID of the position counter of an active recording, if any.
pub fn find_counter_id_by_recording_id(reader: &CountersReader, recording_id: i64) -> Option<i32> {
    ffi::recordingPosFindCounterIdByRecordingId(&reader.inner, recording_id)
        .ok()
        .filter(|&id| id >= 0)
}

/// The ID of the position counter of the active recording of a session (the
/// recorded stream's session ID), if any.
pub fn find_counter_id_by_session_id(reader: &CountersReader, session_id: i32) -> Option<i32> {
    ffi::recordingPosFindCounterIdBySessionId(&reader.inner, session_id)
        .ok()
        .filter(|&id| id >= 0)
}

/// The recording ID of a recording position counter, or `None` if the counter
/// is not an allocated recording position counter.
pub fn get_recording_id(reader: &CountersReader, counter_id: i32) -> Option<i64> {
    ffi::recordingPosGetRecordingId(&reader.inner, counter_id)
        .ok()
        .filter(|&id| id >= 0)
}

/// The source identity of a recording position counter's stream (e.g. a
/// socket address); empty if the counter is not an allocated recording
/// position counter. Fails for an out-of-range counter ID.
pub fn get_source_identity(reader: &CountersReader, counter_id: i32) -> Result<String> {
    ffi::recordingPosGetSourceIdentity(&reader.inner, counter_id).ffi()
}

/// Returns `true` if `counter_id` is still the position counter of the active
/// recording `recording_id`. Fails for an out-of-range counter ID.
pub fn is_active(reader: &CountersReader, counter_id: i32, recording_id: i64) -> Result<bool> {
    ffi::recordingPosIsActive(&reader.inner, counter_id, recording_id).ffi()
}
