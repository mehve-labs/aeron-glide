#pragma once
#include <cstring>
#include <deque>
#include <exception>
#include <atomic>
#include <mutex>
#include <memory>
#include <string>
#include <Aeron.h>
#include <HeartbeatTimestamp.h>
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

// A Rust string passed on as a C string: an interior NUL would silently cut
// it short, so it is rejected.
inline std::string cString(rust::Str s) {
    std::string out(s.data(), s.size());
    if (out.find('\0') != std::string::npos) {
        throw aeron::util::IllegalArgumentException(
            "string contains a NUL character: " + out.substr(0, out.find('\0')), SOURCEINFO, EINVAL);
    }
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
// (ctx, observation count, first observation timestamp, last observation timestamp, encoded exception)
using ErrorLogFn = rust::Fn<void(size_t, int32_t, int64_t, int64_t, rust::Slice<const uint8_t>)>;
// Long-lived handlers, run on the client conductor thread. Each takes the opaque
// Rust context first; the release function frees it (see RustOwned).
using ErrorFn = rust::Fn<void(size_t, rust::Slice<const uint8_t>)>;
using ReleaseFn = rust::Fn<void(size_t)>;
struct ImageInfo; // shared with Rust (lib.rs)
using ImageEventFn = rust::Fn<void(size_t, const ImageInfo &)>;
// (ctx, channel, stream id, session id, correlation id)
using NewPublicationFn = rust::Fn<void(size_t, rust::Slice<const uint8_t>, int32_t, int32_t, int64_t)>;
// (ctx, channel, stream id, correlation id)
using NewSubscriptionFn = rust::Fn<void(size_t, rust::Slice<const uint8_t>, int32_t, int64_t)>;
// (ctx, registration id, counter id)
using CounterEventFn = rust::Fn<void(size_t, int64_t, int32_t)>;
using CloseClientFn = rust::Fn<void(size_t)>;
// (ctx, registration id, session id, stream id, group tag, source port, address type, address bytes)
using ErrorFrameFn = rust::Fn<void(size_t, int64_t, int32_t, int32_t, int64_t, uint16_t, int16_t, rust::Slice<const uint8_t>)>;

// Serialises the client conductor in agent invoker mode. There the C client runs
// conductor work inline on whichever thread adds, closes or changes a resource,
// so those calls, and invoke(), must not run concurrently. Disabled (no locking)
// in the default threaded mode, where the conductor has its own thread.
class ConductorLock {
public:
    explicit ConductorLock(bool enabled) : enabled_(enabled) {}
    ConductorLock(const ConductorLock &) = delete;
    ConductorLock &operator=(const ConductorLock &) = delete;
    bool enabled() const { return enabled_; }

    // Holds the lock for one operation. If this thread already holds it (a handler
    // running inside invoke()), operations throw ReentrantException; destructors
    // (`nested_ok`) proceed, since the lock is already held.
    class Guard {
    public:
        explicit Guard(const std::shared_ptr<ConductorLock> &lock, bool nested_ok = false);
        ~Guard();
        Guard(const Guard &) = delete;
        Guard &operator=(const Guard &) = delete;

    private:
        ConductorLock *lock_ = nullptr;
        const ConductorLock *previous_ = nullptr;
    };

private:
    std::mutex mutex_;
    const bool enabled_;
};

// Owns a Rust handler context: releases it when the last std::function copy
// holding this object is destroyed (e.g. when the C++ client is destroyed).
class RustOwned {
public:
    RustOwned(ReleaseFn release, size_t ctx) : release_(release), ctx_(ctx) {}
    RustOwned(const RustOwned &) = delete;
    RustOwned &operator=(const RustOwned &) = delete;
    ~RustOwned() { release_(ctx_); }
    size_t ctx() const { return ctx_; }

private:
    ReleaseFn release_;
    size_t ctx_;
};

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
    void setUseConductorAgentInvoker(bool value);
    // Each takes ownership of `ctx`: `release(ctx)` runs when the last copy of the
    // handler is destroyed, i.e. when the C++ client is destroyed.
    void setErrorHandler(ErrorFn handler, ReleaseFn release, size_t ctx);
    void setAvailableImageHandler(ImageEventFn handler, ReleaseFn release, size_t ctx);
    void setUnavailableImageHandler(ImageEventFn handler, ReleaseFn release, size_t ctx);
    void setNewPublicationHandler(NewPublicationFn handler, ReleaseFn release, size_t ctx);
    void setNewExclusivePublicationHandler(NewPublicationFn handler, ReleaseFn release, size_t ctx);
    void setNewSubscriptionHandler(NewSubscriptionFn handler, ReleaseFn release, size_t ctx);
    void setAvailableCounterHandler(CounterEventFn handler, ReleaseFn release, size_t ctx);
    void setUnavailableCounterHandler(CounterEventFn handler, ReleaseFn release, size_t ctx);
    void setCloseClientHandler(CloseClientFn handler, ReleaseFn release, size_t ctx);
    void setErrorFrameHandler(ErrorFrameFn handler, ReleaseFn release, size_t ctx);

    std::shared_ptr<aeron::Context> ctx;
};

