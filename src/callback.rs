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

use crate::{ControlledAction, PollAction};
use std::any::Any;
use std::panic::{self, AssertUnwindSafe};

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
        self as *mut Self as usize
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
        unsafe { &mut *(ctx as *mut Self) }
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
pub(crate) fn fragment<F: FnMut(&[u8])>(ctx: usize, buffer: &[u8]) {
    // SAFETY: C++ only calls this with the ctx passed alongside it, during the call.
    let cb = unsafe { Callback::<F>::from_ctx(ctx) };
    cb.call((), |f| f(buffer));
}

/// Controlled fragment handler. A panic aborts the fragment, so it is delivered
/// again by the next poll.
pub(crate) fn controlled_fragment<F, R>(ctx: usize, buffer: &[u8]) -> i32
where
    F: FnMut(&[u8]) -> R,
    R: PollAction,
{
    // SAFETY: as in `fragment`.
    let cb = unsafe { Callback::<F>::from_ctx(ctx) };
    cb.call(ControlledAction::Abort as i32, |f| {
        f(buffer).into_action() as i32
    })
}

/// Buffer claim handler: `true` commits, `false` aborts. A panic aborts the claim.
pub(crate) fn claim<F: FnMut(&mut [u8]) -> bool>(ctx: usize, buffer: &mut [u8]) -> bool {
    // SAFETY: as in `fragment`.
    let cb = unsafe { Callback::<F>::from_ctx(ctx) };
    cb.call(false, |f| f(buffer))
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
