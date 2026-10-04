//! Counters ([`Counter`], [`CountersReader`]) and the CnC file ([`CncFile`]).

use super::*;
use crate::error::Ffi;
use std::time::Duration;

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
// driver with ordered stores), and holds a `shared_ptr` aliasing the client or the
// CnC file that owns that memory (atomic reference count). All bridged methods
// are const.
unsafe impl Send for CountersReader {}
unsafe impl Sync for CountersReader {}

impl std::fmt::Debug for CountersReader {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CountersReader")
            .field("max_counter_id", &self.max_counter_id())
            .finish_non_exhaustive()
    }
}

impl CountersReader {
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
        self.inner.getCounterValue(id).ffi()
    }

    /// The record state of a counter. Fails for an out-of-range ID.
    pub fn get_counter_state(&self, id: i32) -> Result<CounterState> {
        Ok(CounterState::from_raw(
            self.inner.getCounterState(id).ffi()?,
        ))
    }

    /// Get the type ID of a counter. Fails for an out-of-range ID.
    pub fn get_counter_type_id(&self, id: i32) -> Result<i32> {
        self.inner.getCounterTypeId(id).ffi()
    }

    /// Get the human-readable label of a counter. Fails for an out-of-range ID.
    /// Invalid UTF-8 is replaced with `U+FFFD`.
    pub fn get_counter_label(&self, id: i32) -> Result<String> {
        self.inner.getCounterLabel(id).ffi()
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
        self.inner.getCounterRegistrationId(id).ffi()
    }

    /// The ID of the client that owns a counter ([`AeronClient::client_id`]); -1
    /// for the driver's system counters and for static counters. Fails for an
    /// out-of-range ID.
    pub fn get_counter_owner_id(&self, id: i32) -> Result<i64> {
        self.inner.getCounterOwnerId(id).ffi()
    }

    /// When a freed counter's record may be reused, in milliseconds since the
    /// epoch ([`NOT_FREE_TO_REUSE`](Self::NOT_FREE_TO_REUSE) while allocated).
    /// Fails for an out-of-range ID.
    pub fn get_free_for_reuse_deadline(&self, id: i32) -> Result<i64> {
        self.inner.getFreeForReuseDeadline(id).ffi()
    }

    /// A copy of a counter's key: the whole key region,
    /// [`MAX_KEY_LENGTH`](Self::MAX_KEY_LENGTH) bytes (counters do not record
    /// their key length). Fails for an out-of-range ID.
    pub fn get_counter_key(&self, id: i32) -> Result<Vec<u8>> {
        self.inner.getCounterKey(id).ffi()
    }

    /// A writable [`Counter`] handle on a counter another client (or another
    /// part of your program) allocated, e.g. to update a shared statistic (C++
    /// `Counter(CountersReader&, registrationId, counterId)`).
    ///
    /// Only for your own counters: the counter must be allocated, have a type ID
    /// of 1000 or more (lower ones are Aeron's, see
    /// [`counter_types`](crate::counter_types)) and the given registration ID;
    /// otherwise this fails with [`ErrorKind::IllegalArgument`]. Writes through
    /// the handle do nothing once the counter is freed (see
    /// [`Counter::is_valid`]). The handle does not own the counter: dropping it
    /// does not free it, and [`Counter::is_closed`] is always `false`.
    ///
    /// Also fails if `counter_id` is out of range, and with
    /// [`ErrorKind::UnsupportedOperation`] for a reader from a [`CncFile`], which
    /// maps the counters read-only.
    pub fn counter(&self, registration_id: i64, counter_id: i32) -> Result<Counter> {
        Ok(Counter {
            inner: self
                .inner
                .counter(registration_id, counter_id, true)
                .ffi()?,
        })
    }

    /// A writable [`Counter`] handle on any counter, Aeron's included, without
    /// [`counter`](Self::counter)'s checks; writes are never refused.
    /// `registration_id` is only reported back by [`Counter::registration_id`].
    ///
    /// Fails if `counter_id` is out of range, and with
    /// [`ErrorKind::UnsupportedOperation`] for a reader from a [`CncFile`].
    ///
    /// # Safety
    ///
    /// Aeron clients in this process trust the counters the media driver
    /// allocates for them: an invalid value written to a subscriber position or
    /// publisher limit counter (e.g. a negative position) makes them read out of
    /// bounds. The counter must not be one of those, now or while the handle
    /// writes to it (a freed record can be reused for one).
    pub unsafe fn counter_unchecked(
        &self,
        registration_id: i64,
        counter_id: i32,
    ) -> Result<Counter> {
        Ok(Counter {
            inner: self
                .inner
                .counter(registration_id, counter_id, false)
                .ffi()?,
        })
    }

    /// A read-only [`CounterView`] of any counter, also from a [`CncFile`].
    /// Fails if `counter_id` is out of range.
    pub fn counter_view(&self, counter_id: i32) -> Result<CounterView> {
        Ok(CounterView {
            inner: self.inner.counterView(counter_id).ffi()?,
        })
    }

    /// Iterate over the allocated counters, calling `handler(counter_id, type_id, key_bytes, label)` for each.
    pub fn for_each<F>(&self, handler: F) -> Result<()>
    where
        F: FnMut(i32, i32, &[u8], &str),
    {
        let mut cb = Callback::new(handler);
        let result = self.inner.forEach(callback::counter::<F>, cb.ctx());
        cb.finish(result).ffi()
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
/// An added counter is freed when the `Counter` is dropped (static counters are
/// never freed). The `Counter` keeps its client open until then, even if the
/// [`AeronClient`] is dropped. If the client is closed by the driver (e.g. a
/// driver timeout), its counters are freed and [`is_closed`](Self::is_closed)
/// returns `true`. Writes do nothing once the counter's record no longer holds
/// it (see [`is_valid`](Self::is_valid)), so a freed record the driver reuses
/// for another counter is never written. Other processes see the counter
/// through their counters reader, e.g. with `AeronStat`.
///
/// Each write checks the record first (a few loads from the counter's metadata).
/// The check and the write are separate accesses: in theory the record could be
/// freed and reused between them, which needs the driver's reuse timeout
/// (about a second) to pass within one call.
///
/// `Send + Sync`. Every operation is a single atomic access except
/// [`increment_ordered`](Self::increment_ordered) and
/// [`get_and_add_ordered`](Self::get_and_add_ordered), cheaper read-then-store
/// sequences that lose updates if another thread or process writes the counter
/// concurrently: use them only on a counter with a single writer.
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

    /// The registration ID: the add's correlation ID
    /// ([`PendingAdd::registration_id`]), or the ID given to
    /// [`AeronClient::add_static_counter`].
    pub fn registration_id(&self) -> i64 {
        self.inner.registrationId()
    }

    /// The counter's record state.
    pub fn state(&self) -> CounterState {
        CounterState::from_raw(self.inner.state())
    }

    /// The counter's label. Invalid UTF-8 is replaced with `U+FFFD`.
    pub fn label(&self) -> String {
        self.inner.label()
    }

    /// Returns `true` while the counter's record still holds this counter
    /// (allocated, with the same type and registration ID). Once it is freed,
    /// e.g. dropped by its owner or after its client closed, writes through this
    /// handle do nothing. Always `true` for a handle from
    /// [`CountersReader::counter_unchecked`].
    pub fn is_valid(&self) -> bool {
        self.inner.isValid()
    }

    /// Returns `true` once the counter has been closed by its client, e.g. after
    /// a driver timeout. Always `false` for a static counter (the driver never
    /// frees it) and for a handle from [`CountersReader::counter`].
    pub fn is_closed(&self) -> bool {
        self.inner.isClosed()
    }

    /// The current value (volatile read).
    pub fn get(&self) -> i64 {
        self.inner.get()
    }

    /// The current value, without ordering guarantees (relaxed read).
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

    /// Set the value without ordering guarantees (relaxed store).
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

/// One distinct error in a media driver's error log, passed to
/// [`CncFile::read_error_log`].
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct ErrorLogEntry<'a> {
    /// How many times the error was observed.
    pub observation_count: i32,
    /// When it was first observed, in milliseconds since the epoch.
    pub first_observation_timestamp: i64,
    /// When it was last observed, in milliseconds since the epoch.
    pub last_observation_timestamp: i64,
    /// The error (invalid UTF-8 replaced with `U+FFFD`).
    pub error: &'a str,
}

