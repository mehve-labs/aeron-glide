//! Error type shared by the whole crate.
//!
//! C++ exceptions raised by the Aeron C++ wrapper (and by the shim around the C
//! media driver) are classified on the C++ side by `rust::behavior::trycatch`
//! in `shim.h`, which encodes the exception class and Aeron error code into the
//! message. [`Error::from`] decodes it back into an [`ErrorKind`].

use std::fmt;

/// Separator used by the C++ shim to encode `kind`, `code` and `message`.
const SEP: char = '\u{1e}';
/// Prefix marking a message encoded by the shim.
const MAGIC: &str = "aeron-glide";

/// Result type used throughout the crate.
pub type Result<T, E = Error> = std::result::Result<T, E>;

/// The category of an [`Error`], from the Aeron C++ exception hierarchy (kinds
/// Aeron's C++ wrapper never raises over its C client are left out). Match on
/// [`Error::code`] for finer detail; [`Error::is_timeout`] covers every kind
/// of timeout.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum ErrorKind {
    /// An argument was rejected, e.g. a message longer than the maximum message
    /// length or an out-of-range counter id.
    IllegalArgument,
    /// The operation is not valid in the current state, e.g. a closed resource.
    IllegalState,
    /// An I/O error, e.g. a missing or unreadable Aeron directory.
    Io,
    /// An index was out of range in Aeron's C++ code (`std::out_of_range`).
    OutOfBounds,
    /// Input could not be parsed.
    Parse,
    /// The media driver did not respond in time, or shut down. Fatal for the client.
    DriverTimeout,
    /// The client conductor service was not invoked in time. Fatal for the client.
    ConductorServiceTimeout,
    /// The media driver timed this client out. Fatal for the client.
    ClientTimeout,
    /// A generic timeout, e.g. the media driver not responding to a synchronous
    /// add. Archive timeouts are reported as [`Archive`](Self::Archive).
    Timeout,
    /// The media driver rejected an add (publication, subscription, counter or
    /// destination): an invalid channel, an unknown host, a bad term length,
    /// ... [`Error::code`] is the driver's (negative) error code.
    Registration,
    /// A callback re-entered the client in an unsupported way, e.g. a nested
    /// assembled poll on the same subscription.
    Reentrant,
    /// The operation is not supported, e.g. a writable counter handle on a
    /// counters reader of a (read-only) CnC file.
    UnsupportedOperation,
    /// An [`Agent`](crate::concurrent::Agent) asks to stop (C++
    /// `AgentTerminationException`); see [`Error::agent_termination`].
    AgentTermination,
    /// An Archive error, including archive connect and request timeouts.
    Archive,
    /// A general Aeron error without a more specific category.
    Aeron,
    /// Any other error, e.g. a non-Aeron C++ exception.
    Other,
}

impl ErrorKind {
    fn from_token(token: &str) -> Self {
        match token {
            "illegal_argument" => Self::IllegalArgument,
            "illegal_state" => Self::IllegalState,
            "io" => Self::Io,
            "out_of_bounds" => Self::OutOfBounds,
            "parse" => Self::Parse,
            "driver_timeout" => Self::DriverTimeout,
            "conductor_service_timeout" => Self::ConductorServiceTimeout,
            "client_timeout" => Self::ClientTimeout,
            "timeout" => Self::Timeout,
            "registration" => Self::Registration,
            "reentrant" => Self::Reentrant,
            "unsupported_operation" => Self::UnsupportedOperation,
            "archive" => Self::Archive,
            "aeron" => Self::Aeron,
            _ => Self::Other,
        }
    }
}

/// An error returned by an Aeron operation.
///
/// `Error` is `Send + Sync + 'static`, so it works with `?` into `anyhow`,
/// `Box<dyn Error + Send + Sync>` and across threads.
#[derive(Clone, PartialEq, Eq)]
pub struct Error {
    kind: ErrorKind,
    code: i32,
    message: String,
}

