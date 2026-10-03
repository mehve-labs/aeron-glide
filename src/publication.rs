//! Publications ([`Publication`], [`ExclusivePublication`]).

use super::*;

/// A concurrent publication for sending messages on a channel+stream.
///
/// `Send + Sync`: several threads may `offer` / `try_claim` on the same
/// publication concurrently (e.g. through an `Arc<Publication>`).
pub struct Publication {
    pub(crate) inner: cxx::UniquePtr<ffi::PublicationWrapper>,
}

impl Drop for Publication {
    fn drop(&mut self) {
        callback::drop_outside_conductor(&mut self.inner);
    }
}

// SAFETY: a concurrent publication is designed for use from multiple threads:
// aeron_publication_offer / try_claim are thread-safe, the other bridged methods
// only read state, and every method bridged as `&self` is a const C++ method.
unsafe impl Send for Publication {}
unsafe impl Sync for Publication {}

impl Publication {
    /// Publish a message. Returns the new stream position on success.
    ///
    /// On failure, [`OfferError::is_retryable`] tells whether retrying can succeed
    /// (not connected, back pressured, admin action).
    pub fn offer(&self, buffer: &[u8]) -> std::result::Result<i64, OfferError> {
        error::offer_result(self.inner.offer(buffer)?)
    }

    /// Zero-copy publish: claims a region of the log buffer, calls `handler` with a mutable
    /// slice pointing directly into shared memory, then commits or aborts based on the return value.
    /// Returns the new stream position if the claim succeeded.
    pub fn try_claim<F>(&self, length: usize, handler: F) -> std::result::Result<i64, OfferError>
    where
        F: FnMut(&mut [u8]) -> bool,
    {
        let length = claim_length(length)?;
        let mut cb = Callback::new(handler);
        let result = self.inner.tryClaim(length, callback::claim::<F>, cb.ctx());
        error::offer_result(cb.finish(result)?)
    }

    /// Returns `true` if there is at least one subscriber connected to this publication.
    pub fn is_connected(&self) -> bool {
        self.inner.isConnected()
    }

    /// The session ID assigned by the media driver for this publication.
    pub fn session_id(&self) -> i32 {
        self.inner.sessionId()
    }
}

/// An exclusive publication — single-writer, lower overhead than [`Publication`].
///
/// `Send` but not `Sync`: it can move to another thread, but only one thread may
/// use it at a time.
pub struct ExclusivePublication {
    pub(crate) inner: cxx::UniquePtr<ffi::ExclusivePublicationWrapper>,
}

impl Drop for ExclusivePublication {
    fn drop(&mut self) {
        callback::drop_outside_conductor(&mut self.inner);
    }
}

// SAFETY: an exclusive publication has no thread affinity; it only requires a
// single writer at a time, which `&mut self` on every mutating method enforces.
unsafe impl Send for ExclusivePublication {}

impl ExclusivePublication {
    /// Publish a message. Returns the new stream position on success.
    ///
    /// On failure, [`OfferError::is_retryable`] tells whether retrying can succeed
    /// (not connected, back pressured, admin action).
    pub fn offer(&mut self, buffer: &[u8]) -> std::result::Result<i64, OfferError> {
        error::offer_result(self.inner.pin_mut().offer(buffer)?)
    }

    /// Zero-copy publish: claims a region of the log buffer, calls `handler` with a mutable
    /// slice pointing directly into shared memory, then commits or aborts based on the return value.
    /// Returns the new stream position if the claim succeeded.
    pub fn try_claim<F>(
        &mut self,
        length: usize,
        handler: F,
    ) -> std::result::Result<i64, OfferError>
    where
        F: FnMut(&mut [u8]) -> bool,
    {
        let length = claim_length(length)?;
        let mut cb = Callback::new(handler);
        let result = self
            .inner
            .pin_mut()
            .tryClaim(length, callback::claim::<F>, cb.ctx());
        error::offer_result(cb.finish(result)?)
    }

    /// Returns `true` if there is at least one subscriber connected to this publication.
    pub fn is_connected(&self) -> bool {
        self.inner.isConnected()
    }
}

/// Aeron claim lengths are `int32`; reject longer ones instead of truncating them.
fn claim_length(length: usize) -> std::result::Result<usize, OfferError> {
    if length > i32::MAX as usize {
        return Err(OfferError::Error(Error::new(
            ErrorKind::IllegalArgument,
            format!("claim length {length} exceeds the maximum of {}", i32::MAX),
        )));
    }
    Ok(length)
}
