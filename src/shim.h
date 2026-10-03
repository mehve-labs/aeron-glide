#pragma once
#include <cstring>
#include <deque>
#include <exception>
#include <memory>
#include <string>
#include <Aeron.h>
#include <ControlledFragmentAssembler.h>
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
using FragmentFn = rust::Fn<void(size_t, rust::Slice<const uint8_t>, const aeron::concurrent::logbuffer::Header &)>;
using ControlledFragmentFn = rust::Fn<int32_t(size_t, rust::Slice<const uint8_t>, const aeron::concurrent::logbuffer::Header &)>;
using ClaimFn = rust::Fn<bool(size_t, rust::Slice<uint8_t>)>;
// (ctx, block of frames, session id, term id)
using BlockFn = rust::Fn<void(size_t, rust::Slice<const uint8_t>, int32_t, int32_t)>;
// (ctx, frame) -> reserved value for the frame header.
using ReservedValueFn = rust::Fn<int64_t(size_t, rust::Slice<const uint8_t>)>;
struct OfferPart; // shared with Rust (lib.rs)
struct ClaimFrame; // shared with Rust (lib.rs): a claimed frame, header included

// BufferClaim operations on a claimed frame (the C++ BufferClaim API).
void claimCommit(ClaimFrame frame);
void claimAbort(ClaimFrame frame);
uint8_t claimFlags(ClaimFrame frame);
void claimSetFlags(ClaimFrame frame, uint8_t flags);
uint16_t claimHeaderType(ClaimFrame frame);
void claimSetHeaderType(ClaimFrame frame, uint16_t type);
int64_t claimReservedValue(ClaimFrame frame);
void claimSetReservedValue(ClaimFrame frame, int64_t value);
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

// Workarounds for two upstream bugs in the 1.53.3 C++ wrapper, calling the C
// functions the C++ methods are meant to call:
// - Publication/ExclusivePublication::localSocketAddresses() build a string from an
//   uninitialised buffer when the C call returns no address (e.g. IPC).
// - ExclusivePublication::channelStatus() is declared but never defined.
namespace detail {
inline int localSockaddrs(aeron::Publication &p, aeron_iovec_t *iov, size_t count) {
    return aeron_publication_local_sockaddrs(p.publication(), iov, count);
}
inline int localSockaddrs(aeron::ExclusivePublication &p, aeron_iovec_t *iov, size_t count) {
    return aeron_exclusive_publication_local_sockaddrs(p.publication(), iov, count);
}
inline int64_t channelStatus(aeron::Publication &p) { return p.channelStatus(); }
inline int64_t channelStatus(aeron::ExclusivePublication &p) {
    return aeron_exclusive_publication_channel_status(p.publication());
}
} // namespace detail

// One wrapper for both publication types. Members that exist on only one of them
// (isOriginal, revoke, ...) are only instantiated for the type that uses them.
// All members are const: a concurrent publication is used from several threads,
// and the Rust side enforces single-writer access for exclusive publications.
template <typename P>
class PublicationWrapperT {
public:
    explicit PublicationWrapperT(std::shared_ptr<P> pub) : pub(std::move(pub)) {}

    int64_t offer(rust::Slice<const uint8_t> buffer) const {
        aeron::AtomicBuffer atomic_buffer(const_cast<uint8_t *>(buffer.data()), buffer.size());
        return pub->offer(atomic_buffer);
    }

    // Offer `parts` as one message (vectored offer), optionally with a reserved
    // value supplier. Up to 16 parts are wrapped on the stack.
    int64_t offerParts(rust::Slice<const OfferPart> parts, ReservedValueFn supplier, size_t ctx, bool useSupplier) const;

    // Claim `length` bytes; on success `frame` describes the claimed frame.
    int64_t tryClaim(size_t length, ClaimFrame &frame) const;

