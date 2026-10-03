//! Passing Rust closures through the cxx bridge.
//!
//! A closure is wrapped in a [`Callback`] on the caller's stack and handed to
//! C++ as a pair: a monomorphised trampoline (`fn` pointer) and the address of
//! the `Callback` (`ctx`). C++ calls the trampoline synchronously, before the
//! bridged call returns, so `ctx` is always valid while it is used. There is no
//! registry, so nested polls (polling another subscription from inside a
//! handler) work.
//!
//! Panics must not unwind into C++, so each trampoline catches them, stops
//! invoking the closure, and [`Callback::finish`] resumes the panic once the
//! bridged call has returned.

use crate::{ControlledAction, Header, PollAction, ffi};
use std::any::Any;
use std::cell::Cell;
use std::panic::{self, AssertUnwindSafe};

thread_local! {
    static IN_CONDUCTOR_CALLBACK: Cell<bool> = const { Cell::new(false) };
}

/// Returns `true` while this thread runs a callback invoked by an Aeron client
/// conductor (a handler, or a handler's release).
pub(crate) fn in_conductor_callback() -> bool {
    IN_CONDUCTOR_CALLBACK.get()
}

/// Fails with [`ErrorKind::Reentrant`](crate::ErrorKind::Reentrant) inside a
/// conductor callback, for calls that would wait on the conductor.
pub(crate) fn ensure_not_in_conductor_callback(what: &str) -> crate::Result<()> {
    if in_conductor_callback() {
        return Err(crate::Error::new(
            crate::ErrorKind::Reentrant,
            format!("{what} cannot be called from an Aeron handler (client, archive or driver)"),
        ));
    }
    Ok(())
}

/// Marks the current thread as running a callback invoked by an Aeron client
/// conductor (e.g. the error handler) until dropped.
pub(crate) struct ConductorCallbackScope(bool);

impl ConductorCallbackScope {
    pub(crate) fn enter() -> Self {
        Self(IN_CONDUCTOR_CALLBACK.replace(true))
    }
}

impl Drop for ConductorCallbackScope {
    fn drop(&mut self) {
        IN_CONDUCTOR_CALLBACK.set(self.0);
    }
}