// Throws the Aeron exception matching aeron_errcode(), prefixed with `what`.
[[noreturn]] void throwDriverError(const char *what);

// Settings are applied by the generated driver_gen.h functions.
// (ctx, token) -> whether to terminate.
using TerminationValidatorFn = rust::Fn<bool(size_t, rust::Slice<const uint8_t>)>;

class MediaDriverWrapper {
public:
    MediaDriverWrapper();
    ~MediaDriverWrapper();

    // `manual_main_loop`: the caller runs the conductor with doWork (required in
    // the INVOKER threading mode).
    void start(bool manual_main_loop);
    // One duty cycle of a manually run driver (aeron_driver_main_do_work).
    // const: the Rust side serialises calls; the driver is reached through a pointer.
    int32_t doWork() const;
    // Its idle strategy (aeron_driver_main_idle_strategy).
    void idle(int32_t work_count) const;
    // Termination requests (D3), called on the driver conductor thread. Each
    // takes ownership of its ctx, released after the driver is closed.
    void setTerminationValidator(TerminationValidatorFn validator, ReleaseFn release, size_t ctx);
    void setTerminationHook(CloseClientFn hook, ReleaseFn release, size_t ctx);
    // Throws IllegalStateException once started: the driver's threads read the context.
    void ensureNotStarted() const;
    aeron_driver_context_t *context() const { return context_; }
    // A copy of `value` that lives as long as this wrapper: some C setters keep
    // the pointer they are given instead of copying the string.
    const char *keep(rust::Str value) {
        strings_.push_back(detail::cString(value));
        return strings_.back().c_str();
    }

    void setThreadingMode(int32_t mode);

    struct TerminationValidator {
        TerminationValidatorFn validator;
        RustOwned owner;
    };
    struct TerminationHook {
        CloseClientFn hook;
        RustOwned owner;
    };
private:
    // Destroyed after the driver and context are closed (members outlive the
    // destructor's body), so the driver never calls into released handlers.
    std::unique_ptr<TerminationValidator> validator_;
    std::unique_ptr<TerminationHook> hook_;
    aeron_driver_context_t* context_;
    aeron_driver_t* driver_;
    bool manual_ = false;
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
    PublicationWrapperT(std::shared_ptr<P> pub, std::shared_ptr<ConductorLock> lock)
        : pub(std::move(pub)), lock_(std::move(lock)) {}
    // Releasing the last reference closes the publication (conductor work).
    ~PublicationWrapperT() {
        ConductorLock::Guard guard(lock_, true);
        pub.reset();
    }

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
    int64_t addDestination(rust::Str endpoint) const {
        ConductorLock::Guard guard(lock_);
        return pub->addDestination(detail::cString(endpoint));
    }
    int64_t removeDestination(rust::Str endpoint) const {
        ConductorLock::Guard guard(lock_);
        return pub->removeDestination(detail::cString(endpoint));
    }
    int64_t removeDestinationById(int64_t registrationId) const {
        ConductorLock::Guard guard(lock_);
        return pub->removeDestination(registrationId);
    }
    bool findDestinationResponse(int64_t correlationId) const {
        ConductorLock::Guard guard(lock_);
        return pub->findDestinationResponse(correlationId);
    }