    // Accessors (P1)
    rust::String channel() const { return rust::String::lossy(pub->channel()); }
    int32_t streamId() const { return pub->streamId(); }
    int32_t sessionId() const { return pub->sessionId(); }
    int32_t initialTermId() const { return pub->initialTermId(); }
    int64_t originalRegistrationId() const { return pub->originalRegistrationId(); }
    int64_t registrationId() const { return pub->registrationId(); }
    bool isOriginal() const { return pub->isOriginal(); }
    int32_t maxMessageLength() const { return pub->maxMessageLength(); }
    int32_t maxPayloadLength() const { return pub->maxPayloadLength(); }
    int32_t termBufferLength() const { return pub->termBufferLength(); }
    int32_t positionBitsToShift() const { return pub->positionBitsToShift(); }
    bool isConnected() const { return pub->isConnected(); }
    bool isClosed() const { return pub->isClosed(); }
    int64_t maxPossiblePosition() const { return pub->maxPossiblePosition(); }
    int64_t position() const { return pub->position(); }
    int64_t publicationLimit() const { return pub->publicationLimit(); }
    int32_t publicationLimitId() const { return pub->publicationLimitId(); }
    int64_t availableWindow() const { return pub->availableWindow(); }
    int32_t channelStatusId() const { return pub->channelStatusId(); }
    int64_t channelStatus() const { return detail::channelStatus(*pub); }
    // Multi-destination-cast (P5). Each returns a correlation id; poll
    // findDestinationResponse until the driver has applied the change.
    int64_t addDestination(rust::Str endpoint) const { return pub->addDestination(std::string(endpoint)); }
    int64_t removeDestination(rust::Str endpoint) const { return pub->removeDestination(std::string(endpoint)); }
    int64_t removeDestinationById(int64_t registrationId) const { return pub->removeDestination(registrationId); }
    bool findDestinationResponse(int64_t correlationId) const { return pub->findDestinationResponse(correlationId); }

    // Exclusive publications only. revoke() frees the C publication; the Rust side
    // consumes the publication so nothing can be called on it afterwards.
    void revoke() const { pub->revoke(); }
    void revokeOnClose() const { pub->revokeOnClose(); }

    // Publications have at most one local address.
    rust::Vec<rust::String> localSocketAddresses() const {
        char buffer[AERON_CLIENT_MAX_LOCAL_ADDRESS_STR_LEN] = {0};
        aeron_iovec_t iov;
        iov.iov_base = reinterpret_cast<uint8_t *>(buffer);
        iov.iov_len = sizeof(buffer) - 1;
        int count = detail::localSockaddrs(*pub, &iov, 1);
        if (count < 0) {
            using namespace aeron::util;
            AERON_MAP_ERRNO_TO_SOURCED_EXCEPTION_AND_THROW;
        }
        rust::Vec<rust::String> addresses;
        if (count > 0) {
            addresses.push_back(rust::String::lossy(buffer));
        }
        return addresses;
    }

private:
    std::shared_ptr<P> pub;
};

using PublicationWrapper = PublicationWrapperT<aeron::Publication>;
using ExclusivePublicationWrapper = PublicationWrapperT<aeron::ExclusivePublication>;

class ImageWrapper; // forward declaration

// The reassembly state of a subscription (a C++ ControlledFragmentAssembler, keyed
// by session id), shared by the subscription and all of its Image handles so that a
// message split across polls on different handles is not lost.
class AssemblerState {
public:
    AssemblerState();
    AssemblerState(const AssemblerState &) = delete;
    AssemblerState &operator=(const AssemblerState &) = delete;

    aeron::ControlledFragmentAssembler &assembler() { return assembler_; }

    // Installs the handler of one assembled poll. Throws ReentrantException if an
    // assembled poll using this state is already running: it would modify the buffer
    // the running handler's message points into.
    class Scope {
    public:
        Scope(AssemblerState &state, const ControlledFragmentFn &handler, size_t ctx);
        ~Scope();

    private:
        AssemblerState &state_;
    };

private:
    friend class Scope;
    const ControlledFragmentFn *handler_ = nullptr;
    size_t ctx_ = 0;
    aeron::ControlledFragmentAssembler assembler_;
};

// A snapshot of a subscription's images (Subscription::copyOfImageList).
class ImageListWrapper {
public:
    ImageListWrapper(std::shared_ptr<std::vector<std::shared_ptr<aeron::Image>>> images,
                     std::shared_ptr<aeron::Subscription> subscription,
                     std::shared_ptr<AssemblerState> assembly)
        : images_(std::move(images)), subscription_(std::move(subscription)), assembly_(std::move(assembly)) {}
    size_t count() const { return images_ ? images_->size() : 0; }
    std::unique_ptr<ImageWrapper> get(size_t index) const;

private:
    std::shared_ptr<std::vector<std::shared_ptr<aeron::Image>>> images_;
    std::shared_ptr<aeron::Subscription> subscription_;
    std::shared_ptr<AssemblerState> assembly_;
};

class SubscriptionWrapper {
public:
    SubscriptionWrapper(std::shared_ptr<aeron::Subscription> sub);
    ~SubscriptionWrapper();