/// Drop a C++ object that may hold the last reference to an Aeron client.
///
/// Destroying a client closes it and joins its conductor thread, so it must not
/// happen on that conductor thread, inside one of its callbacks. There the drop is
/// handed to a short-lived thread instead.
pub(crate) fn drop_outside_conductor<T>(inner: &mut cxx::UniquePtr<T>)
where
    T: cxx::memory::UniquePtrTarget + 'static,
{
    if !IN_CONDUCTOR_CALLBACK.get() {
        return;
    }
    struct SendPtr<T: cxx::memory::UniquePtrTarget>(#[allow(dead_code)] cxx::UniquePtr<T>);
    // SAFETY: only used for the types that call it, which are `Send` themselves
    // (the client, publications, subscriptions, counters and counters readers).
    unsafe impl<T: cxx::memory::UniquePtrTarget> Send for SendPtr<T> {}
    let ptr = SendPtr(std::mem::replace(inner, cxx::UniquePtr::null()));
    std::thread::spawn(move || drop(ptr));
}

pub(crate) struct Callback<F> {
    f: F,
    panic: Option<Box<dyn Any + Send + 'static>>,
}

impl<F> Callback<F> {
    pub(crate) fn new(f: F) -> Self {
        Self { f, panic: None }
    }

    /// The opaque context handed to C++ alongside the trampoline.
    pub(crate) fn ctx(&mut self) -> usize {
        (self as *mut Self).expose_provenance()
    }

    /// Resume a panic raised by the closure, otherwise return `result`.
    pub(crate) fn finish<T>(self, result: T) -> T {
        if let Some(payload) = self.panic {
            panic::resume_unwind(payload);
        }
        result
    }

    /// # Safety
    ///
    /// `ctx` must come from [`Callback::ctx`] on a `Callback<F>` that is still alive
    /// and not otherwise borrowed.
    pub(crate) unsafe fn from_ctx<'a>(ctx: usize) -> &'a mut Self {
        unsafe { &mut *std::ptr::with_exposed_provenance_mut::<Self>(ctx) }
    }

    /// Invoke the closure, or return `fallback` if it panicked now or earlier.
    pub(crate) fn call<R>(&mut self, fallback: R, invoke: impl FnOnce(&mut F) -> R) -> R {
        if self.panic.is_some() {
            return fallback;
        }
        match panic::catch_unwind(AssertUnwindSafe(|| invoke(&mut self.f))) {
            Ok(r) => r,
            Err(payload) => {
                self.panic = Some(payload);
                fallback
            }
        }
    }
}

/// Fragment handler. After a panic, the remaining fragments of the same poll are
/// consumed without being delivered.
pub(crate) fn fragment<F: FnMut(&[u8], &Header)>(ctx: usize, buffer: &[u8], header: &ffi::Header) {
    // SAFETY: C++ only calls this with the ctx passed alongside it, during the call.
    let cb = unsafe { Callback::<F>::from_ctx(ctx) };
    cb.call((), |f| f(buffer, Header::from_ffi(header)));
}

/// Controlled fragment handler. A panic aborts the fragment, so it is delivered
/// again by the next poll.
pub(crate) fn controlled_fragment<F, R>(ctx: usize, buffer: &[u8], header: &ffi::Header) -> i32
where
    F: FnMut(&[u8], &Header) -> R,
    R: PollAction,
{
    // SAFETY: as in `fragment`.
    let cb = unsafe { Callback::<F>::from_ctx(ctx) };
    cb.call(ControlledAction::Abort as i32, |f| {
        f(buffer, Header::from_ffi(header)).into_action() as i32
    })
}

/// Block handler: `(block of frames, session_id, term_id)`.
pub(crate) fn block<F: FnMut(&[u8], i32, i32)>(
    ctx: usize,
    block: &[u8],
    session_id: i32,
    term_id: i32,
) {
    // SAFETY: as in `fragment`.
    let cb = unsafe { Callback::<F>::from_ctx(ctx) };
    cb.call((), |f| f(block, session_id, term_id));
}

/// Reserved value supplier: called with each frame (header and payload), returns
/// the value written into the frame header. A panic writes 0.
pub(crate) fn reserved_value<F: FnMut(&[u8]) -> i64>(ctx: usize, frame: &[u8]) -> i64 {
    // SAFETY: as in `fragment`.
    let cb = unsafe { Callback::<F>::from_ctx(ctx) };
    cb.call(0, |f| f(frame))
}

/// Error log consumer.
pub(crate) fn error_log<F: FnMut(&crate::ErrorLogEntry<'_>)>(
    ctx: usize,
    observation_count: i32,
    first_observation_timestamp: i64,
    last_observation_timestamp: i64,
    error: &[u8],
) {
    // SAFETY: as in `fragment`.
    let cb = unsafe { Callback::<F>::from_ctx(ctx) };
    let error = String::from_utf8_lossy(error);
    let entry = crate::ErrorLogEntry {
        observation_count,
        first_observation_timestamp,
        last_observation_timestamp,
        error: &error,
    };
    cb.call((), |f| f(&entry));
}

/// Loss report entry handler.
#[allow(clippy::too_many_arguments)]
pub(crate) fn loss_report<F: FnMut(&crate::LossReportEntry<'_>)>(
    ctx: usize,
    observation_count: i64,
    total_bytes_lost: i64,
    first_observation_timestamp: i64,
    last_observation_timestamp: i64,
    session_id: i32,
    stream_id: i32,
    channel: &[u8],
    source: &[u8],
) {
    // SAFETY: as in `fragment`.
    let cb = unsafe { Callback::<F>::from_ctx(ctx) };
    let channel = String::from_utf8_lossy(channel);
    let source = String::from_utf8_lossy(source);
    let entry = crate::LossReportEntry {
        observation_count,
        total_bytes_lost,
        first_observation_timestamp,
        last_observation_timestamp,
        session_id,
        stream_id,
        channel: &channel,
        source: &source,
    };
    cb.call((), |f| f(&entry));
}

/// Counter metadata handler: `(counter_id, type_id, key, label)`.
pub(crate) fn counter<F: FnMut(i32, i32, &[u8], &str)>(
    ctx: usize,
    counter_id: i32,
    type_id: i32,
    key: &[u8],
    label: &[u8],
) {
    // SAFETY: as in `fragment`.
    let cb = unsafe { Callback::<F>::from_ctx(ctx) };
    let label = String::from_utf8_lossy(label);
    cb.call((), |f| f(counter_id, type_id, key, &label));
}