impl Error {
    /// Create an error with the given kind and message, and no Aeron error code.
    pub fn new(kind: ErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            code: 0,
            message: message.into(),
        }
    }

    /// The error an [`Agent`](crate::concurrent::Agent) returns to stop its
    /// runner or invoker (kind [`ErrorKind::AgentTermination`]).
    pub fn agent_termination() -> Self {
        Self::new(ErrorKind::AgentTermination, "agent terminated")
    }

    /// The same error with an Aeron error code.
    pub(crate) fn with_code(mut self, code: i32) -> Self {
        self.code = code;
        self
    }

    /// The category of this error.
    pub fn kind(&self) -> ErrorKind {
        self.kind
    }

    /// The Aeron error code (`aeron_errcode()` / `SourcedException::errorCode()`),
    /// or `0` if none was reported.
    ///
    /// Positive values are `errno` codes (e.g. `22`, `EINVAL`); negative values are
    /// Aeron error codes (`AERON_ERROR_CODE_*` / `AERON_CLIENT_ERROR_*` in Aeron's
    /// C headers), e.g. for a registration rejected by the media driver.
    pub fn code(&self) -> i32 {
        self.code
    }

    /// The error message reported by Aeron.
    pub fn message(&self) -> &str {
        &self.message
    }

    /// Returns `true` for errors after which the Aeron client is unusable
    /// (driver, client or conductor service timeouts).
    pub fn is_fatal(&self) -> bool {
        matches!(
            self.kind,
            ErrorKind::DriverTimeout
                | ErrorKind::ClientTimeout
                | ErrorKind::ConductorServiceTimeout
        )
    }

    /// Returns `true` for timeouts of any kind: the media driver, client or
    /// conductor timeouts, a generic [`Timeout`](ErrorKind::Timeout), and
    /// archive requests that timed out (reported as
    /// [`Archive`](ErrorKind::Archive) with the platform's `ETIMEDOUT`).
    pub fn is_timeout(&self) -> bool {
        match self.kind {
            ErrorKind::Timeout
            | ErrorKind::DriverTimeout
            | ErrorKind::ClientTimeout
            | ErrorKind::ConductorServiceTimeout => true,
            ErrorKind::Archive => {
                std::io::Error::from_raw_os_error(self.code).kind() == std::io::ErrorKind::TimedOut
            }
            _ => false,
        }
    }

    /// The same error, classified as a registration the media driver rejected
    /// (from a generic Aeron error).
    pub(crate) fn into_registration(mut self) -> Self {
        if self.kind == ErrorKind::Aeron {
            self.kind = ErrorKind::Registration;
        }
        self
    }

    /// Decode a message encoded by the C++ shim (see `encode_exception` in shim.h).
    pub(crate) fn from_encoded(what: &str) -> Self {
        Self::decode(what)
    }

    fn decode(what: &str) -> Self {
        let mut parts = what.splitn(4, SEP);
        if let (Some(MAGIC), Some(kind), Some(code), Some(message)) =
            (parts.next(), parts.next(), parts.next(), parts.next())
        {
            return Self {
                kind: ErrorKind::from_token(kind),
                code: code.parse().unwrap_or(0),
                message: message.to_string(),
            };
        }
        Self::new(ErrorKind::Other, what)
    }
}

impl Error {
    /// An error raised by the C++ shim (encoded by `encode_exception`).
    pub(crate) fn from_cxx(e: cxx::Exception) -> Self {
        Self::decode(e.what())
    }
}

/// Converts the result of a call into C++ to this crate's [`Result`]. Not a
/// `From` impl: that would make `cxx`, an implementation detail, part of the
/// public API.
pub(crate) trait Ffi<T> {
    fn ffi(self) -> Result<T>;
}

impl<T> Ffi<T> for std::result::Result<T, cxx::Exception> {
    fn ffi(self) -> Result<T> {
        self.map_err(Error::from_cxx)
    }
}

impl fmt::Debug for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Error")
            .field("kind", &self.kind)
            .field("code", &self.code)
            .field("message", &self.message)
            .finish()
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}: {}", self.kind, self.message.trim_end())?;
        // Aeron's messages usually start with "(<code>) ": don't repeat it.
        if self.code != 0 && !self.message.starts_with(&format!("({}) ", self.code)) {
            write!(f, " (code {})", self.code)?;
        }
        Ok(())
    }
}

impl std::error::Error for Error {}

/// Why an `offer` or `try_claim` did not publish.
///
/// The first three variants are transient: retrying (after idling, or once a
/// subscriber connects) can succeed. See [`OfferError::is_retryable`].
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum OfferError {
    /// No subscriber is connected (`NOT_CONNECTED`, -1).
    NotConnected,
    /// Flow control or a full term is applying back pressure (`BACK_PRESSURED`, -2).
    BackPressured,
    /// An administrative action such as a term rotation is in progress (`ADMIN_ACTION`, -3).
    AdminAction,
    /// The publication is closed (`PUBLICATION_CLOSED`, -4).
    Closed,
    /// The maximum stream position was reached; a new publication is required
    /// (`MAX_POSITION_EXCEEDED`, -5).
    MaxPositionExceeded,
    /// Aeron raised an error, e.g. a message longer than the maximum message length.
    Error(Error),
}

impl OfferError {
    /// Map a negative offer / try_claim result to an `OfferError`.
    pub(crate) fn from_position(position: i64) -> Self {
        match position {
            -1 => Self::NotConnected,
            -2 => Self::BackPressured,
            -3 => Self::AdminAction,
            -4 => Self::Closed,
            -5 => Self::MaxPositionExceeded,
            other => Self::Error(Error::new(
                ErrorKind::Aeron,
                format!("unexpected offer result {other}"),
            )),
        }
    }

