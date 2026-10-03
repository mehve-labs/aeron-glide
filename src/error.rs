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

/// The category of an [`Error`], mirroring the Aeron C++ exception hierarchy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum ErrorKind {
    /// An argument was rejected, e.g. a message longer than the max payload.
    IllegalArgument,
    /// The operation is not valid in the current state, e.g. a closed resource.
    IllegalState,
    /// An I/O error, e.g. a missing or unreadable Aeron directory.
    Io,
    /// Malformed data.
    Format,
    /// An index or id outside the valid range, e.g. a bad counter id.
    OutOfBounds,
    /// A channel URI or other input could not be parsed.
    Parse,
    /// A requested element does not exist.
    ElementNotFound,
    /// The media driver did not respond in time. Fatal for the client.
    DriverTimeout,
    /// The client conductor service was not invoked in time. Fatal for the client.
    ConductorServiceTimeout,
    /// The media driver timed this client out. Fatal for the client.
    ClientTimeout,
    /// A generic timeout, e.g. an archive request without a response.
    Timeout,
    /// A channel endpoint failed, e.g. a UDP port already in use.
    ChannelEndpoint,
    /// The media driver rejected a registration (publication, subscription, ...).
    Registration,
    /// A subscription is unknown to the media driver.
    UnknownSubscription,
    /// A callback re-entered the client in an unsupported way.
    Reentrant,
    /// The operation is not supported.
    UnsupportedOperation,
    /// An Archive error response.
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
            "format" => Self::Format,
            "out_of_bounds" => Self::OutOfBounds,
            "parse" => Self::Parse,
            "element_not_found" => Self::ElementNotFound,
            "driver_timeout" => Self::DriverTimeout,
            "conductor_service_timeout" => Self::ConductorServiceTimeout,
            "client_timeout" => Self::ClientTimeout,
            "timeout" => Self::Timeout,
            "channel_endpoint" => Self::ChannelEndpoint,
            "registration" => Self::Registration,
            "unknown_subscription" => Self::UnknownSubscription,
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

    /// The category of this error.
    pub fn kind(&self) -> ErrorKind {
        self.kind
    }

    /// The Aeron error code (`aeron_errcode()` / `SourcedException::errorCode()`),
    /// or `0` if none was reported.
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

impl From<cxx::Exception> for Error {
    fn from(e: cxx::Exception) -> Self {
        Self::decode(e.what())
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
        write!(f, "{:?}: {}", self.kind, self.message)?;
        if self.code != 0 {
            write!(f, " (code {})", self.code)?;
        }
        Ok(())
    }
}

impl std::error::Error for Error {}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn is_send_sync_static() {
        fn assert<T: Send + Sync + 'static + std::error::Error>() {}
        assert::<Error>();
    }
}
