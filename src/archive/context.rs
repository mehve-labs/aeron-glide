//! Archive client configuration ([`Context`]).

use super::client::{AeronArchive, AsyncConnect};
use super::ffi;
use super::types::RecordingSignal;
use crate::handlers::{self, into_ctx, release};
use crate::{AeronClient, Error, Result};
use std::pin::Pin;
use std::time::Duration;

/// Configuration for an archive client (C++ `aeron::archive::client::Context`),
/// connected with [`connect`](Self::connect) or [`connect_async`](Self::connect_async).
///
/// The handlers run on the thread calling the archive client, while it waits
/// for the archive (they need `Send + Sync`). Archive requests from them, or
/// from client handlers, fail with [`ErrorKind::Reentrant`](crate::ErrorKind::Reentrant).
///
/// Settings are applied as they are set; the first invalid one is reported by
/// `connect`.
pub struct Context {
    inner: Result<cxx::UniquePtr<ffi::ArchiveContextWrapper>>,
}

// SAFETY: the C++ context is not shared with any other thread until it is
// connected, and the handlers it holds are `Send + Sync`.
unsafe impl Send for Context {}

impl Drop for Context {
    fn drop(&mut self) {
        // It may hold the last reference to its client (`aeron`): not on that
        // client's conductor thread, e.g. inside one of its handlers.
        if let Ok(inner) = &mut self.inner {
            crate::callback::drop_outside_conductor(inner);
        }
    }
}

impl Default for Context {
    fn default() -> Self {
        Self {
            inner: ffi::create_archive_context().map_err(Error::from),
        }
    }
}

impl std::fmt::Debug for Context {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Context")
            .field("error", &self.inner.as_ref().err())
            .finish_non_exhaustive()
    }
}

/// The settings of a connected archive client ([`AeronArchive::context`]).
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct ContextInfo {
    /// The Aeron directory set on the context, used by an internal client (a
    /// shared client's directory is [`AeronClient::aeron_dir`]).
    pub aeron_directory_name: String,
    /// The channel requests are sent on.
    pub control_request_channel: String,
    /// The stream ID requests are sent on.
    pub control_request_stream_id: i32,
    /// The channel responses are received on.
    pub control_response_channel: String,
    /// The stream ID responses are received on.
    pub control_response_stream_id: i32,
    /// The channel recording events are published on.
    pub recording_events_channel: String,
    /// How long to wait for a response.
    pub message_timeout: Duration,
    /// How many times a request is retried when the publication is back
    /// pressured.
    pub message_retry_attempts: u32,
    /// The maximum length of an error message from the archive.
    pub max_error_message_length: u32,
}

impl ContextInfo {
    pub(crate) fn from_ffi(c: ffi::ArchiveContextInfo) -> Self {
        Self {
            aeron_directory_name: c.aeron_directory_name,
            control_request_channel: c.control_request_channel,
            control_request_stream_id: c.control_request_stream_id,
            control_response_channel: c.control_response_channel,
            control_response_stream_id: c.control_response_stream_id,
            recording_events_channel: c.recording_events_channel,
            message_timeout: Duration::from_nanos(u64::try_from(c.message_timeout_ns).unwrap_or(0)),
            message_retry_attempts: c.message_retry_attempts,
            max_error_message_length: c.max_error_message_length,
        }
    }
}

impl Context {
    /// A context with the archive client's defaults (and the `AERON_ARCHIVE_*`
    /// environment variables).
    pub fn new() -> Self {
        Self::default()
    }

    fn set(
        mut self,
        setting: impl FnOnce(Pin<&mut ffi::ArchiveContextWrapper>) -> Result<()>,
    ) -> Self {
        if let Ok(ctx) = &mut self.inner
            && let Err(e) = setting(ctx.pin_mut())
        {
            self.inner = Err(e);
        }
        self
    }

    /// Use `client` for the archive's publication and subscription (C++
    /// `aeron`), instead of an internal client. Recorded publications and
    /// replays then belong to it.
    pub fn aeron(self, client: &AeronClient) -> Self {
        // The C++ context shares (and keeps alive) the C++ client.
        self.set(|ctx| Ok(ctx.setAeron(&client.inner)?))
    }

