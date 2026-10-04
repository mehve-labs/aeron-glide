// Archive client shim (the `archive` feature): wrappers over the Aeron C++
// archive client (aeron::archive::client), bridged in src/archive/mod.rs.
#pragma once
#include <array>
#include "shim.h"
#include <AeronCounters.h>
#include <client/archive/PersistentSubscription.h>
#include <client/archive/RecordingPos.h>
extern "C" {
#include <client/aeron_archive_async_connect.h>
}

namespace aeron_rs {

namespace arc = aeron::archive::client;

// Shared with Rust (src/archive/mod.rs).
struct RecordingDescriptorInfo;
struct RecordingSubscriptionInfo;
struct RecordingSignalInfo;
struct ArchiveContextInfo;

using RecordingDescriptorFn = rust::Fn<void(size_t, const RecordingDescriptorInfo &)>;
using RecordingSubscriptionFn = rust::Fn<void(size_t, const RecordingSubscriptionInfo &)>;
using RecordingSignalFn = rust::Fn<void(size_t, const RecordingSignalInfo &)>;
// (ctx, work count)
using IdleFn = rust::Fn<void(size_t, int32_t)>;
using InvokerFn = rust::Fn<void(size_t)>;
// (ctx) -> encoded credentials; (ctx, challenge) -> encoded credentials
using CredentialsFn = rust::Fn<rust::Vec<uint8_t>(size_t)>;
using ChallengeFn = rust::Fn<rust::Vec<uint8_t>(size_t, rust::Slice<const uint8_t>)>;
// (ctx, error code, message)
using PersistentErrorFn = rust::Fn<void(size_t, int32_t, rust::Slice<const uint8_t>)>;

// archive::Context::idleStrategy takes an object with `idle(int)` by reference,
// so it must outlive the archive.
class IdleStrategy {
public:
    virtual ~IdleStrategy() = default;
    virtual void idle(int workCount) = 0;
};

// Calls a Rust idle strategy.
class RustIdleStrategy : public IdleStrategy {
public:
    RustIdleStrategy(IdleFn idle, ReleaseFn release, size_t ctx) : idle_(idle), owner_(release, ctx) {}
    void idle(int workCount) override { idle_(owner_.ctx(), workCount); }

private:
    IdleFn idle_;
    RustOwned owner_;
};

// Runs an agent invoker client's conductor, then idles. Upstream (1.53.3): some
// archive client wait loops (adding recorded publications, replay
// subscriptions) only call the idle strategy, never the client's conductor, so
// with an agent invoker client they would wait forever. Requests hold the
// client's conductor lock, so the conductor runs on the waiting thread.
class InvokingIdleStrategy : public IdleStrategy {
public:
    InvokingIdleStrategy(std::shared_ptr<aeron::Aeron> aeron, std::shared_ptr<IdleStrategy> next)
        : aeron_(std::move(aeron)), next_(std::move(next)) {}
    void idle(int workCount) override {
        int work = aeron_->conductorAgentInvoker().invoke();
        if (next_) {
            next_->idle(workCount + work);
        } else {
            yielding_.idle(workCount + work);
        }
    }

private:
    std::shared_ptr<aeron::Aeron> aeron_;
    std::shared_ptr<IdleStrategy> next_;
    aeron::concurrent::YieldingIdleStrategy yielding_;
};

// Configuration for an archive connection or a persistent subscription
// (aeron::archive::client::Context). Settings are applied to the C++ context
// as they are set.
class ArchiveContextWrapper {
public:
    ArchiveContextWrapper();

    void setAeron(const AeronWrapper &client);
    void setAeronDirectoryName(rust::Str name);
    void setControlRequestChannel(rust::Str channel);
    void setControlRequestStreamId(int32_t stream_id);
    void setControlResponseChannel(rust::Str channel);
    void setControlResponseStreamId(int32_t stream_id);
    void setRecordingEventsChannel(rust::Str channel);
    void setMessageTimeoutNs(int64_t timeout_ns);
    void setMessageRetryAttempts(uint32_t attempts);
    void setMaxErrorMessageLength(uint32_t length);
    // Each takes ownership of its ctx (released with the last copy of the handler).
    void setIdleStrategy(IdleFn idle, ReleaseFn release, size_t ctx);
    void setDelegatingInvoker(InvokerFn invoker, ReleaseFn release, size_t ctx);
    void setErrorHandler(ErrorFn handler, ReleaseFn release, size_t ctx);
    void setRecordingSignalConsumer(RecordingSignalFn consumer, ReleaseFn release, size_t ctx);
    // One ctx owns both closures.
    void setCredentialsSupplier(CredentialsFn credentials, ChallengeFn on_challenge, ReleaseFn release, size_t ctx);