    // For the archive shim (not bridged).
    const std::shared_ptr<P> &shared() const { return pub; }

    // Exclusive publications only. revoke() frees the C publication; the Rust side
    // consumes the publication so nothing can be called on it afterwards.
    void revoke() const {
        ConductorLock::Guard guard(lock_);
        pub->revoke();
    }
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
    std::shared_ptr<ConductorLock> lock_;
};

using PublicationWrapper = PublicationWrapperT<aeron::Publication>;
using ExclusivePublicationWrapper = PublicationWrapperT<aeron::ExclusivePublication>;

class ImageWrapper; // forward declaration

// Polling state of a subscription, shared by the subscription and all of its Image
// handles: the reassembly state (a C++ ControlledFragmentAssembler, keyed by session
// id), so a message split across polls on different handles is not lost, and the
// sessions whose image is being polled, so a handler cannot poll the same image
// again through another handle.
class AssemblerState {
public:
    explicit AssemblerState(std::shared_ptr<ConductorLock> lock);

    // The client's conductor lock, shared with every image of the subscription.
    const std::shared_ptr<ConductorLock> conductorLock;
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

    // Marks an image (by session id) as being polled. Throws ReentrantException if
    // it already is: a nested poll would move the subscriber position under the
    // outer poll, re-delivering or releasing fragments it is still handling.
    class ImagePoll {
    public:
        ImagePoll(AssemblerState &state, int32_t session_id);
        ~ImagePoll();

    private:
        AssemblerState &state_;
    };

private:
    friend class Scope;
    friend class ImagePoll;
    std::vector<int32_t> polling_sessions_;
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
    SubscriptionWrapper(std::shared_ptr<aeron::Subscription> sub, std::shared_ptr<ConductorLock> lock);
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
    int64_t addDestination(rust::Str endpoint) const {
        ConductorLock::Guard guard(assembly_->conductorLock);
        return sub->addDestination(detail::cString(endpoint));
    }
    int64_t removeDestination(rust::Str endpoint) const {
        ConductorLock::Guard guard(assembly_->conductorLock);
        return sub->removeDestination(detail::cString(endpoint));
    }
    bool findDestinationResponse(int64_t correlationId) const {
        ConductorLock::Guard guard(assembly_->conductorLock);
        return sub->findDestinationResponse(correlationId);
    }

    // Image accessors
    int imageCount() const;
    std::unique_ptr<ImageWrapper> imageByIndex(size_t index) const;
    std::unique_ptr<ImageWrapper> imageBySessionId(int32_t session_id) const;

    // Internal accessor for ReplayMerge (not exposed through cxx)
    const std::shared_ptr<aeron::Subscription>& sharedSubscription() const { return sub; }
    const std::shared_ptr<AssemblerState>& sharedAssembly() const { return assembly_; }

private:
    std::shared_ptr<aeron::Subscription> sub;
    // The subscription's polling state, shared with its images.
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
    int boundedControlledPollAssembled(int64_t limit_position, int fragment_limit, ControlledFragmentFn handler, size_t ctx);