    int poll(int fragment_limit, FragmentFn handler, size_t ctx);
    int controlledPollAssembled(int fragment_limit, ControlledFragmentFn handler, size_t ctx);
    int controlledPoll(int fragment_limit, ControlledFragmentFn handler, size_t ctx);
    int64_t blockPoll(int block_length_limit, BlockFn handler, size_t ctx);
    bool isConnected() const;
    bool deleteSessionBuffer(int32_t session_id);

    // Accessors (P7)
    rust::String channel() const { return rust::String::lossy(sub->channel()); }
    int32_t streamId() const { return sub->streamId(); }
    int64_t registrationId() const { return sub->registrationId(); }
    int32_t channelStatusId() const { return sub->channelStatusId(); }
    int64_t channelStatus() const { return sub->channelStatus(); }
    bool isClosed() const { return sub->isClosed(); }
    rust::Vec<rust::String> localSocketAddresses() const {
        rust::Vec<rust::String> addresses;
        for (const auto &address : sub->localSocketAddresses()) {
            addresses.push_back(rust::String::lossy(address));
        }
        return addresses;
    }
    rust::String resolvedEndpoint() const { return rust::String::lossy(sub->resolvedEndpoint()); }
    rust::String tryResolveChannelEndpointPort() const { return rust::String::lossy(sub->tryResolveChannelEndpointPort()); }
    std::unique_ptr<ImageListWrapper> copyOfImageList() const;

    // Multi-destination subscriptions (P5).
    int64_t addDestination(rust::Str endpoint) const { return sub->addDestination(std::string(endpoint)); }
    int64_t removeDestination(rust::Str endpoint) const { return sub->removeDestination(std::string(endpoint)); }
    bool findDestinationResponse(int64_t correlationId) const { return sub->findDestinationResponse(correlationId); }

    // Image accessors
    int imageCount() const;
    std::unique_ptr<ImageWrapper> imageByIndex(size_t index) const;
    std::unique_ptr<ImageWrapper> imageBySessionId(int32_t session_id) const;

    // Internal accessor for ReplayMerge (not exposed through cxx)
    const std::shared_ptr<aeron::Subscription>& sharedSubscription() const { return sub; }
    const std::shared_ptr<AssemblerState>& sharedAssembly() const { return assembly_; }

private:
    std::shared_ptr<aeron::Subscription> sub;
    // Handler of the controlledPollAssembled call in progress, used by the assembler.
    std::shared_ptr<AssemblerState> assembly_;
};

class ImageWrapper {
public:
    // `subscription` owns the image: aeron::Image only holds raw C pointers, so the
    // subscription (and through it the client) must outlive it.
    // `assembly` is the subscription's shared reassembly state.
    ImageWrapper(std::shared_ptr<aeron::Image> image, std::shared_ptr<aeron::Subscription> subscription,
                 std::shared_ptr<AssemblerState> assembly);
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
    int controlledPoll(int fragment_limit, ControlledFragmentFn handler, size_t ctx);
    int boundedPoll(int64_t limit_position, int fragment_limit, FragmentFn handler, size_t ctx);
    int boundedControlledPoll(int64_t limit_position, int fragment_limit, ControlledFragmentFn handler, size_t ctx);
    int blockPoll(int block_length_limit, BlockFn handler, size_t ctx);

    // Accessors (P8)
    int32_t initialTermId() const { return image_->initialTermId(); }
    int32_t termBufferLength() const { return image_->termBufferLength(); }
    int32_t positionBitsToShift() const { return image_->positionBitsToShift(); }
    int32_t subscriberPositionId() const { return image_->subscriberPositionId(); }
    int64_t subscriptionRegistrationId() const { return image_->subscriptionRegistrationId(); }
    bool isPublicationRevoked() const { return image_->isPublicationRevoked(); }
    int32_t activeTransportCount() const { return image_->activeTransportCount(); }
    void reject(rust::Str reason) const { image_->reject(std::string(reason)); }

private:
    std::shared_ptr<aeron::Subscription> subscription_;
    std::shared_ptr<aeron::Image> image_;
    // Handler of the controlledPollAssembled call in progress, used by the assembler.
    std::shared_ptr<AssemblerState> assembly_;
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
        std::shared_ptr<AssemblerState> assembly,
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
    std::shared_ptr<AssemblerState> assembly_;
    std::unique_ptr<aeron::archive::client::ReplayMerge> merge_;
};

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