/// One stream with data loss in a media driver's loss report, passed to
/// [`CncFile::read_loss_report`].
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct LossReportEntry<'a> {
    /// How many times loss was observed.
    pub observation_count: i64,
    /// The total number of bytes lost.
    pub total_bytes_lost: i64,
    /// When loss was first observed, in milliseconds since the epoch.
    pub first_observation_timestamp: i64,
    /// When loss was last observed, in milliseconds since the epoch.
    pub last_observation_timestamp: i64,
    /// The session of the stream.
    pub session_id: i32,
    /// The stream ID.
    pub stream_id: i32,
    /// The stream's channel (invalid UTF-8 replaced with `U+FFFD`).
    pub channel: &'a str,
    /// The source the loss was observed from (invalid UTF-8 replaced with `U+FFFD`).
    pub source: &'a str,
}

/// The constants of a CnC file (C `aeron_cnc_constants_t`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct CncConstants {
    /// The CnC format version (semantic version as `major << 16 | minor << 8 | patch`).
    pub cnc_version: i32,
    /// Length of the clients-to-driver command buffer.
    pub to_driver_buffer_length: usize,
    /// Length of the driver-to-clients broadcast buffer.
    pub to_clients_buffer_length: usize,
    /// Length of the counters metadata buffer.
    pub counter_metadata_buffer_length: usize,
    /// Length of the counters values buffer.
    pub counter_values_buffer_length: usize,
    /// Length of the error log buffer.
    pub error_log_buffer_length: usize,
    /// How long the driver waits for a client keepalive before closing it.
    pub client_liveness_timeout: Duration,
    /// When the driver started, in milliseconds since the epoch.
    pub start_timestamp: i64,
    /// The process ID of the driver.
    pub pid: i64,
    /// The page size used for the file.
    pub file_page_size: usize,
}