    // Accessors (P8)
    int32_t initialTermId() const { return image_->initialTermId(); }
    int32_t termBufferLength() const { return image_->termBufferLength(); }
    int32_t positionBitsToShift() const { return image_->positionBitsToShift(); }
    int32_t subscriberPositionId() const { return image_->subscriberPositionId(); }
    int64_t subscriptionRegistrationId() const { return image_->subscriptionRegistrationId(); }
    bool isPublicationRevoked() const { return image_->isPublicationRevoked(); }
    int32_t activeTransportCount() const { return image_->activeTransportCount(); }
    void reject(rust::Str reason) const { image_->reject(detail::cString(reason)); }

private:
    std::shared_ptr<aeron::Subscription> subscription_;
    std::shared_ptr<aeron::Image> image_;
    // The subscription's polling state, shared with its images.
    std::shared_ptr<AssemblerState> assembly_;
};

// A counter (C1): one this client added, closed when the last handle is dropped,
// or a view of any counter through a CountersReader (the public C++
// Counter(CountersReader&, registrationId, counterId) constructor).
// AtomicCounter's operations are non-const in C++ but only touch the counter's
// shared memory, so they are bridged as const.
namespace detail {
// Relaxed and release accesses to a counter. AtomicCounter's weak and ordered
// operations use plain loads and stores, a data race when the counter is shared
// between threads; these compile to the same instructions without the race.
#if defined(__GNUC__) || defined(__clang__)
inline int64_t loadRelaxed(int64_t *p) { return __atomic_load_n(p, __ATOMIC_RELAXED); }
inline void storeRelaxed(int64_t *p, int64_t v) { __atomic_store_n(p, v, __ATOMIC_RELAXED); }
inline void storeRelease(int64_t *p, int64_t v) { __atomic_store_n(p, v, __ATOMIC_RELEASE); }
#else
// MSVC: aligned 64-bit volatile accesses are single, atomic accesses on x64 and ARM64.
inline int64_t loadRelaxed(int64_t *p) { return *static_cast<volatile int64_t *>(p); }
inline void storeRelaxed(int64_t *p, int64_t v) { *static_cast<volatile int64_t *>(p) = v; }
inline void storeRelease(int64_t *p, int64_t v) {
    std::atomic_thread_fence(std::memory_order_release);
    *static_cast<volatile int64_t *>(p) = v;
}
#endif
} // namespace detail

class CounterWrapper {
public:
    // `keepalive` is released after the counter: it owns what a view's reader
    // points into (the client or CnC file), or the client of an added counter.
    // `addr` is the counter's value.
    CounterWrapper(std::shared_ptr<aeron::Counter> counter, int64_t *addr, std::shared_ptr<const void> keepalive,
                   std::shared_ptr<ConductorLock> lock)
        : counter_(std::move(counter)), addr_(addr), keepalive_(std::move(keepalive)), lock_(std::move(lock)) {}
    // Closing an added counter is conductor work, and either member may hold the
    // last reference to the client.
    ~CounterWrapper() {
        ConductorLock::Guard guard(lock_, true);
        counter_.reset();
        keepalive_.reset();
    }

    // Check, before each write, that the counter's record still holds this
    // counter (allocated, with the type and registration ID it has now): a
    // freed record can be reused by the driver for a counter Aeron relies on.
    void checkWrites(aeron::CountersReader &reader) {
        reader_ = reader.countersReader();
        const int32_t counterId = id();
        aeron_counters_reader_counter_type_id(reader_, counterId, &typeId_);
        aeron_counters_reader_counter_registration_id(reader_, counterId, &registrationId_);
    }

    // Whether the record still holds this counter (always for an unchecked one).
    bool isValid() const {
        if (reader_ == nullptr) {
            return true;
        }
        const int32_t counterId = id();
        int32_t state = 0;
        int32_t typeId = 0;
        int64_t registrationId = 0;
        return aeron_counters_reader_counter_state(reader_, counterId, &state) == 0 &&
               state == AERON_COUNTER_RECORD_ALLOCATED &&
               aeron_counters_reader_counter_type_id(reader_, counterId, &typeId) == 0 && typeId == typeId_ &&
               aeron_counters_reader_counter_registration_id(reader_, counterId, &registrationId) == 0 &&
               registrationId == registrationId_;
    }