    /// Returns `true` if retrying the same offer can succeed:
    /// [`NotConnected`](Self::NotConnected), [`BackPressured`](Self::BackPressured)
    /// or [`AdminAction`](Self::AdminAction).
    pub fn is_retryable(&self) -> bool {
        matches!(
            self,
            Self::NotConnected | Self::BackPressured | Self::AdminAction
        )
    }

    /// Returns `true` for [`BackPressured`](Self::BackPressured).
    pub fn is_back_pressured(&self) -> bool {
        matches!(self, Self::BackPressured)
    }
}

impl From<Error> for OfferError {
    fn from(e: Error) -> Self {
        Self::Error(e)
    }
}

impl fmt::Display for OfferError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotConnected => f.write_str("publication is not connected"),
            Self::BackPressured => f.write_str("publication is back pressured"),
            Self::AdminAction => f.write_str("publication admin action in progress"),
            Self::Closed => f.write_str("publication is closed"),
            Self::MaxPositionExceeded => f.write_str("publication max position exceeded"),
            Self::Error(e) => write!(f, "offer failed: {e}"),
        }
    }
}

/// For `?` in functions returning [`Result`]: an Aeron error is returned as is,
/// the publication states as [`ErrorKind::IllegalState`] with the offer result
/// (e.g. -2 for back pressure) as the [`code`](Error::code).
impl From<OfferError> for Error {
    fn from(e: OfferError) -> Self {
        let code = match &e {
            OfferError::Error(_) => 0,
            OfferError::NotConnected => -1,
            OfferError::BackPressured => -2,
            OfferError::AdminAction => -3,
            OfferError::Closed => -4,
            OfferError::MaxPositionExceeded => -5,
        };
        match e {
            OfferError::Error(e) => e,
            other => Error::new(ErrorKind::IllegalState, other.to_string()).with_code(code),
        }
    }
}

impl std::error::Error for OfferError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Error(e) => Some(e),
            _ => None,
        }
    }
}

/// A path as the UTF-8 string Aeron's C API takes.
pub(crate) fn path_str(path: &std::path::Path) -> Result<&str> {
    path.to_str().ok_or_else(|| {
        Error::new(
            ErrorKind::IllegalArgument,
            format!("{} is not valid UTF-8", path.display()),
        )
    })
}

/// A poll limit for Aeron, which takes an `i32`: larger limits are capped.
pub(crate) fn ffi_limit(limit: usize) -> i32 {
    i32::try_from(limit).unwrap_or(i32::MAX)
}

/// A count Aeron returns (never negative: errors are raised as exceptions).
pub(crate) fn count<T: TryInto<usize>>(count: T) -> usize {
    count.try_into().unwrap_or(0)
}

/// Convert a raw offer / try_claim position into a `Result`.
pub(crate) fn offer_result(position: i64) -> Result<i64, OfferError> {
    if position >= 0 {
        Ok(position)
    } else {
        Err(OfferError::from_position(position))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn offer_errors_convert_to_errors() {
        let e = Error::from(OfferError::BackPressured);
        assert_eq!((e.kind(), e.code()), (ErrorKind::IllegalState, -2));
        let inner = Error::new(ErrorKind::IllegalArgument, "too long");
        assert_eq!(Error::from(OfferError::Error(inner.clone())), inner);
    }

    #[test]
    fn decodes_shim_encoding() {
        let e = Error::decode("aeron-glide\u{1e}illegal_argument\u{1e}22\u{1e}bad length");
        assert_eq!(e.kind(), ErrorKind::IllegalArgument);
        assert_eq!(e.code(), 22);
        assert_eq!(e.message(), "bad length");
    }

    #[test]
    fn keeps_separator_inside_message() {
        let e = Error::decode("aeron-glide\u{1e}io\u{1e}2\u{1e}a\u{1e}b");
        assert_eq!(e.kind(), ErrorKind::Io);
        assert_eq!(e.message(), "a\u{1e}b");
    }

    #[test]
    fn falls_back_to_other() {
        let e = Error::decode("plain message");
        assert_eq!(e.kind(), ErrorKind::Other);
        assert_eq!(e.code(), 0);
        assert_eq!(e.message(), "plain message");
    }

    #[test]
    fn maps_offer_codes() {
        assert_eq!(offer_result(128), Ok(128));
        assert_eq!(offer_result(-1), Err(OfferError::NotConnected));
        assert_eq!(offer_result(-2), Err(OfferError::BackPressured));
        assert_eq!(offer_result(-3), Err(OfferError::AdminAction));
        assert_eq!(offer_result(-4), Err(OfferError::Closed));
        assert_eq!(offer_result(-5), Err(OfferError::MaxPositionExceeded));
        assert!(matches!(offer_result(-6), Err(OfferError::Error(_))));
        assert!(OfferError::BackPressured.is_retryable());
        assert!(!OfferError::Closed.is_retryable());
    }

    #[test]
    fn is_send_sync_static() {
        fn assert<T: Send + Sync + 'static + std::error::Error>() {}
        assert::<Error>();
        assert::<OfferError>();
    }
}
