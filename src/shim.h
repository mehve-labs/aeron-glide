#pragma once
#include <cstring>
#include <deque>
#include <exception>
#include <memory>
#include <string>
#include <Aeron.h>
#include <ControlledFragmentAssembler.h>
#include <ImageControlledFragmentAssembler.h>
#include "rust/cxx.h"

#ifdef AERON_ARCHIVE
#include <client/archive/AeronArchive.h>
#include <client/archive/ArchiveContext.h>
#include <client/archive/ReplayParams.h>
#include <client/archive/ReplayMerge.h>
#endif

namespace aeron_rs {
namespace detail {

// Encodes an exception as "aeron-glide<RS>kind<RS>code<RS>message", decoded by
// `Error::from(cxx::Exception)` in src/error.rs. Most-derived classes first.
inline std::string encode_exception(const std::exception &e) {
    using namespace aeron::util;
    const char *kind = "other";
    std::int32_t code = 0;
    if (auto *s = dynamic_cast<const SourcedException *>(&e)) {
        code = s->errorCode();
        if (dynamic_cast<const RegistrationException *>(&e)) kind = "registration";
        else if (dynamic_cast<const TimeoutException *>(&e)) kind = "timeout";
        else if (dynamic_cast<const ChannelEndpointException *>(&e)) kind = "channel_endpoint";
        else if (dynamic_cast<const IllegalArgumentException *>(&e)) kind = "illegal_argument";
        else if (dynamic_cast<const IllegalStateException *>(&e)) kind = "illegal_state";
        else if (dynamic_cast<const IOException *>(&e)) kind = "io";
        else if (dynamic_cast<const FormatException *>(&e)) kind = "format";
        else if (dynamic_cast<const OutOfBoundsException *>(&e)) kind = "out_of_bounds";
        else if (dynamic_cast<const ParseException *>(&e)) kind = "parse";
        else if (dynamic_cast<const ElementNotFound *>(&e)) kind = "element_not_found";
        else if (dynamic_cast<const DriverTimeoutException *>(&e)) kind = "driver_timeout";
        else if (dynamic_cast<const ConductorServiceTimeoutException *>(&e)) kind = "conductor_service_timeout";
        else if (dynamic_cast<const ClientTimeoutException *>(&e)) kind = "client_timeout";
        else if (dynamic_cast<const UnknownSubscriptionException *>(&e)) kind = "unknown_subscription";
        else if (dynamic_cast<const ReentrantException *>(&e)) kind = "reentrant";
        else if (dynamic_cast<const UnsupportedOperationException *>(&e)) kind = "unsupported_operation";
#ifdef AERON_ARCHIVE
        else if (dynamic_cast<const ArchiveException *>(&e)) kind = "archive";
#endif
        else kind = "aeron";
        // The C++ wrapper maps only the negated client error codes; the conductor's
        // error handler reports them positive (e.g. "MediaDriver has been shutdown"
        // arrives as a plain AeronException with code 1000). Classify by code too.
        if (std::strcmp(kind, "aeron") == 0) {
            switch (code < 0 ? -code : code) {
                case AERON_CLIENT_ERROR_DRIVER_TIMEOUT: kind = "driver_timeout"; break;
                case AERON_CLIENT_ERROR_CLIENT_TIMEOUT: kind = "client_timeout"; break;
                case AERON_CLIENT_ERROR_CONDUCTOR_SERVICE_TIMEOUT: kind = "conductor_service_timeout"; break;
                case AERON_CLIENT_ERROR_BUFFER_FULL:
                case AERON_CLIENT_ERROR_DRIVER_BUFFER_FULL: kind = "illegal_state"; break;
                default: break;
            }
        }
    } else if (dynamic_cast<const std::out_of_range *>(&e)) {
        kind = "out_of_bounds";
    } else if (dynamic_cast<const std::invalid_argument *>(&e)) {
        kind = "illegal_argument";
    }
    std::string out("aeron-glide\x1e");
    out += kind;
    out += '\x1e';
    out += std::to_string(code);
    out += '\x1e';
    out += e.what();
    return out;
}

} // namespace detail
} // namespace aeron_rs