/// The command-and-control (CnC) file of a media driver, read without
/// connecting a client (C++ `aeron::CncFileReader`): the driver's counters, its
/// error log and its liveness, as used by tools like `AeronStat` and `ErrorStat`.
///
/// Mapping the file does not mean the driver is running: a driver that stopped
/// without deleting its directory leaves the file behind. Check
/// [`is_driver_active`](Self::is_driver_active).
///
/// The file is mapped read-only. `Send + Sync`.
///
/// ```no_run
/// use aeron_glide::CncFile;
/// use std::time::Duration;
///
/// let cnc = CncFile::map_existing("/dev/shm/aeron")?;
/// println!("driver active: {}", cnc.is_driver_active(Duration::from_secs(10)));
/// cnc.counters_reader().for_each(|id, _, _, label| {
///     println!("{id}: {label}");
/// })?;
/// cnc.read_error_log(0, |entry| {
///     println!("{} observations: {}", entry.observation_count, entry.error);
/// })?;
/// # Ok::<(), aeron_glide::Error>(())
/// ```
pub struct CncFile {
    inner: cxx::UniquePtr<ffi::CncFileWrapper>,
}

// SAFETY: the wrapper only reads its read-only mapping (written by the media
// driver concurrently anyway), and every bridged method is const. It is shared
// with its counters readers through a `shared_ptr` (atomic reference count).
unsafe impl Send for CncFile {}
unsafe impl Sync for CncFile {}

impl std::fmt::Debug for CncFile {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CncFile")
            .field("file_name", &self.file_name())
            .finish_non_exhaustive()
    }
}

impl CncFile {
    /// Map the CnC file in `aeron_dir`, waiting up to 10 seconds for it to exist
    /// and be initialised by a media driver (C++ `CncFileReader::mapExisting`).
    /// Fails with [`ErrorKind::Io`] if it does not, or at once if the file's
    /// version is incompatible or its layout is invalid.
    pub fn map_existing(aeron_dir: impl AsRef<std::path::Path>) -> Result<Self> {
        Self::map_existing_with_timeout(aeron_dir, Duration::from_secs(10))
    }