    int32_t id() const { return counter_->id(); }
    int64_t registrationId() const { return counter_->registrationId(); }
    int32_t state() const { return counter_->state(); }
    rust::String label() const { return rust::String::lossy(counter_->label()); }
    bool isClosed() const { return counter_->isClosed(); }

    int64_t get() const { return counter_->get(); }
    int64_t getWeak() const { return detail::loadRelaxed(addr_); }
    // Writes do nothing once the record no longer holds this counter; those
    // returning the previous value then return the current one.
    void set(int64_t value) const {
        if (isValid()) counter_->set(value);
    }
    void setOrdered(int64_t value) const {
        if (isValid()) detail::storeRelease(addr_, value);
    }
    void setWeak(int64_t value) const {
        if (isValid()) detail::storeRelaxed(addr_, value);
    }
    void increment() const {
        if (isValid()) counter_->increment();
    }
    // Single-writer read-modify-write, as in AtomicCounter (not atomic as a whole).
    void incrementOrdered() const {
        if (isValid()) detail::storeRelease(addr_, detail::loadRelaxed(addr_) + 1);
    }
    int64_t getAndAdd(int64_t value) const { return isValid() ? counter_->getAndAdd(value) : get(); }
    int64_t getAndAddOrdered(int64_t value) const {
        if (!isValid()) {
            return get();
        }
        int64_t current = detail::loadRelaxed(addr_);
        detail::storeRelease(addr_, current + value);
        return current;
    }
    int64_t getAndSet(int64_t value) const { return isValid() ? counter_->getAndSet(value) : get(); }
    bool compareAndSet(int64_t expected, int64_t update) const {
        if (!isValid()) {
            return false;
        }
#if defined(__GNUC__) || defined(__clang__)
        // Upstream bug (1.53.3, Atomic64_gcc_cpp11.h, used on ARM): a failed
        // cmpxchg returns a fresh read, so compareAndSet can report success
        // without writing when the value changed back to `expected`.
        return __atomic_compare_exchange_n(addr_, &expected, update, false, __ATOMIC_SEQ_CST, __ATOMIC_SEQ_CST);
#else
        return counter_->compareAndSet(expected, update);
#endif
    }

    // For the archive shim (not bridged).
    const std::shared_ptr<aeron::Counter> &sharedCounter() const { return counter_; }
    const std::shared_ptr<const void> &keepalive() const { return keepalive_; }

private:
    std::shared_ptr<aeron::Counter> counter_;
    int64_t *addr_;
    std::shared_ptr<const void> keepalive_;
    std::shared_ptr<ConductorLock> lock_;
    aeron_counters_reader_t *reader_ = nullptr; // checked writes (kept alive by keepalive_ / counter_)
    int32_t typeId_ = 0;
    int64_t registrationId_ = 0;
};

class CountersReaderWrapper {
public:
    // `reader` shares ownership of what it reads (the client or the CnC file).
    // A read-only reader (the CnC file is mapped read-only) refuses counter().
    CountersReaderWrapper(std::shared_ptr<aeron::CountersReader> reader, std::shared_ptr<ConductorLock> lock,
                          bool writable = true);
    ~CountersReaderWrapper();