    // Fills in the client (an internal one with a non-exiting error handler if
    // none was set: the archive C client would otherwise create one with
    // Aeron's default handler, which exit()s).
    void conclude();
    // Installs the delegating invoker (the user's, if any, see installInvoker).
    void installInvoker();
    std::function<void()> userInvoker;

    std::shared_ptr<arc::Context> ctx;
    std::shared_ptr<ConductorLock> lock;
    std::shared_ptr<IdleStrategy> idle;
};

// A connected archive client. Every operation runs under the client's conductor
// lock (the archive drives the client's conductor in agent invoker mode).
class ArchiveWrapper {
public:
    ArchiveWrapper(std::shared_ptr<arc::AeronArchive> archive, std::shared_ptr<ConductorLock> lock,
                   std::shared_ptr<IdleStrategy> idle);
    ~ArchiveWrapper();

    ArchiveContextInfo context() const;
    int64_t archiveId() const;
    int64_t controlSessionId() const;
    int32_t pollForRecordingSignals() const;
    rust::String pollForErrorResponse() const;
    void checkForErrorResponse() const;

    // Recording (A3, A4)
    std::unique_ptr<PublicationWrapper> addRecordedPublication(rust::Str channel, int32_t stream_id) const;
    std::unique_ptr<ExclusivePublicationWrapper> addRecordedExclusivePublication(rust::Str channel, int32_t stream_id) const;
    int64_t startRecording(rust::Str channel, int32_t stream_id, int32_t source_location, bool auto_stop) const;
    int64_t extendRecording(int64_t recording_id, rust::Str channel, int32_t stream_id, int32_t source_location,
                            bool auto_stop) const;
    void stopRecording(int64_t subscription_id) const;
    bool tryStopRecording(int64_t subscription_id) const;
    void stopRecordingByChannelAndStream(rust::Str channel, int32_t stream_id) const;
    bool tryStopRecordingByChannelAndStream(rust::Str channel, int32_t stream_id) const;
    bool tryStopRecordingByIdentity(int64_t recording_id) const;
    void stopRecordingPublication(const PublicationWrapper &publication) const;
    void stopRecordingExclusivePublication(const ExclusivePublicationWrapper &publication) const;
    int64_t purgeRecording(int64_t recording_id) const;
    void updateChannel(int64_t recording_id, rust::Str channel) const;
    int64_t truncateRecording(int64_t recording_id, int64_t position) const;

    // Positions and queries (A5)
    int64_t getRecordingPosition(int64_t recording_id) const;
    int64_t getStartPosition(int64_t recording_id) const;
    int64_t getStopPosition(int64_t recording_id) const;
    int64_t getMaxRecordedPosition(int64_t recording_id) const;
    int64_t findLastMatchingRecording(int64_t min_recording_id, rust::Str channel_fragment, int32_t stream_id,
                                      int32_t session_id) const;
    int32_t listRecording(int64_t recording_id, RecordingDescriptorFn handler, size_t ctx) const;
    int32_t listRecordings(int64_t from_recording_id, int32_t record_count, RecordingDescriptorFn handler,
                           size_t ctx) const;
    int32_t listRecordingsForUri(int64_t from_recording_id, int32_t record_count, rust::Str channel_fragment,
                                 int32_t stream_id, RecordingDescriptorFn handler, size_t ctx) const;
    int32_t listRecordingSubscriptions(int32_t pseudo_index, int32_t subscription_count, rust::Str channel_fragment,
                                       int32_t stream_id, bool apply_stream_id, RecordingSubscriptionFn handler,
                                       size_t ctx) const;