    /// [`map_existing`](Self::map_existing), waiting up to `timeout` (zero: fail
    /// at once if the file is not ready).
    pub fn map_existing_with_timeout(
        aeron_dir: impl AsRef<std::path::Path>,
        timeout: Duration,
    ) -> Result<Self> {
        let aeron_dir = crate::error::path_str(aeron_dir.as_ref())?;
        // The C client adds it to the clock: clamp far below overflow.
        let timeout_ms = crate::timeout_millis(timeout);
        Ok(Self {
            inner: ffi::mapCncFile(aeron_dir, timeout_ms).ffi()?,
        })
    }

    /// The path of the mapped file.
    pub fn file_name(&self) -> String {
        self.inner.fileName()
    }

    /// The file's constants: buffer lengths, the driver's PID and start time, ...
    pub fn constants(&self) -> Result<CncConstants> {
        let c = self.inner.constants().ffi()?;
        let length = |n: i32| usize::try_from(n).unwrap_or(0);
        Ok(CncConstants {
            cnc_version: c.cnc_version,
            to_driver_buffer_length: length(c.to_driver_buffer_length),
            to_clients_buffer_length: length(c.to_clients_buffer_length),
            counter_metadata_buffer_length: length(c.counter_metadata_buffer_length),
            counter_values_buffer_length: length(c.counter_values_buffer_length),
            error_log_buffer_length: length(c.error_log_buffer_length),
            client_liveness_timeout: Duration::from_nanos(
                u64::try_from(c.client_liveness_timeout_ns).unwrap_or(0),
            ),
            start_timestamp: c.start_timestamp_ms,
            pid: c.pid,
            file_page_size: length(c.file_page_size),
        })
    }

    /// When the driver last showed it is alive (its consumer heartbeat on the
    /// command buffer), in milliseconds since the epoch; 0 if it never has.
    pub fn to_driver_heartbeat(&self) -> i64 {
        self.inner.toDriverHeartbeat()
    }

    /// Returns `true` if the driver's heartbeat is at most `timeout` old (Java
    /// `CommonContext.isDriverActive`), i.e. a driver is running in this
    /// directory. Clients use their driver timeout (10 seconds by default).
    pub fn is_driver_active(&self, timeout: Duration) -> bool {
        let heartbeat = self.to_driver_heartbeat();
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX));
        let timeout = i64::try_from(timeout.as_millis()).unwrap_or(i64::MAX);
        heartbeat > 0 && now <= heartbeat.saturating_add(timeout)
    }

    /// A reader for the driver's counters. It keeps the file mapped.
    pub fn counters_reader(&self) -> CountersReader {
        CountersReader {
            inner: self.inner.countersReader(),
        }
    }

    /// Read the driver's distinct-error log: `consumer` is called for each
    /// distinct error last observed at or after `since_timestamp` (milliseconds
    /// since the epoch; 0 for all). Returns the number of errors read. An error
    /// the driver is still recording (no observation yet) is skipped.
    pub fn read_error_log<F>(&self, since_timestamp: i64, consumer: F) -> Result<usize>
    where
        F: FnMut(&ErrorLogEntry<'_>),
    {
        let mut cb = Callback::new(consumer);
        let result = self
            .inner
            .readErrorLog(callback::error_log::<F>, cb.ctx(), since_timestamp);
        Ok(cb.finish(result).ffi()?.max(0) as usize)
    }

    /// Read the driver's loss report (`loss-report.dat`, next to the CnC file,
    /// as Aeron's `LossStat` tool does): `consumer` is called for each stream
    /// that lost data. Returns the number of entries read.
    ///
    /// The file is checked as it is read, so a corrupt one ends the report
    /// early instead of being read out of bounds. Fails if the file cannot be
    /// mapped (e.g. the driver is gone and its directory deleted).
    pub fn read_loss_report<F>(&self, consumer: F) -> Result<usize>
    where
        F: FnMut(&LossReportEntry<'_>),
    {
        let mut cb = Callback::new(consumer);
        let result = self
            .inner
            .readLossReport(callback::loss_report::<F>, cb.ctx());
        Ok(cb.finish(result).ffi()?.max(0) as usize)
    }
}

/// Client liveness through heartbeat counters (C++ `aeron::HeartbeatTimestamp`).
///
/// Each client has a heartbeat counter of type [`CLIENT_HEARTBEAT_TYPE_ID`](heartbeat_timestamp::CLIENT_HEARTBEAT_TYPE_ID)
/// whose key is its client ID ([`AeronClient::client_id`]). The driver allocates
/// it when the client first adds a resource (a keepalive alone does not), and
/// frees it when the client closes or times out.
pub mod heartbeat_timestamp {
    use super::*;

    /// Counter type ID of a client heartbeat timestamp
    /// ([`DRIVER_HEARTBEAT_TYPE_ID`](crate::counter_types::DRIVER_HEARTBEAT_TYPE_ID)).
    pub const CLIENT_HEARTBEAT_TYPE_ID: i32 = crate::counter_types::DRIVER_HEARTBEAT_TYPE_ID;

    /// The ID of the allocated heartbeat counter of type `counter_type_id` whose
    /// key holds `registration_id` (e.g. a client ID), if any.
    pub fn find_counter_id_by_registration_id(
        reader: &CountersReader,
        counter_type_id: i32,
        registration_id: i64,
    ) -> Option<i32> {
        reader
            .inner
            .findHeartbeatCounterId(counter_type_id, registration_id)
            .ok()
            .and_then(found)
    }

    /// Returns `true` if `counter_id` is still the allocated heartbeat counter of
    /// type `counter_type_id` for `registration_id` (`false` for an out-of-range
    /// ID).
    pub fn is_active(
        reader: &CountersReader,
        counter_id: i32,
        counter_type_id: i32,
        registration_id: i64,
    ) -> bool {
        reader
            .inner
            .isHeartbeatActive(counter_id, counter_type_id, registration_id)
            .unwrap_or(false)
    }
}

/// A read-only handle on any counter, from [`CountersReader::counter_view`]:
/// its identity, label and value. `Send + Sync`.
pub struct CounterView {
    inner: cxx::UniquePtr<ffi::CounterWrapper>,
}

impl Drop for CounterView {
    fn drop(&mut self) {
        callback::drop_outside_conductor(&mut self.inner);
    }
}

// SAFETY: as for Counter; a view only reads.
unsafe impl Send for CounterView {}
unsafe impl Sync for CounterView {}

impl std::fmt::Debug for CounterView {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CounterView")
            .field("id", &self.id())
            .field("registration_id", &self.registration_id())
            .field("value", &self.get())
            .finish()
    }
}

