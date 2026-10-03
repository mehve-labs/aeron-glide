//! Fragment headers ([`Header`]).

use super::*;

/// The header of a received fragment (C++ `logbuffer::Header`), passed to fragment
/// handlers alongside the payload. Valid only during the handler call.
///
/// For a message reassembled from several fragments (`poll_assembled`), the header
/// describes the message as a whole.
#[repr(transparent)]
pub struct Header(ffi::Header);

impl Header {
    pub(crate) fn from_ffi(header: &ffi::Header) -> &Header {
        // SAFETY: `Header` is a `repr(transparent)` wrapper around `ffi::Header`.
        unsafe { &*(header as *const ffi::Header as *const Header) }
    }

    /// The session ID of the publication that sent the fragment.
    pub fn session_id(&self) -> i32 {
        self.0.sessionId()
    }

    /// The stream ID of the fragment.
    pub fn stream_id(&self) -> i32 {
        self.0.streamId()
    }

    /// The term ID the fragment is in.
    pub fn term_id(&self) -> i32 {
        self.0.termId()
    }

    /// The offset of the fragment within its term.
    pub fn term_offset(&self) -> i32 {
        self.0.termOffset()
    }

    /// The initial term ID of the stream.
    pub fn initial_term_id(&self) -> i32 {
        self.0.initialTermId()
    }

    /// The stream position just after this fragment.
    pub fn position(&self) -> i64 {
        self.0.position()
    }

    /// The number of bits to shift a term ID by to get a stream position.
    pub fn position_bits_to_shift(&self) -> i32 {
        self.0.positionBitsToShift()
    }

    /// The length of the frame, header included.
    pub fn frame_length(&self) -> i32 {
        self.0.frameLength()
    }

    /// The frame type (e.g. data or padding).
    pub fn header_type(&self) -> u16 {
        self.0.headerType()
    }

    /// The frame flags (e.g. begin/end fragment, end of stream).
    pub fn flags(&self) -> u8 {
        self.0.flags()
    }

    /// The reserved value set by the publisher (e.g. with
    /// `offer_with_reserved_value` or `BufferClaim::set_reserved_value`).
    pub fn reserved_value(&self) -> i64 {
        self.0.reservedValue()
    }
}

impl std::fmt::Debug for Header {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Header")
            .field("session_id", &self.session_id())
            .field("stream_id", &self.stream_id())
            .field("term_id", &self.term_id())
            .field("term_offset", &self.term_offset())
            .field("position", &self.position())
            .field("frame_length", &self.frame_length())
            .field("flags", &self.flags())
            .field("reserved_value", &self.reserved_value())
            .finish()
    }
}
