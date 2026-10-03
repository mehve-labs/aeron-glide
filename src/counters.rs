//! Counters ([`Counter`], [`CountersReader`]).

use super::*;

/// Reader for the media driver's CNC (Command and Control) counters.
///
/// Provides access to real-time statistics like bytes sent/received, NAKs,
/// errors, and heartbeats. `Send + Sync`.
pub struct CountersReader {
    pub(crate) inner: cxx::UniquePtr<ffi::CountersReaderWrapper>,
}

impl Drop for CountersReader {
    fn drop(&mut self) {
        callback::drop_outside_conductor(&mut self.inner);
    }
}

// SAFETY: the reader only reads the counters' shared memory (written by the media
// driver with ordered stores), and holds a `shared_ptr<aeron::Aeron>` (atomic
// reference count). All bridged methods are const.
unsafe impl Send for CountersReader {}
unsafe impl Sync for CountersReader {}

impl CountersReader {
    /// State of a counter record that was never allocated.
    pub const RECORD_UNUSED: i32 = 0;
    /// State of an allocated counter.
    pub const RECORD_ALLOCATED: i32 = 1;
    /// State of a freed counter.
    pub const RECORD_RECLAIMED: i32 = -1;
    /// The registration ID of a counter allocated without one.
    pub const DEFAULT_REGISTRATION_ID: i64 = 0;
    /// The free-for-reuse deadline of a counter that is not free.
    pub const NOT_FREE_TO_REUSE: i64 = i64::MAX;
    /// Maximum length of a counter label, in bytes.
    pub const MAX_LABEL_LENGTH: usize = 380;
    /// Maximum length of a counter key, in bytes.
    pub const MAX_KEY_LENGTH: usize = 112;

    /// The highest valid counter ID (the counters file's capacity minus one).
    pub fn max_counter_id(&self) -> i32 {
        self.inner.maxCounterId()
    }

    /// Read the current value of a counter by ID. Fails for an out-of-range ID.
    pub fn get_counter_value(&self, id: i32) -> Result<i64> {
        Ok(self.inner.getCounterValue(id)?)
    }

    /// Get the state of a counter (e.g., active, inactive). Fails for an out-of-range ID.
    pub fn get_counter_state(&self, id: i32) -> Result<i32> {
        Ok(self.inner.getCounterState(id)?)
    }

    /// Get the type ID of a counter. Fails for an out-of-range ID.
    pub fn get_counter_type_id(&self, id: i32) -> Result<i32> {
        Ok(self.inner.getCounterTypeId(id)?)
    }

    /// Get the human-readable label of a counter. Fails for an out-of-range ID.
    /// Invalid UTF-8 is replaced with `U+FFFD`.
    pub fn get_counter_label(&self, id: i32) -> Result<String> {
        Ok(self.inner.getCounterLabel(id)?)
    }

    /// The ID of the first allocated counter with this registration ID, if any
    /// (C++ `findByRegistrationId`). Registration IDs are only unique per counter
    /// type (the driver's system counters use their counter ID), so prefer
    /// [`find_by_type_id_and_registration_id`](Self::find_by_type_id_and_registration_id).
    pub fn find_by_registration_id(&self, registration_id: i64) -> Option<i32> {
        // Only fails for an invalid ID, which iteration never produces.
        self.inner
            .findByRegistrationId(registration_id)
            .ok()
            .and_then(found)
    }

    /// The ID of the allocated counter with this type ID and registration ID, if
    /// any.
    pub fn find_by_type_id_and_registration_id(
        &self,
        type_id: i32,
        registration_id: i64,
    ) -> Option<i32> {
        // Only fails for an invalid ID, which iteration never produces.
        self.inner
            .findByTypeIdAndRegistrationId(type_id, registration_id)
            .ok()
            .and_then(found)
    }

    /// The registration ID of a counter
    /// ([`DEFAULT_REGISTRATION_ID`](Self::DEFAULT_REGISTRATION_ID) if it was
    /// allocated without one). Fails for an out-of-range ID.
    pub fn get_counter_registration_id(&self, id: i32) -> Result<i64> {
        Ok(self.inner.getCounterRegistrationId(id)?)
    }

    /// The ID of the client that owns a counter (the driver's own counters have
    /// the default owner ID). Fails for an out-of-range ID.
    pub fn get_counter_owner_id(&self, id: i32) -> Result<i64> {
        Ok(self.inner.getCounterOwnerId(id)?)
    }

    /// When a freed counter's record may be reused, in milliseconds since the
    /// epoch ([`NOT_FREE_TO_REUSE`](Self::NOT_FREE_TO_REUSE) while allocated).
    /// Fails for an out-of-range ID.
    pub fn get_free_for_reuse_deadline(&self, id: i32) -> Result<i64> {
        Ok(self.inner.getFreeForReuseDeadline(id)?)
    }

    /// A copy of a counter's key: the whole key region,
    /// [`MAX_KEY_LENGTH`](Self::MAX_KEY_LENGTH) bytes (counters do not record
    /// their key length). Fails for an out-of-range ID.
    pub fn get_counter_key(&self, id: i32) -> Result<Vec<u8>> {
        Ok(self.inner.getCounterKey(id)?)
    }