    int32_t maxCounterId() const;
    int64_t getCounterValue(int32_t id) const;
    int32_t getCounterState(int32_t id) const;
    int32_t getCounterTypeId(int32_t id) const;
    rust::String getCounterLabel(int32_t id) const;
    void forEach(CounterFn handler, size_t ctx) const;
    // C2: the remaining CountersReader lookups. find* return NULL_COUNTER_ID (-1)
    // when nothing matches.
    int32_t findByRegistrationId(int64_t registration_id) const { return reader_->findByRegistrationId(registration_id); }
    int32_t findByTypeIdAndRegistrationId(int32_t type_id, int64_t registration_id) const {
        return reader_->findByTypeIdAndRegistrationId(type_id, registration_id);
    }
    int64_t getCounterRegistrationId(int32_t id) const { return reader_->getCounterRegistrationId(id); }
    int64_t getCounterOwnerId(int32_t id) const { return reader_->getCounterOwnerId(id); }
    int64_t getFreeForReuseDeadline(int32_t id) const { return reader_->getFreeForReuseDeadline(id); }
    // The key region of a counter's metadata record (MAX_KEY_LENGTH bytes).
    rust::Vec<uint8_t> getCounterKey(int32_t id) const;
    // A handle on a counter (see CounterWrapper). Throws for an out-of-range id.
    // `checked`: only a user counter (type id >= 1000) with this registration ID,
    // and writes are checked; otherwise any counter, unchecked.
    std::unique_ptr<CounterWrapper> counter(int64_t registration_id, int32_t counter_id, bool checked) const;
    // A handle only read from: any counter, also from a CnC file.
    std::unique_ptr<CounterWrapper> counterView(int32_t counter_id) const;

    // HeartbeatTimestamp (C3). isActive is false for an out-of-range id, which
    // the C++ function does not check.
    int32_t findHeartbeatCounterId(int32_t type_id, int64_t registration_id) const {
        return aeron::HeartbeatTimestamp::findCounterIdByRegistrationId(*reader_, type_id, registration_id);
    }
    bool isHeartbeatActive(int32_t counter_id, int32_t type_id, int64_t registration_id) const {
        return counter_id >= 0 && counter_id <= reader_->maxCounterId() &&
               aeron::HeartbeatTimestamp::isActive(*reader_, counter_id, type_id, registration_id);
    }

    // For the archive shim (not bridged).
    aeron::CountersReader &reader() const { return *reader_; }

private:
    void validateCounterId(int32_t id) const;

    std::shared_ptr<aeron::CountersReader> reader_;
    std::shared_ptr<ConductorLock> lock_;
    bool writable_;
};

struct CncConstants; // shared with Rust (lib.rs)

// The CnC file of a media driver, mapped read-only without a client (C3). Uses
// the aeron_cnc_* functions aeron::CncFileReader wraps, since CncFileReader keeps
// its handle private: they also give the driver heartbeat and the file constants.
class CncFileWrapper {
public:
    // Waits up to `timeout_ms` for the file (CncFileReader::mapExisting waits 10 s).
    CncFileWrapper(rust::Str directory, int64_t timeout_ms);

    std::unique_ptr<CountersReaderWrapper> countersReader() const;
    int32_t readErrorLog(ErrorLogFn handler, size_t ctx, int64_t since_timestamp) const;
    int64_t toDriverHeartbeat() const { return aeron_cnc_to_driver_heartbeat(state_->cnc); }
    CncConstants constants() const;
    rust::String fileName() const { return rust::String::lossy(aeron_cnc_filename(state_->cnc)); }

private:
    struct State {
        explicit State(aeron_cnc_t *cnc) : cnc(cnc), reader(aeron_cnc_counters_reader(cnc)) {}
        ~State() { aeron_cnc_close(cnc); }
        State(const State &) = delete;
        State &operator=(const State &) = delete;
        aeron_cnc_t *cnc;
        aeron::CountersReader reader; // points into the mapping
    };
    std::shared_ptr<State> state_;
};

std::unique_ptr<CncFileWrapper> mapCncFile(rust::Str directory, int64_t timeout_ms);

class AeronWrapper {
public:
    AeronWrapper(std::shared_ptr<ContextWrapper> context);
    ~AeronWrapper();
    
    bool isClosed() const;
    