// Custom cxx exception conversion: every bridged `Result` function reports the
// Aeron exception class and error code, not just the message.
namespace rust {
namespace behavior {
template <typename Try, typename Fail>
static void trycatch(Try &&func, Fail &&fail) noexcept try {
    func();
} catch (const std::exception &e) {
    fail(aeron_rs::detail::encode_exception(e).c_str());
} catch (...) {
    fail("aeron-glide\x1eother\x1e" "0\x1eunknown C++ exception");
}
} // namespace behavior
} // namespace rust

// Forward declarations for C driver types (defined in aeronmd.h)
extern "C" {
    struct aeron_driver_context_stct;
    typedef struct aeron_driver_context_stct aeron_driver_context_t;
    struct aeron_driver_stct;
    typedef struct aeron_driver_stct aeron_driver_t;
}

namespace aeron_rs {

// Rust trampolines (see src/callback.rs). The size_t is the opaque closure context.
using FragmentFn = rust::Fn<void(size_t, rust::Slice<const uint8_t>)>;
using ControlledFragmentFn = rust::Fn<int32_t(size_t, rust::Slice<const uint8_t>)>;
using ClaimFn = rust::Fn<bool(size_t, rust::Slice<uint8_t>)>;
using CounterFn = rust::Fn<void(size_t, int32_t, int32_t, rust::Slice<const uint8_t>, rust::Slice<const uint8_t>)>;
// Long-lived handler: (ctx, encoded exception). The release function frees ctx.
using ErrorFn = rust::Fn<void(size_t, rust::Slice<const uint8_t>)>;
using ReleaseFn = rust::Fn<void(size_t)>;

// Client configuration, applied to aeron::Context before connecting.
class ContextWrapper {
public:
    ContextWrapper();
    ~ContextWrapper();

    void setAeronDir(rust::Str dir);
    void setClientName(rust::Str name);
    void setDriverTimeoutMs(int64_t value);
    void setResourceLingerTimeoutMs(int64_t value);
    void setIdleSleepDurationMs(int64_t value);
    void setPreTouchMappedMemory(bool value);
    // Takes ownership of `ctx`: `release(ctx)` runs when the last copy of the
    // handler is destroyed, i.e. when the C++ client is destroyed.
    void setErrorHandler(ErrorFn handler, ReleaseFn release, size_t ctx);

    std::shared_ptr<aeron::Context> ctx;
};

// Throws the Aeron exception matching aeron_errcode(), prefixed with `what`.
[[noreturn]] void throwDriverError(const char *what);

// Settings are applied by the generated driver_gen.h functions.
class MediaDriverWrapper {
public:
    MediaDriverWrapper();
    ~MediaDriverWrapper();

    void start();
    // Throws IllegalStateException once started: the driver's threads read the context.
    void ensureNotStarted() const;
    aeron_driver_context_t *context() const { return context_; }
    // A copy of `value` that lives as long as this wrapper: some C setters keep
    // the pointer they are given instead of copying the string.
    const char *keep(rust::Str value) {
        strings_.emplace_back(value.data(), value.size());
        return strings_.back().c_str();
    }

    void setThreadingMode(int32_t mode);

private:
    aeron_driver_context_t* context_;
    aeron_driver_t* driver_;
    std::deque<std::string> strings_; // destroyed after the driver and context are closed
};

class PublicationWrapper {
public:
    PublicationWrapper(std::shared_ptr<aeron::Publication> pub);
    ~PublicationWrapper();
    
    // const: a concurrent publication may be used from several threads at once.
    int64_t offer(rust::Slice<const uint8_t> buffer) const;
    int64_t tryClaim(size_t length, ClaimFn handler, size_t ctx) const;
    bool isConnected() const;
    int32_t sessionId() const;

private:
    std::shared_ptr<aeron::Publication> pub;
};

class ExclusivePublicationWrapper {
public:
    ExclusivePublicationWrapper(std::shared_ptr<aeron::ExclusivePublication> pub);
    ~ExclusivePublicationWrapper();