    /// A [`Counter`] handle on the counter `counter_id`, e.g. to update a counter
    /// another client allocated (C++ `Counter(CountersReader&, registrationId,
    /// counterId)`). `registration_id` is only reported back by
    /// [`Counter::registration_id`]. The handle does not own the counter: dropping
    /// it does not free it, and [`Counter::is_closed`] is always `false`.
    ///
    /// Fails if `counter_id` is out of range.
    pub fn counter(&self, registration_id: i64, counter_id: i32) -> Result<Counter> {
        Ok(Counter {
            inner: self.inner.counter(registration_id, counter_id)?,
        })
    }

    /// Iterate over the allocated counters, calling `handler(counter_id, type_id, key_bytes, label)` for each.
    pub fn for_each<F>(&self, handler: F) -> Result<()>
    where
        F: FnMut(i32, i32, &[u8], &str),
    {
        let mut cb = Callback::new(handler);
        let result = self.inner.forEach(callback::counter::<F>, cb.ctx());
        Ok(cb.finish(result)?)
    }
}

/// `NULL_COUNTER_ID` (-1) means not found.
fn found(id: i32) -> Option<i32> {
    (id >= 0).then_some(id)
}

/// A counter in the media driver's counters file (C++ `aeron::Counter`), added
/// with [`AeronClient::add_counter`] or [`AeronClient::add_static_counter`], or a
/// handle on an existing counter from [`CountersReader::counter`].
///
/// An added counter is freed when its last handle is dropped (static counters
/// are never freed). Other processes see it through their counters reader, e.g.
/// with `AeronStat`.
///
/// `Send + Sync`. The plain operations are atomic. The `_ordered` and `_weak`
/// operations are cheaper but assume a single writer: concurrent writes through
/// them (from any thread or process) can lose updates.
pub struct Counter {
    pub(crate) inner: cxx::UniquePtr<ffi::CounterWrapper>,
}

impl Drop for Counter {
    fn drop(&mut self) {
        callback::drop_outside_conductor(&mut self.inner);
    }
}

// SAFETY: every operation is a read or atomic write of the counter's shared
// memory, or a const read of the counters metadata (both written by other
// processes concurrently anyway). The wrapper holds `shared_ptr`s (atomic
// reference counts); closing it runs under the client's conductor lock.
unsafe impl Send for Counter {}
unsafe impl Sync for Counter {}

impl std::fmt::Debug for Counter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Counter")
            .field("id", &self.id())
            .field("registration_id", &self.registration_id())
            .field("value", &self.get())
            .finish()
    }
}

impl Counter {
    /// The counter's ID in the counters file.
    pub fn id(&self) -> i32 {
        self.inner.id()
    }

    /// The registration ID: the add's correlation ID, or the ID given to
    /// [`AeronClient::add_static_counter`].
    pub fn registration_id(&self) -> i64 {
        self.inner.registrationId()
    }

    /// The counter's record state, e.g. [`CountersReader::RECORD_ALLOCATED`].
    pub fn state(&self) -> Result<i32> {
        Ok(self.inner.state()?)
    }

    /// The counter's label. Invalid UTF-8 is replaced with `U+FFFD`.
    pub fn label(&self) -> Result<String> {
        Ok(self.inner.label()?)
    }

    /// Returns `true` once the counter has been closed, e.g. because the client
    /// closed. Always `false` for a handle from [`CountersReader::counter`].
    pub fn is_closed(&self) -> bool {
        self.inner.isClosed()
    }

    /// The current value (volatile read).
    pub fn get(&self) -> i64 {
        self.inner.get()
    }

    /// The current value, without ordering guarantees (plain read).
    pub fn get_weak(&self) -> i64 {
        self.inner.getWeak()
    }

    /// Set the value (atomic store with full ordering).
    pub fn set(&self, value: i64) {
        self.inner.set(value)
    }

    /// Set the value with release ordering.
    pub fn set_ordered(&self, value: i64) {
        self.inner.setOrdered(value)
    }

    /// Set the value without ordering guarantees (plain store).
    pub fn set_weak(&self, value: i64) {
        self.inner.setWeak(value)
    }

    /// Add one (atomic).
    pub fn increment(&self) {
        self.inner.increment()
    }

    /// Add one with a release store: single writer only.
    pub fn increment_ordered(&self) {
        self.inner.incrementOrdered()
    }

    /// Add `delta` (atomic), returning the previous value.
    pub fn get_and_add(&self, delta: i64) -> i64 {
        self.inner.getAndAdd(delta)
    }

    /// Add `delta` with a release store, returning the previous value: single
    /// writer only.
    pub fn get_and_add_ordered(&self, delta: i64) -> i64 {
        self.inner.getAndAddOrdered(delta)
    }

    /// Set the value (atomic), returning the previous one.
    pub fn get_and_set(&self, value: i64) -> i64 {
        self.inner.getAndSet(value)
    }

    /// Set the value to `update` if it is `expected` (atomic). Returns whether it
    /// was set.
    pub fn compare_and_set(&self, expected: i64, update: i64) -> bool {
        self.inner.compareAndSet(expected, update)
    }
}