impl CounterView {
    /// The counter's ID.
    pub fn id(&self) -> i32 {
        self.inner.id()
    }

    /// The counter's registration ID when the view was created.
    pub fn registration_id(&self) -> i64 {
        self.inner.registrationId()
    }

    /// The counter's record state.
    pub fn state(&self) -> CounterState {
        CounterState::from_raw(self.inner.state())
    }

    /// The counter's label. Invalid UTF-8 is replaced with `U+FFFD`.
    pub fn label(&self) -> String {
        self.inner.label()
    }

    /// Returns `true` while the record still holds the counter the view was
    /// created for (once it is freed and reused, the view reads another one).
    pub fn is_valid(&self) -> bool {
        self.inner.isValid()
    }

    /// The current value (volatile read).
    pub fn get(&self) -> i64 {
        self.inner.get()
    }

    /// The current value, without ordering guarantees (relaxed read).
    pub fn get_weak(&self) -> i64 {
        self.inner.getWeak()
    }
}

/// The state of a counter's record in a [`CountersReader`] (C++
/// `CountersReader::RECORD_*`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum CounterState {
    /// Never allocated (`RECORD_UNUSED`, 0).
    Unused,
    /// Allocated (`RECORD_ALLOCATED`, 1).
    Allocated,
    /// Freed, waiting to be reused (`RECORD_RECLAIMED`, -1).
    Reclaimed,
    /// A value this version does not know.
    Other(i32),
}

impl CounterState {
    /// The state for Aeron's raw value.
    pub fn from_raw(state: i32) -> Self {
        match state {
            0 => Self::Unused,
            1 => Self::Allocated,
            -1 => Self::Reclaimed,
            other => Self::Other(other),
        }
    }

    /// Aeron's raw value.
    pub fn as_raw(self) -> i32 {
        match self {
            Self::Unused => 0,
            Self::Allocated => 1,
            Self::Reclaimed => -1,
            Self::Other(state) => state,
        }
    }
}
