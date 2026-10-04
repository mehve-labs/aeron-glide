//! Aeron's data frame format (`aeron_data_header_t`, C++ `DataFrameHeader`):
//! the flags and header types of [`Header`](crate::Header) and
//! [`BufferClaim`](crate::BufferClaim), and the layout `offer_block` and
//! `block_poll` work with.

/// Length of a data frame header, in bytes.
pub const DATA_HEADER_LENGTH: usize = 32;

/// Frames start at multiples of this, in bytes.
pub const FRAME_ALIGNMENT: usize = 32;

/// Flag of the first fragment of a message.
pub const BEGIN_FLAG: u8 = 0x80;

/// Flag of the last fragment of a message.
pub const END_FLAG: u8 = 0x40;

/// Flags of a message in a single fragment ([`BEGIN_FLAG`] | [`END_FLAG`]).
pub const UNFRAGMENTED: u8 = BEGIN_FLAG | END_FLAG;

/// Flag of the end-of-stream frame.
pub const EOS_FLAG: u8 = 0x20;

/// Header type of a padding frame (skipped by subscribers).
pub const HDR_TYPE_PAD: u16 = 0x00;

/// Header type of a data frame.
pub const HDR_TYPE_DATA: u16 = 0x01;