    int64_t offer(rust::Slice<const uint8_t> buffer);
    int64_t tryClaim(size_t length, ClaimFn handler, size_t ctx);
    bool isConnected() const;

private:
    std::shared_ptr<aeron::ExclusivePublication> pub;
};

class ImageWrapper; // forward declaration

class SubscriptionWrapper {
public:
    SubscriptionWrapper(std::shared_ptr<aeron::Subscription> sub);
    ~SubscriptionWrapper();

    int poll(int fragment_limit, FragmentFn handler, size_t ctx);
    int controlledPollAssembled(int fragment_limit, ControlledFragmentFn handler, size_t ctx);
    bool isConnected() const;
    bool deleteSessionBuffer(int32_t session_id);

    // Image accessors
    int imageCount() const;
    std::unique_ptr<ImageWrapper> imageByIndex(size_t index) const;
    std::unique_ptr<ImageWrapper> imageBySessionId(int32_t session_id) const;

    // Internal accessor for ReplayMerge (not exposed through cxx)
    const std::shared_ptr<aeron::Subscription>& sharedSubscription() const { return sub; }

private:
    std::shared_ptr<aeron::Subscription> sub;
    // Handler of the controlledPollAssembled call in progress, used by the assembler.
    const ControlledFragmentFn *controlled_handler_ = nullptr;
    size_t controlled_ctx_ = 0;
    aeron::ControlledFragmentAssembler controlled_assembler_;
};

class ImageWrapper {
public:
    // `subscription` owns the image: aeron::Image only holds raw C pointers, so the
    // subscription (and through it the client) must outlive it.
    ImageWrapper(std::shared_ptr<aeron::Image> image, std::shared_ptr<aeron::Subscription> subscription);
    ~ImageWrapper();

    // Metadata
    int32_t sessionId() const;
    int64_t correlationId() const;
    int64_t joinPosition() const;
    rust::String sourceIdentity() const;

    // Position
    int64_t position() const;

    // State
    bool isClosed() const;
    bool isEndOfStream() const;
    int64_t endOfStreamPosition() const;

    // Polling
    int poll(int fragment_limit, FragmentFn handler, size_t ctx);
    int controlledPollAssembled(int fragment_limit, ControlledFragmentFn handler, size_t ctx);

private:
    std::shared_ptr<aeron::Subscription> subscription_;
    std::shared_ptr<aeron::Image> image_;
    // Handler of the controlledPollAssembled call in progress, used by the assembler.
    const ControlledFragmentFn *controlled_handler_ = nullptr;
    size_t controlled_ctx_ = 0;
    aeron::ImageControlledFragmentAssembler controlled_assembler_;
};

class CountersReaderWrapper {
public:
    CountersReaderWrapper(std::shared_ptr<aeron::Aeron> aeron);
    ~CountersReaderWrapper();

    int32_t maxCounterId() const;
    int64_t getCounterValue(int32_t id) const;
    int32_t getCounterState(int32_t id) const;
    int32_t getCounterTypeId(int32_t id) const;
    rust::String getCounterLabel(int32_t id) const;
    void forEach(CounterFn handler, size_t ctx) const;

private:
    std::shared_ptr<aeron::Aeron> aeron;
};

class AeronWrapper {
public:
    AeronWrapper(std::shared_ptr<ContextWrapper> context);
    ~AeronWrapper();
    
    void start() const;
    bool isClosed() const;
    
    // const: aeron::Aeron is thread-safe for adding resources.
    std::unique_ptr<PublicationWrapper> addPublication(rust::Str channel, int32_t stream_id) const;
    std::unique_ptr<ExclusivePublicationWrapper> addExclusivePublication(rust::Str channel, int32_t stream_id) const;
    std::unique_ptr<SubscriptionWrapper> addSubscription(rust::Str channel, int32_t stream_id) const;
    std::unique_ptr<CountersReaderWrapper> countersReader() const;
    
private:
    std::shared_ptr<aeron::Aeron> aeron;
};

// Factory functions that cxx can safely bind to
std::unique_ptr<ContextWrapper> create_context();
std::unique_ptr<AeronWrapper> create_aeron(std::unique_ptr<ContextWrapper> context);
std::unique_ptr<MediaDriverWrapper> create_media_driver();

#ifdef AERON_ARCHIVE
// (ctx, control_session_id, correlation_id, recording_id, start_timestamp, stop_timestamp,
//  start_position, stop_position, initial_term_id, segment_file_length, term_buffer_length,
//  mtu_length, session_id, stream_id, stripped_channel, original_channel)
using RecordingDescriptorFn = rust::Fn<void(
    size_t, int64_t, int64_t, int64_t, int64_t, int64_t, int64_t, int64_t,
    int32_t, int32_t, int32_t, int32_t, int32_t, int32_t,
    rust::Slice<const uint8_t>, rust::Slice<const uint8_t>)>;

class ArchiveWrapper {
public:
    ArchiveWrapper(std::shared_ptr<aeron::archive::client::AeronArchive> archive);
    ~ArchiveWrapper();