    // Replay (A7). Params: position, length, bounding limit counter id, file io
    // max length, replay token, subscription registration id.
    int64_t startReplay(int64_t recording_id, rust::Str channel, int32_t stream_id, int64_t position, int64_t length,
                        int32_t bounding_limit_counter_id, int32_t file_io_max_length, int64_t replay_token,
                        int64_t subscription_registration_id) const;
    std::unique_ptr<SubscriptionWrapper> replay(int64_t recording_id, rust::Str channel, int32_t stream_id,
                                                int64_t position, int64_t length, int32_t bounding_limit_counter_id,
                                                int32_t file_io_max_length, int64_t replay_token,
                                                int64_t subscription_registration_id) const;
    void stopReplay(int64_t replay_session_id) const;
    void stopAllReplays(int64_t recording_id) const;

    // Replication (A8)
    int64_t replicate(int64_t src_recording_id, int32_t src_control_stream_id, rust::Str src_control_channel,
                      int64_t stop_position, int64_t dst_recording_id, rust::Str live_destination,
                      rust::Str replication_channel, int64_t channel_tag_id, int64_t subscription_tag_id,
                      int32_t file_io_max_length, int32_t replication_session_id,
                      rust::Slice<const uint8_t> encoded_credentials) const;
    void stopReplication(int64_t replication_id) const;
    bool tryStopReplication(int64_t replication_id) const;

    // Segments (A9)
    void detachSegments(int64_t recording_id, int64_t new_start_position) const;
    int64_t deleteDetachedSegments(int64_t recording_id) const;
    int64_t purgeSegments(int64_t recording_id, int64_t new_start_position) const;
    int64_t attachSegments(int64_t recording_id) const;
    int64_t migrateSegments(int64_t src_recording_id, int64_t dst_recording_id) const;

    // For ReplayMerge (not bridged).
    const std::shared_ptr<arc::AeronArchive> &sharedArchive() const { return archive_; }
    const std::shared_ptr<IdleStrategy> &sharedIdle() const { return idle_; }
    const std::shared_ptr<ConductorLock> &conductorLock() const { return lock_; }

private:
    std::shared_ptr<arc::AeronArchive> archive_;
    std::shared_ptr<ConductorLock> lock_;
    // The idle strategy the archive's context refers to.
    std::shared_ptr<IdleStrategy> idle_;
};

std::unique_ptr<ArchiveContextWrapper> create_archive_context();
// Blocking connect (A1). The context must stay alive until it returns.
std::unique_ptr<ArchiveWrapper> archive_connect(ArchiveContextWrapper &context);

// Asynchronous connect (A2). Owns the context: its callbacks run while connecting.
class ArchiveAsyncConnectWrapper {
public:
    explicit ArchiveAsyncConnectWrapper(std::unique_ptr<ArchiveContextWrapper> context);
    ~ArchiveAsyncConnectWrapper();
    // Null while connecting.
    std::unique_ptr<ArchiveWrapper> poll();

private:
    std::unique_ptr<ArchiveContextWrapper> context_;
    std::shared_ptr<arc::AeronArchive::AsyncConnect> async_;
};

std::unique_ptr<ArchiveAsyncConnectWrapper> archive_async_connect(std::unique_ptr<ArchiveContextWrapper> context);


// RecordingPos (A10).
int32_t recordingPosFindCounterIdByRecordingId(const CountersReaderWrapper &reader, int64_t recording_id);
int32_t recordingPosFindCounterIdBySessionId(const CountersReaderWrapper &reader, int32_t session_id);
int64_t recordingPosGetRecordingId(const CountersReaderWrapper &reader, int32_t counter_id);
rust::String recordingPosGetSourceIdentity(const CountersReaderWrapper &reader, int32_t counter_id);
bool recordingPosIsActive(const CountersReaderWrapper &reader, int32_t counter_id, int64_t recording_id);

class ReplayMergeWrapper {
public:
    ReplayMergeWrapper(const std::shared_ptr<aeron::Subscription> &subscription,
                       std::shared_ptr<AssemblerState> assembly,
                       const std::shared_ptr<arc::AeronArchive> &archive, std::shared_ptr<IdleStrategy> idle,
                       const std::string &replayChannel,
                       const std::string &replayDestination, const std::string &liveDestination,
                       int64_t recordingId, int64_t startPosition, int64_t mergeProgressTimeoutMs);
    ~ReplayMergeWrapper();