    /// The Aeron directory of the internal client (when [`aeron`](Self::aeron) is
    /// not set).
    pub fn aeron_directory_name(self, dir: &str) -> Self {
        let dir = dir.to_string();
        self.set(move |ctx| Ok(ctx.setAeronDirectoryName(&dir)?))
    }

    /// The archive's control channel, e.g. `aeron:udp?endpoint=localhost:8010`.
    pub fn control_request_channel(self, channel: &str) -> Self {
        let channel = channel.to_string();
        self.set(move |ctx| Ok(ctx.setControlRequestChannel(&channel)?))
    }

    /// The archive's control stream ID.
    pub fn control_request_stream_id(self, stream_id: i32) -> Self {
        self.set(move |ctx| Ok(ctx.setControlRequestStreamId(stream_id)?))
    }

    /// The channel this client receives responses on, e.g.
    /// `aeron:udp?endpoint=localhost:0`, or a response channel
    /// (`aeron:udp?control-mode=response|control=localhost:10002`).
    pub fn control_response_channel(self, channel: &str) -> Self {
        let channel = channel.to_string();
        self.set(move |ctx| Ok(ctx.setControlResponseChannel(&channel)?))
    }

    /// The stream ID this client receives responses on.
    pub fn control_response_stream_id(self, stream_id: i32) -> Self {
        self.set(move |ctx| Ok(ctx.setControlResponseStreamId(stream_id)?))
    }

    /// The channel the archive publishes recording events on.
    pub fn recording_events_channel(self, channel: &str) -> Self {
        let channel = channel.to_string();
        self.set(move |ctx| Ok(ctx.setRecordingEventsChannel(&channel)?))
    }

    /// How long to wait for a response (C++ `messageTimeoutNs`).
    pub fn message_timeout(self, timeout: Duration) -> Self {
        // The C client adds it to the clock: clamp far below overflow.
        let ns = crate::timeout_nanos(timeout);
        self.set(move |ctx| Ok(ctx.setMessageTimeoutNs(ns)?))
    }

    /// How many times a request is retried when the publication is back
    /// pressured.
    pub fn message_retry_attempts(self, attempts: u32) -> Self {
        self.set(move |ctx| Ok(ctx.setMessageRetryAttempts(attempts)?))
    }

    /// The maximum length of an error message from the archive (default 1000).
    pub fn max_error_message_length(self, length: u32) -> Self {
        self.set(move |ctx| Ok(ctx.setMaxErrorMessageLength(length)?))
    }

    /// Called with the amount of work done each time the client waits for the
    /// archive (C++ `idleStrategy`; the default yields the thread).
    pub fn idle_strategy<F>(self, idle: F) -> Self
    where
        F: Fn(usize) + Send + Sync + 'static,
    {
        self.set(move |ctx| {
            Ok(ctx.setIdleStrategy(idle_trampoline::<F>, release::<F>, into_ctx(idle))?)
        })
    }

    /// Called while the client waits for the archive (C++
    /// `delegatingInvoker`), e.g. to run other work. In agent invoker mode the
    /// archive client runs the client conductor itself: calling
    /// [`AeronClient::invoke`] from here fails.
    pub fn delegating_invoker<F>(self, invoker: F) -> Self
    where
        F: Fn() + Send + Sync + 'static,
    {
        self.set(move |ctx| {
            Ok(ctx.setDelegatingInvoker(
                invoker_trampoline::<F>,
                release::<F>,
                into_ctx(invoker),
            )?)
        })
    }

    /// Handle errors the archive reports asynchronously, e.g. error responses to
    /// requests that are no longer awaited. Without a handler they are dropped
    /// (only [`AeronArchive::poll_for_error_response`] and
    /// [`check_for_error_response`](AeronArchive::check_for_error_response)
    /// report them).
    pub fn error_handler<F>(self, handler: F) -> Self
    where
        F: Fn(&Error) + Send + Sync + 'static,
    {
        self.set(move |ctx| {
            Ok(ctx.setErrorHandler(handlers::error::<F>, release::<F>, into_ctx(handler))?)
        })
    }

    /// Called with the recording signals received while the client polls for
    /// responses (C++ `recordingSignalConsumer`).
    pub fn recording_signal_consumer<F>(self, consumer: F) -> Self
    where
        F: Fn(&RecordingSignal) + Send + Sync + 'static,
    {
        self.set(move |ctx| {
            Ok(ctx.setRecordingSignalConsumer(
                signal_trampoline::<F>,
                release::<F>,
                into_ctx(consumer),
            )?)
        })
    }