    // const: aeron::Aeron is thread-safe for adding resources.
    // Asynchronous adds (P10): start the registration and return its id; find*
    // returns null while it is pending and throws if the driver rejected it.
    int64_t addPublication(rust::Str channel, int32_t stream_id) const;
    int64_t addExclusivePublication(rust::Str channel, int32_t stream_id) const;
    int64_t addSubscription(rust::Str channel, int32_t stream_id) const;
    std::unique_ptr<PublicationWrapper> findPublication(int64_t registration_id) const;
    std::unique_ptr<ExclusivePublicationWrapper> findExclusivePublication(int64_t registration_id) const;
    std::unique_ptr<SubscriptionWrapper> findSubscription(int64_t registration_id) const;
    // Counters (C1), added asynchronously like the other resources.
    int64_t addCounter(int32_t type_id, rust::Slice<const uint8_t> key, rust::Str label) const;
    int64_t addStaticCounter(int32_t type_id, rust::Slice<const uint8_t> key, rust::Str label, int64_t registration_id) const;
    std::unique_ptr<CounterWrapper> findCounter(int64_t registration_id) const;

    int64_t clientId() const { return aeron->clientId(); }
    int64_t nextCorrelationId() const { return aeron->nextCorrelationId(); }
    rust::String aeronDir() const { return rust::String::lossy(aeron->context().aeronDir()); }
    rust::String cncFileName() const { return rust::String::lossy(aeron->context().cncFileName()); }
    int64_t driverTimeoutMs() const { return aeron->context().mediaDriverTimeout(); }
    rust::String clientName() const { return rust::String::lossy(aeron->context().clientName()); }
    int64_t idleSleepDurationMs() const { return aeron->context().idleSleepDuration(); }

    // Agent invoker mode (P12): run the client conductor's duty cycle on the
    // calling thread. Throws IllegalStateException unless the context enabled it.
    bool usesAgentInvoker() const { return aeron->usesAgentInvoker(); }
    int32_t invokeConductor() const;

    // Lifecycle handlers added at runtime (P11); each takes ownership of its ctx.
    int64_t addSubscriptionWithImageHandlers(
        rust::Str channel, int32_t stream_id,
        ImageEventFn on_available, ReleaseFn release_available, size_t available_ctx,
        ImageEventFn on_unavailable, ReleaseFn release_unavailable, size_t unavailable_ctx) const;
    int64_t addAvailableCounterHandler(CounterEventFn handler, ReleaseFn release, size_t ctx) const;
    void removeAvailableCounterHandler(int64_t registration_id) const;
    int64_t addUnavailableCounterHandler(CounterEventFn handler, ReleaseFn release, size_t ctx) const;
    void removeUnavailableCounterHandler(int64_t registration_id) const;
    int64_t addCloseClientHandler(CloseClientFn handler, ReleaseFn release, size_t ctx) const;
    void removeCloseClientHandler(int64_t registration_id) const;
    std::unique_ptr<CountersReaderWrapper> countersReader() const;

    // For the archive shim (not bridged).
    const std::shared_ptr<aeron::Aeron> &sharedAeron() const { return aeron; }
    const std::shared_ptr<ConductorLock> &conductorLock() const { return lock_; }

private:
    std::shared_ptr<aeron::Aeron> aeron;
    std::shared_ptr<ConductorLock> lock_;
};

// Version and clocks (T3).
inline rust::String aeronVersion() { return rust::String::lossy(aeron::Aeron::version()); }
inline int64_t currentTimeMillis() { return aeron::currentTimeMillis(); }
inline int64_t systemNanoClock() { return aeron::systemNanoClock(); }

// Static aeron::Context utilities (P13).
inline bool requestDriverTermination(rust::Str directory, rust::Slice<const uint8_t> token) {
    return aeron::Context::requestDriverTermination(detail::cString(directory), token.data(), token.size());
}
inline rust::String defaultAeronPath() { return rust::String::lossy(aeron::Context::defaultAeronPath()); }

// Factory functions that cxx can safely bind to
std::unique_ptr<ContextWrapper> create_context();
std::unique_ptr<AeronWrapper> create_aeron(std::unique_ptr<ContextWrapper> context);
std::unique_ptr<MediaDriverWrapper> create_media_driver();


} // namespace aeron_rs