    int doWork();
    int poll(int fragment_limit, FragmentFn handler, size_t ctx);
    std::unique_ptr<ImageWrapper> image();
    bool isMerged() const;
    bool hasFailed() const;
    bool isLiveAdded() const;

private:
    std::shared_ptr<aeron::Subscription> subscription_;
    std::shared_ptr<AssemblerState> assembly_;
    // The archive's idle strategy: the merge calls it, and keeps the archive
    // alive, past the ArchiveWrapper.
    std::shared_ptr<IdleStrategy> idle_;
    std::unique_ptr<arc::ReplayMerge> merge_;
};

std::unique_ptr<ReplayMergeWrapper> create_replay_merge(
    SubscriptionWrapper &subscription, const ArchiveWrapper &archive, rust::Str replay_channel,
    rust::Str replay_destination, rust::Str live_destination, int64_t recording_id, int64_t start_position,
    int64_t merge_progress_timeout_ms);

// PersistentSubscription (A11).
class PersistentSubscriptionContextWrapper {
public:
    PersistentSubscriptionContextWrapper();

    // Takes the archive context (settings and handlers) over.
    void setArchiveContext(std::unique_ptr<ArchiveContextWrapper> archive);
    void setAeron(const AeronWrapper &client);
    void setAeronDirectoryName(rust::Str name);
    void setRecordingId(int64_t recording_id);
    void setStartPosition(int64_t position);
    void setLiveChannel(rust::Str channel);
    void setLiveStreamId(int32_t stream_id);
    void setReplayChannel(rust::Str channel);
    void setReplayStreamId(int32_t stream_id);
    // 0 = state, 1 = join difference, 2 = live left, 3 = live joined.
    void setCounter(int32_t which, const CounterWrapper &counter);
    void setOnLiveJoined(InvokerFn callback, ReleaseFn release, size_t ctx);
    void setOnLiveLeft(InvokerFn callback, ReleaseFn release, size_t ctx);
    void setOnError(PersistentErrorFn callback, ReleaseFn release, size_t ctx);

    arc::PersistentSubscription::Context ctx;
    std::unique_ptr<ArchiveContextWrapper> archive;
    std::shared_ptr<ConductorLock> lock;
    bool hasClient = false;
    std::shared_ptr<aeron::Aeron> client;
    std::string aeronDir;
    // The counters handed over, one per slot (state, join difference, live
    // left, live joined): the C context closes them, so their C++ handles must
    // not close them again. Setting a slot again replaces its counter, which is
    // then closed as usual.
    std::array<std::shared_ptr<aeron::Counter>, 4> counters;
    // What the counters' handles keep alive (see CounterWrapper).
    std::array<std::shared_ptr<const void>, 4> keepalive;
};

class PersistentSubscriptionWrapper {
public:
    PersistentSubscriptionWrapper(std::shared_ptr<arc::PersistentSubscription> subscription,
                                  std::unique_ptr<PersistentSubscriptionContextWrapper> context);
    ~PersistentSubscriptionWrapper();

    int poll(int fragment_limit, FragmentFn handler, size_t ctx);
    int controlledPoll(int fragment_limit, ControlledFragmentFn handler, size_t ctx);
    bool isLive() const;
    bool isReplaying() const;
    bool hasFailed() const;
    // Empty unless failed; then `code` is set.
    rust::String failureReason(int32_t &code) const;

private:
    std::shared_ptr<arc::PersistentSubscription> subscription_;
    std::unique_ptr<PersistentSubscriptionContextWrapper> context_;
};

std::unique_ptr<PersistentSubscriptionContextWrapper> create_persistent_subscription_context();
std::unique_ptr<PersistentSubscriptionWrapper> create_persistent_subscription(
    std::unique_ptr<PersistentSubscriptionContextWrapper> context);

} // namespace aeron_rs