    /// Credentials for an archive that authenticates (C++ `credentialsSupplier`):
    /// `encoded_credentials` returns the credentials sent when connecting, and
    /// `on_challenge` answers a challenge from the archive.
    pub fn credentials_supplier<C, H>(self, encoded_credentials: C, on_challenge: H) -> Self
    where
        C: Fn() -> Vec<u8> + Send + Sync + 'static,
        H: Fn(&[u8]) -> Vec<u8> + Send + Sync + 'static,
    {
        self.set(move |ctx| {
            Ok(ctx.setCredentialsSupplier(
                credentials_trampoline::<(C, H)>,
                challenge_trampoline::<(C, H)>,
                release::<(C, H)>,
                into_ctx((encoded_credentials, on_challenge)),
            )?)
        })
    }

    pub(crate) fn build(mut self) -> Result<cxx::UniquePtr<ffi::ArchiveContextWrapper>> {
        std::mem::replace(&mut self.inner, Err(used()))
    }

    /// Connect to the archive, waiting for it to answer (C++
    /// `AeronArchive::connect`).
    pub fn connect(self) -> Result<AeronArchive> {
        crate::callback::ensure_not_in_conductor_callback("connecting to an archive")?;
        let mut ctx = self.build()?;
        Ok(AeronArchive::new(ffi::archive_connect(ctx.pin_mut())?))
    }

    /// Start connecting to the archive without waiting (C++
    /// `AeronArchive::asyncConnect`): poll the returned [`AsyncConnect`].
    ///
    /// Starting makes one blocking round trip to the media driver (for the next
    /// session id), so like [`connect`](Self::connect) it fails with
    /// [`ErrorKind::Reentrant`](crate::ErrorKind::Reentrant) from a client handler.
    pub fn connect_async(self) -> Result<AsyncConnect> {
        crate::callback::ensure_not_in_conductor_callback("connecting to an archive")?;
        Ok(AsyncConnect::new(ffi::archive_async_connect(
            self.build()?,
        )?))
    }
}

fn invoker_trampoline<F: Fn() + Send + Sync + 'static>(ctx: usize) {
    handlers::invoke::<F>(ctx, "archive delegating invoker", |f| f());
}

fn idle_trampoline<F: Fn(usize) + Send + Sync + 'static>(ctx: usize, work_count: i32) {
    let work_count = crate::error::count(work_count);
    handlers::invoke::<F>(ctx, "archive idle strategy", |f| f(work_count));
}

fn signal_trampoline<F: Fn(&RecordingSignal) + Send + Sync + 'static>(
    ctx: usize,
    signal: &ffi::RecordingSignalInfo,
) {
    let signal = RecordingSignal::from_ffi(signal);
    handlers::invoke::<F>(ctx, "recording signal", |f| f(&signal));
}

fn credentials_trampoline<T>(ctx: usize) -> Vec<u8>
where
    T: CredentialsPair,
{
    let mut out = Vec::new();
    handlers::invoke::<T>(ctx, "credentials supplier", |pair| out = pair.credentials());
    out
}

fn challenge_trampoline<T>(ctx: usize, challenge: &[u8]) -> Vec<u8>
where
    T: CredentialsPair,
{
    let mut out = Vec::new();
    handlers::invoke::<T>(ctx, "credentials challenge", |pair| {
        out = pair.on_challenge(challenge)
    });
    out
}

trait CredentialsPair {
    fn credentials(&self) -> Vec<u8>;
    fn on_challenge(&self, challenge: &[u8]) -> Vec<u8>;
}

impl<C, H> CredentialsPair for (C, H)
where
    C: Fn() -> Vec<u8>,
    H: Fn(&[u8]) -> Vec<u8>,
{
    fn credentials(&self) -> Vec<u8> {
        (self.0)()
    }

    fn on_challenge(&self, challenge: &[u8]) -> Vec<u8> {
        (self.1)(challenge)
    }
}

/// The value left behind once a context's C++ object has been taken.
pub(crate) fn used() -> Error {
    Error::new(
        crate::ErrorKind::IllegalState,
        "the archive context was already used",
    )
}