    // Recording
    int64_t startRecording(::rust::Str channel, int32_t stream_id, int32_t source_location, bool auto_stop);
    void stopRecording(int64_t subscription_id);
    void stopRecordingByChannelAndStream(::rust::Str channel, int32_t stream_id);

    // Position queries
    int64_t getRecordingPosition(int64_t recording_id);
    int64_t getStartPosition(int64_t recording_id);
    int64_t getStopPosition(int64_t recording_id);
    int64_t getMaxRecordedPosition(int64_t recording_id);

    // Listing
    int32_t listRecordings(int64_t from_recording_id, int32_t record_count, RecordingDescriptorFn handler, size_t ctx);
    int32_t listRecordingsForUri(int64_t from_recording_id, int32_t record_count, ::rust::Str channel_fragment, int32_t stream_id, RecordingDescriptorFn handler, size_t ctx);
    int64_t findLastMatchingRecording(int64_t min_recording_id, ::rust::Str channel_fragment, int32_t stream_id, int32_t session_id);

    // Replay
    int64_t startReplay(int64_t recording_id, ::rust::Str replay_channel, int32_t replay_stream_id, int64_t position, int64_t length);
    void stopReplay(int64_t replay_session_id);
    void stopAllReplays(int64_t recording_id);

    // Truncate
    int64_t truncateRecording(int64_t recording_id, int64_t position);

    // Error polling
    ::rust::String pollForErrorResponse();
    void checkForErrorResponse();

    int64_t archiveId() const;
    int64_t controlSessionId() const;

    // Internal accessor for ReplayMerge (not exposed through cxx)
    const std::shared_ptr<aeron::archive::client::AeronArchive>& sharedArchive() const { return archive_; }

private:
    std::shared_ptr<aeron::archive::client::AeronArchive> archive_;
};

class ReplayMergeWrapper {
public:
    ReplayMergeWrapper(
        const std::shared_ptr<aeron::Subscription>& subscription,
        const std::shared_ptr<aeron::archive::client::AeronArchive>& archive,
        const std::string& replayChannel,
        const std::string& replayDestination,
        const std::string& liveDestination,
        int64_t recordingId,
        int64_t startPosition,
        int64_t mergeProgressTimeoutMs);
    ~ReplayMergeWrapper();

    int doWork();
    int poll(int fragment_limit, FragmentFn handler, size_t ctx);
    std::unique_ptr<ImageWrapper> image();
    bool isMerged() const;
    bool hasFailed() const;
    bool isLiveAdded() const;

private:
    std::shared_ptr<aeron::Subscription> subscription_;
    std::unique_ptr<aeron::archive::client::ReplayMerge> merge_;
};

// F9 prototype: sets a C-only archive context setting through the C handle that
// build.rs exposes, and reads it back. Replaced by the ArchiveContext builder (A1).
size_t probeArchiveControlMtuLength(size_t control_mtu_length);

std::unique_ptr<ArchiveWrapper> connect_archive(
    ::rust::Str control_request_channel, int32_t control_request_stream_id,
    ::rust::Str control_response_channel, int32_t control_response_stream_id);

std::unique_ptr<ReplayMergeWrapper> create_replay_merge(
    SubscriptionWrapper& subscription,
    ArchiveWrapper& archive,
    ::rust::Str replay_channel,
    ::rust::Str replay_destination,
    ::rust::Str live_destination,
    int64_t recording_id,
    int64_t start_position,
    int64_t merge_progress_timeout_ms);
#endif

} // namespace aeron_rs
