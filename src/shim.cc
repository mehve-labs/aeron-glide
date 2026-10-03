#include "shim.h"
#include "counter_types.h"
#include <algorithm>
#include <array>
#include <iostream>
#include <vector>
#include <thread>
#include <filesystem>
#include "aeron-glide/src/lib.rs.h"

extern "C" {
#include <aeronmd.h>
#include <aeron_cnc_file_descriptor.h>
}

namespace aeron_rs {

// Mirrored by the CountersReader constants in src/counters.rs.
static_assert(aeron::CountersReader::RECORD_UNUSED == 0, "src/counters.rs");
static_assert(aeron::CountersReader::RECORD_ALLOCATED == 1, "src/counters.rs");
static_assert(aeron::CountersReader::RECORD_RECLAIMED == -1, "src/counters.rs");
static_assert(aeron::CountersReader::DEFAULT_REGISTRATION_ID == 0, "src/counters.rs");
static_assert(aeron::CountersReader::NOT_FREE_TO_REUSE == INT64_MAX, "src/counters.rs");
static_assert(aeron::CountersReader::MAX_LABEL_LENGTH == 380, "src/counters.rs");
static_assert(aeron::CountersReader::MAX_KEY_LENGTH == 112, "src/counters.rs");
static_assert(aeron::HeartbeatTimestamp::CLIENT_HEARTBEAT_TYPE_ID == 11, "src/counters.rs");

template <typename P>
int64_t PublicationWrapperT<P>::offerParts(
    rust::Slice<const OfferPart> parts, ReservedValueFn supplier, size_t ctx, bool useSupplier) const {
    std::array<aeron::AtomicBuffer, 16> stackBuffers;
    std::vector<aeron::AtomicBuffer> heapBuffers;
    aeron::AtomicBuffer *buffers = stackBuffers.data();
    if (parts.size() > stackBuffers.size()) {
        heapBuffers.resize(parts.size());
        buffers = heapBuffers.data();
    }
    for (size_t i = 0; i < parts.size(); i++) {
        buffers[i].wrap(reinterpret_cast<uint8_t *>(parts[i].ptr), parts[i].len);
    }
    if (!useSupplier) {
        return pub->offer(buffers, parts.size());
    }
    aeron::on_reserved_value_supplier_t reservedValue =
        [&](aeron::AtomicBuffer &termBuffer, aeron::util::index_t termOffset, aeron::util::index_t length) {
            return supplier(ctx, rust::Slice<const uint8_t>(termBuffer.buffer() + termOffset, length));
        };
    return pub->offer(buffers, parts.size(), reservedValue);
}

template <typename P>
int64_t PublicationWrapperT<P>::tryClaim(size_t length, ClaimFrame &frame) const {
    aeron::concurrent::logbuffer::BufferClaim bufferClaim;
    int64_t position = pub->tryClaim(static_cast<aeron::util::index_t>(length), bufferClaim);
    if (position > 0) {
        frame.ptr = reinterpret_cast<size_t>(bufferClaim.buffer().buffer());
        frame.len = static_cast<size_t>(bufferClaim.buffer().capacity());
    }
    return position;
}

namespace {
aeron::concurrent::logbuffer::BufferClaim claimOf(const ClaimFrame &frame) {
    aeron::concurrent::logbuffer::BufferClaim claim;
    claim.wrap(reinterpret_cast<uint8_t *>(frame.ptr), static_cast<aeron::util::index_t>(frame.len));
    return claim;
}
} // namespace

void claimCommit(ClaimFrame frame) { claimOf(frame).commit(); }
void claimAbort(ClaimFrame frame) { claimOf(frame).abort(); }
uint8_t claimFlags(ClaimFrame frame) { return claimOf(frame).flags(); }
void claimSetFlags(ClaimFrame frame, uint8_t flags) { claimOf(frame).flags(flags); }
uint16_t claimHeaderType(ClaimFrame frame) { return claimOf(frame).headerType(); }
void claimSetHeaderType(ClaimFrame frame, uint16_t type) { claimOf(frame).headerType(type); }
int64_t claimReservedValue(ClaimFrame frame) { return claimOf(frame).reservedValue(); }
void claimSetReservedValue(ClaimFrame frame, int64_t value) { claimOf(frame).reservedValue(value); }

// Defined here because they need the shared structs from the generated bridge header.
template int64_t PublicationWrapperT<aeron::Publication>::tryClaim(size_t, ClaimFrame &) const;
template int64_t PublicationWrapperT<aeron::ExclusivePublication>::tryClaim(size_t, ClaimFrame &) const;
template int64_t PublicationWrapperT<aeron::Publication>::offerParts(
    rust::Slice<const OfferPart>, ReservedValueFn, size_t, bool) const;
template int64_t PublicationWrapperT<aeron::ExclusivePublication>::offerParts(
    rust::Slice<const OfferPart>, ReservedValueFn, size_t, bool) const;

// Throws the Aeron exception matching aeron_errcode(), prefixed with `what`.
[[noreturn]] void throwDriverError(const char *what) {
    using namespace aeron::util;
    if (aeron_errcode() == 0) {
        // Some C setters reject a value without recording an error.
        throw IllegalArgumentException(std::string(what) + ": invalid value", SOURCEINFO, EINVAL);
    }
    std::string message = std::string(what) + ": " + aeron_errmsg();
    AERON_MAP_TO_SOURCED_EXCEPTION_AND_THROW(aeron_errcode(), message);
    throw AeronException(message, SOURCEINFO, aeron_errcode()); // unreachable
}

MediaDriverWrapper::MediaDriverWrapper() : context_(nullptr), driver_(nullptr) {
    if (aeron_driver_context_init(&context_) < 0) {
        throwDriverError("Failed to init driver context");
    }
}

void MediaDriverWrapper::ensureNotStarted() const {
    if (driver_ != nullptr) {
        throw aeron::util::IllegalStateException("media driver settings cannot change after start", SOURCEINFO, EPERM);
    }
}

MediaDriverWrapper::~MediaDriverWrapper() {
    if (driver_) { aeron_driver_close(driver_); driver_ = nullptr; }
    if (context_) { aeron_driver_context_close(context_); context_ = nullptr; }
}

void MediaDriverWrapper::start() {
    if (driver_ != nullptr) {
        throw aeron::util::IllegalStateException("media driver already started", SOURCEINFO, EPERM);
    }
    if (aeron_driver_init(&driver_, context_) < 0) {
        throwDriverError("Failed to init driver");
    }
    if (aeron_driver_start(driver_, false) < 0) {
        throwDriverError("Failed to start driver");
    }
}

void MediaDriverWrapper::setThreadingMode(int32_t mode) {
    ensureNotStarted();
    if (aeron_driver_context_set_threading_mode(context_, static_cast<aeron_threading_mode_t>(mode)) < 0) {
        throwDriverError("Failed to set threading_mode");
    }
}

namespace {

// Lifecycle handlers run on the conductor thread, called from C: nothing may
// unwind out of them. Rust handlers catch their own panics; this catches C++
// exceptions (e.g. bad_alloc) raised while preparing the call.
template <typename F>
void noUnwind(const char *what, F &&f) noexcept {
    try {
        f();
    } catch (const std::exception &e) {
        std::cerr << "aeron-glide: " << what << " handler failed: " << e.what() << std::endl;
    } catch (...) {
        std::cerr << "aeron-glide: " << what << " handler failed" << std::endl;
    }
}

rust::Slice<const uint8_t> bytesOf(const std::string &s) {
    return rust::Slice<const uint8_t>(reinterpret_cast<const uint8_t *>(s.data()), s.size());
}

ImageInfo imageInfo(aeron::Image &image) {
    ImageInfo info;
    info.session_id = image.sessionId();
    info.correlation_id = image.correlationId();
    info.subscription_registration_id = image.subscriptionRegistrationId();
    info.join_position = image.joinPosition();
    info.initial_term_id = image.initialTermId();
    info.term_buffer_length = image.termBufferLength();
    info.position_bits_to_shift = image.positionBitsToShift();
    info.source_identity = rust::String::lossy(image.sourceIdentity());
    try {
        info.position = image.position();
    } catch (...) {
        info.position = -1;
    }
    return info;
}

aeron::on_available_image_t imageHandler(const char *what, ImageEventFn handler, ReleaseFn release, size_t ctx) {
    auto owner = std::make_shared<RustOwned>(release, ctx);
    return [owner, handler, what](aeron::Image &image) {
        noUnwind(what, [&] { handler(owner->ctx(), imageInfo(image)); });
    };
}

aeron::on_available_counter_t counterHandler(const char *what, CounterEventFn handler, ReleaseFn release, size_t ctx) {
    auto owner = std::make_shared<RustOwned>(release, ctx);
    return [owner, handler, what](aeron::CountersReader &, int64_t registration_id, int32_t counter_id) {
        noUnwind(what, [&] { handler(owner->ctx(), registration_id, counter_id); });
    };
}

aeron::on_new_publication_t newPublicationHandler(const char *what, NewPublicationFn handler, ReleaseFn release, size_t ctx) {
    auto owner = std::make_shared<RustOwned>(release, ctx);
    return [owner, handler, what](const std::string &channel, int32_t stream_id, int32_t session_id, int64_t correlation_id) {
        noUnwind(what, [&] { handler(owner->ctx(), bytesOf(channel), stream_id, session_id, correlation_id); });
    };
}

aeron::on_close_client_t closeClientHandler(CloseClientFn handler, ReleaseFn release, size_t ctx) {
    auto owner = std::make_shared<RustOwned>(release, ctx);
    return [owner, handler]() { noUnwind("close client", [&] { handler(owner->ctx()); }); };
}

} // namespace

// The C++ wrapper's default error handler calls ::exit(-1); report instead.
ContextWrapper::ContextWrapper() : ctx(std::make_shared<aeron::Context>()) {
    ctx->errorHandler([](const std::exception &e) {
        std::cerr << "aeron-glide: Aeron client error: " << e.what() << std::endl;
    });
}

ContextWrapper::~ContextWrapper() {}

void ContextWrapper::setAeronDir(rust::Str dir) {
    ctx->aeronDir(std::string(dir.data(), dir.size()));
}

void ContextWrapper::setClientName(rust::Str name) {
    ctx->clientName(std::string(name.data(), name.size()));
}

void ContextWrapper::setDriverTimeoutMs(int64_t value) {
    ctx->mediaDriverTimeout(static_cast<long>(value));
}

void ContextWrapper::setResourceLingerTimeoutMs(int64_t value) {
    ctx->resourceLingerTimeout(static_cast<long>(value));
}

void ContextWrapper::setIdleSleepDurationMs(int64_t value) {
    ctx->idleSleepDuration(static_cast<long>(value));
}

void ContextWrapper::setPreTouchMappedMemory(bool value) {
    ctx->preTouchMappedMemory(value);
}

void ContextWrapper::setUseConductorAgentInvoker(bool value) {
    ctx->useConductorAgentInvoker(value);
}

void ContextWrapper::setErrorHandler(ErrorFn handler, ReleaseFn release, size_t context) {
    auto owner = std::make_shared<RustOwned>(release, context);
    ctx->errorHandler([owner, handler](const std::exception &e) {
        noUnwind("error", [&] {
            std::string encoded = detail::encode_exception(e);
            handler(owner->ctx(), bytesOf(encoded));
        });
    });
}

void ContextWrapper::setAvailableImageHandler(ImageEventFn handler, ReleaseFn release, size_t context) {
    ctx->availableImageHandler(imageHandler("available image", handler, release, context));
}

void ContextWrapper::setUnavailableImageHandler(ImageEventFn handler, ReleaseFn release, size_t context) {
    ctx->unavailableImageHandler(imageHandler("unavailable image", handler, release, context));
}

void ContextWrapper::setNewPublicationHandler(NewPublicationFn handler, ReleaseFn release, size_t context) {
    ctx->newPublicationHandler(newPublicationHandler("new publication", handler, release, context));
}

void ContextWrapper::setNewExclusivePublicationHandler(NewPublicationFn handler, ReleaseFn release, size_t context) {
    ctx->newExclusivePublicationHandler(newPublicationHandler("new exclusive publication", handler, release, context));
}

void ContextWrapper::setNewSubscriptionHandler(NewSubscriptionFn handler, ReleaseFn release, size_t context) {
    auto owner = std::make_shared<RustOwned>(release, context);
    ctx->newSubscriptionHandler([owner, handler](const std::string &channel, int32_t stream_id, int64_t correlation_id) {
        noUnwind("new subscription", [&] { handler(owner->ctx(), bytesOf(channel), stream_id, correlation_id); });
    });
}

void ContextWrapper::setAvailableCounterHandler(CounterEventFn handler, ReleaseFn release, size_t context) {
    ctx->availableCounterHandler(counterHandler("available counter", handler, release, context));
}

void ContextWrapper::setUnavailableCounterHandler(CounterEventFn handler, ReleaseFn release, size_t context) {
    ctx->unavailableCounterHandler(counterHandler("unavailable counter", handler, release, context));
}

void ContextWrapper::setCloseClientHandler(CloseClientFn handler, ReleaseFn release, size_t context) {
    ctx->closeClientHandler(closeClientHandler(handler, release, context));
}

void ContextWrapper::setErrorFrameHandler(ErrorFrameFn handler, ReleaseFn release, size_t context) {
    auto owner = std::make_shared<RustOwned>(release, context);
    aeron::on_publication_error_frame_t onErrorFrame = [owner, handler](aeron::status::PublicationErrorFrame &frame) {
        noUnwind("publication error frame", [&] {
            if (!frame.isValid()) {
                return;
            }
            rust::Slice<const uint8_t> address(frame.sourceAddress(), 16);
            handler(owner->ctx(), frame.registrationId(), frame.sessionId(), frame.streamId(), frame.groupTag(),
                    frame.sourcePort(), frame.sourceAddressType(), address);
        });
    };
    ctx->errorFrameHandler(onErrorFrame);
}

int64_t AeronWrapper::addSubscriptionWithImageHandlers(
    rust::Str channel, int32_t stream_id,
    ImageEventFn on_available, ReleaseFn release_available, size_t available_ctx,
    ImageEventFn on_unavailable, ReleaseFn release_unavailable, size_t unavailable_ctx) const {
    auto available = imageHandler("available image", on_available, release_available, available_ctx);
    auto unavailable = imageHandler("unavailable image", on_unavailable, release_unavailable, unavailable_ctx);
    ConductorLock::Guard guard(lock_);
    return aeron->addSubscription(std::string(channel.data(), channel.size()), stream_id, available, unavailable);
}

int64_t AeronWrapper::addAvailableCounterHandler(CounterEventFn handler, ReleaseFn release, size_t context) const {
    auto wrapped = counterHandler("available counter", handler, release, context); // owns the Rust context first
    ConductorLock::Guard guard(lock_);
    return aeron->addAvailableCounterHandler(wrapped);
}

void AeronWrapper::removeAvailableCounterHandler(int64_t registration_id) const {
    ConductorLock::Guard guard(lock_);
    aeron->removeAvailableCounterHandler(registration_id);
}

int64_t AeronWrapper::addUnavailableCounterHandler(CounterEventFn handler, ReleaseFn release, size_t context) const {
    auto wrapped = counterHandler("unavailable counter", handler, release, context); // owns the Rust context first
    ConductorLock::Guard guard(lock_);
    return aeron->addUnavailableCounterHandler(wrapped);
}

void AeronWrapper::removeUnavailableCounterHandler(int64_t registration_id) const {
    ConductorLock::Guard guard(lock_);
    aeron->removeUnavailableCounterHandler(registration_id);
}

int64_t AeronWrapper::addCloseClientHandler(CloseClientFn handler, ReleaseFn release, size_t context) const {
    auto wrapped = closeClientHandler(handler, release, context); // owns the Rust context first
    ConductorLock::Guard guard(lock_);
    return aeron->addCloseClientHandler(wrapped);
}

void AeronWrapper::removeCloseClientHandler(int64_t registration_id) const {
    ConductorLock::Guard guard(lock_);
    aeron->removeCloseClientHandler(registration_id);
}

AeronWrapper::AeronWrapper(std::shared_ptr<ContextWrapper> context)
    : aeron(aeron::Aeron::connect(*context->ctx)),
      lock_(std::make_shared<ConductorLock>(aeron->usesAgentInvoker())) {}

AeronWrapper::~AeronWrapper() {
    ConductorLock::Guard guard(lock_, true);
    aeron.reset();
}

int32_t AeronWrapper::invokeConductor() const {
    ConductorLock::Guard guard(lock_);
    return aeron->conductorAgentInvoker().invoke();
}

bool AeronWrapper::isClosed() const {
    if (aeron) {
        return aeron->isClosed();
    }
    return true;
}

namespace {

aeron::ControlledPollAction dispatchControlled(
    const ControlledFragmentFn *handler, size_t ctx, aeron::AtomicBuffer& buffer, aeron::util::index_t offset,
    aeron::util::index_t length, aeron::Header& header) {
    if (handler == nullptr) {
        return aeron::ControlledPollAction::ABORT;
    }
    rust::Slice<const uint8_t> slice(buffer.buffer() + offset, length);
    return static_cast<aeron::ControlledPollAction>((*handler)(ctx, slice, header));
}

} // namespace

thread_local const ConductorLock *t_held_conductor_lock = nullptr;

ConductorLock::Guard::Guard(const std::shared_ptr<ConductorLock> &lock, bool nested_ok) {
    if (!lock || !lock->enabled_) {
        return;
    }
    if (t_held_conductor_lock == lock.get()) {
        if (nested_ok) {
            return;
        }
        throw aeron::util::ReentrantException(
            "the client conductor is running on this thread (inside invoke)", SOURCEINFO, EPERM);
    }
    lock->mutex_.lock();
    lock_ = lock.get();
    previous_ = t_held_conductor_lock;
    t_held_conductor_lock = lock_;
}

ConductorLock::Guard::~Guard() {
    if (lock_ != nullptr) {
        t_held_conductor_lock = previous_;
        lock_->mutex_.unlock();
    }
}

AssemblerState::AssemblerState(std::shared_ptr<ConductorLock> lock)
    : conductorLock(std::move(lock)),
      assembler_([this](aeron::AtomicBuffer& buffer, aeron::util::index_t offset, aeron::util::index_t length, aeron::Header& header) {
          return dispatchControlled(handler_, ctx_, buffer, offset, length, header);
      }) {}

AssemblerState::Scope::Scope(AssemblerState &state, const ControlledFragmentFn &handler, size_t ctx) : state_(state) {
    if (state.handler_ != nullptr) {
        throw aeron::util::ReentrantException(
            "an assembled poll on this subscription is already running", SOURCEINFO, EPERM);
    }
    state.handler_ = &handler;
    state.ctx_ = ctx;
}

// Clears the handler on exit (including by exception) so the assembler never sees
// a dangling handler.
AssemblerState::Scope::~Scope() {
    state_.handler_ = nullptr;
}

AssemblerState::ImagePoll::ImagePoll(AssemblerState &state, int32_t session_id) : state_(state) {
    auto &sessions = state.polling_sessions_;
    if (std::find(sessions.begin(), sessions.end(), session_id) != sessions.end()) {
        throw aeron::util::ReentrantException(
            "this image is already being polled by an enclosing handler", SOURCEINFO, EPERM);
    }
    sessions.push_back(session_id);
}

// Polls nest, so the innermost one finishes first and its session is the last.
AssemblerState::ImagePoll::~ImagePoll() {
    state_.polling_sessions_.pop_back();
}

SubscriptionWrapper::SubscriptionWrapper(std::shared_ptr<aeron::Subscription> sub, std::shared_ptr<ConductorLock> lock)
    : sub(sub), assembly_(std::make_shared<AssemblerState>(std::move(lock))) {}

// Releasing the last reference closes the subscription (conductor work).
SubscriptionWrapper::~SubscriptionWrapper() {
    ConductorLock::Guard guard(assembly_->conductorLock, true);
    sub.reset();
}

int SubscriptionWrapper::poll(int fragment_limit, FragmentFn handler, size_t ctx) {
    auto fragment_handler = [&](const aeron::AtomicBuffer& buffer, aeron::util::index_t offset, aeron::util::index_t length, aeron::Header& header) {
        rust::Slice<const uint8_t> slice(buffer.buffer() + offset, length);
        handler(ctx, slice, header);
    };
    return sub->poll(fragment_handler, fragment_limit);
}

int SubscriptionWrapper::controlledPollAssembled(int fragment_limit, ControlledFragmentFn handler, size_t ctx) {
    AssemblerState::Scope scope(*assembly_, handler, ctx);
    return sub->controlledPoll(assembly_->assembler().handler(), fragment_limit);
}

int SubscriptionWrapper::controlledPoll(int fragment_limit, ControlledFragmentFn handler, size_t ctx) {
    auto fragment_handler = [&](const aeron::AtomicBuffer& buffer, aeron::util::index_t offset, aeron::util::index_t length, aeron::Header& header) {
        rust::Slice<const uint8_t> slice(buffer.buffer() + offset, length);
        return static_cast<aeron::ControlledPollAction>(handler(ctx, slice, header));
    };
    return sub->controlledPoll(fragment_handler, fragment_limit);
}

int64_t SubscriptionWrapper::blockPoll(int block_length_limit, BlockFn handler, size_t ctx) {
    auto block_handler = [&](aeron::AtomicBuffer& buffer, aeron::util::index_t offset, aeron::util::index_t length,
                             int32_t session_id, int32_t term_id) {
        rust::Slice<const uint8_t> slice(buffer.buffer() + offset, length);
        handler(ctx, slice, session_id, term_id);
    };
    return sub->blockPoll(block_handler, block_length_limit);
}

std::unique_ptr<ImageListWrapper> SubscriptionWrapper::copyOfImageList() const {
    return std::unique_ptr<ImageListWrapper>(new ImageListWrapper(sub->copyOfImageList(), sub, assembly_));
}

std::unique_ptr<ImageWrapper> ImageListWrapper::get(size_t index) const {
    if (!images_ || index >= images_->size()) {
        return nullptr;
    }
    return std::unique_ptr<ImageWrapper>(new ImageWrapper((*images_)[index], subscription_, assembly_));
}

bool SubscriptionWrapper::isConnected() const {
    return sub->isConnected();
}

bool SubscriptionWrapper::deleteSessionBuffer(int32_t session_id) {
    return assembly_->assembler().deleteSessionBuffer(session_id);
}

int SubscriptionWrapper::imageCount() const {
    return static_cast<int>(sub->imageCount());
}

// Not bridged as `Result`: any failure (e.g. the std::logic_error the C++ wrapper
// throws for an index that is out of range) means "no such image".
std::unique_ptr<ImageWrapper> SubscriptionWrapper::imageByIndex(size_t index) const {
    try {
        auto image = sub->imageByIndex(index);
        return image ? std::unique_ptr<ImageWrapper>(new ImageWrapper(image, sub, assembly_)) : nullptr;
    } catch (...) {
        return nullptr;
    }
}

std::unique_ptr<ImageWrapper> SubscriptionWrapper::imageBySessionId(int32_t session_id) const {
    try {
        auto image = sub->imageBySessionId(session_id);
        return image ? std::unique_ptr<ImageWrapper>(new ImageWrapper(image, sub, assembly_)) : nullptr;
    } catch (...) {
        return nullptr;
    }
}

// ImageWrapper

ImageWrapper::ImageWrapper(std::shared_ptr<aeron::Image> image, std::shared_ptr<aeron::Subscription> subscription,
                           std::shared_ptr<AssemblerState> assembly)
    : subscription_(std::move(subscription)), image_(std::move(image)), assembly_(std::move(assembly)) {}

// The image may hold the last reference to its subscription.
ImageWrapper::~ImageWrapper() {
    ConductorLock::Guard guard(assembly_->conductorLock, true);
    image_.reset();
    subscription_.reset();
}

int32_t ImageWrapper::sessionId() const {
    return image_->sessionId();
}

int64_t ImageWrapper::correlationId() const {
    return image_->correlationId();
}

int64_t ImageWrapper::joinPosition() const {
    return image_->joinPosition();
}

rust::String ImageWrapper::sourceIdentity() const {
    return rust::String::lossy(image_->sourceIdentity());
}

int64_t ImageWrapper::position() const {
    return image_->position();
}

bool ImageWrapper::isClosed() const {
    return image_->isClosed();
}

bool ImageWrapper::isEndOfStream() const {
    return image_->isEndOfStream();
}

int64_t ImageWrapper::endOfStreamPosition() const {
    return image_->endOfStreamPosition();
}

int ImageWrapper::poll(int fragment_limit, FragmentFn handler, size_t ctx) {
    AssemblerState::ImagePoll guard(*assembly_, image_->sessionId());
    auto fragment_handler = [&](const aeron::AtomicBuffer& buffer, aeron::util::index_t offset, aeron::util::index_t length, aeron::Header& header) {
        rust::Slice<const uint8_t> slice(buffer.buffer() + offset, length);
        handler(ctx, slice, header);
    };
    return image_->poll(fragment_handler, fragment_limit);
}

int ImageWrapper::controlledPollAssembled(int fragment_limit, ControlledFragmentFn handler, size_t ctx) {
    AssemblerState::ImagePoll guard(*assembly_, image_->sessionId());
    AssemblerState::Scope scope(*assembly_, handler, ctx);
    return image_->controlledPoll(assembly_->assembler().handler(), fragment_limit);
}

int ImageWrapper::controlledPoll(int fragment_limit, ControlledFragmentFn handler, size_t ctx) {
    AssemblerState::ImagePoll guard(*assembly_, image_->sessionId());
    auto fragment_handler = [&](const aeron::AtomicBuffer& buffer, aeron::util::index_t offset, aeron::util::index_t length, aeron::Header& header) {
        rust::Slice<const uint8_t> slice(buffer.buffer() + offset, length);
        return static_cast<aeron::ControlledPollAction>(handler(ctx, slice, header));
    };
    return image_->controlledPoll(fragment_handler, fragment_limit);
}

int ImageWrapper::boundedPoll(int64_t limit_position, int fragment_limit, FragmentFn handler, size_t ctx) {
    AssemblerState::ImagePoll guard(*assembly_, image_->sessionId());
    auto fragment_handler = [&](const aeron::AtomicBuffer& buffer, aeron::util::index_t offset, aeron::util::index_t length, aeron::Header& header) {
        rust::Slice<const uint8_t> slice(buffer.buffer() + offset, length);
        handler(ctx, slice, header);
    };
    return image_->boundedPoll(fragment_handler, limit_position, fragment_limit);
}

int ImageWrapper::boundedControlledPoll(int64_t limit_position, int fragment_limit, ControlledFragmentFn handler, size_t ctx) {
    AssemblerState::ImagePoll guard(*assembly_, image_->sessionId());
    auto fragment_handler = [&](const aeron::AtomicBuffer& buffer, aeron::util::index_t offset, aeron::util::index_t length, aeron::Header& header) {
        rust::Slice<const uint8_t> slice(buffer.buffer() + offset, length);
        return static_cast<aeron::ControlledPollAction>(handler(ctx, slice, header));
    };
    return image_->boundedControlledPoll(fragment_handler, limit_position, fragment_limit);
}

int ImageWrapper::boundedControlledPollAssembled(
    int64_t limit_position, int fragment_limit, ControlledFragmentFn handler, size_t ctx) {
    AssemblerState::ImagePoll guard(*assembly_, image_->sessionId());
    AssemblerState::Scope scope(*assembly_, handler, ctx);
    return image_->boundedControlledPoll(assembly_->assembler().handler(), limit_position, fragment_limit);
}

int ImageWrapper::blockPoll(int block_length_limit, BlockFn handler, size_t ctx) {
    AssemblerState::ImagePoll guard(*assembly_, image_->sessionId());
    auto block_handler = [&](aeron::AtomicBuffer& buffer, aeron::util::index_t offset, aeron::util::index_t length,
                             int32_t session_id, int32_t term_id) {
        rust::Slice<const uint8_t> slice(buffer.buffer() + offset, length);
        handler(ctx, slice, session_id, term_id);
    };
    return image_->blockPoll(block_handler, block_length_limit);
}

CountersReaderWrapper::CountersReaderWrapper(
    std::shared_ptr<aeron::CountersReader> reader, std::shared_ptr<ConductorLock> lock, bool writable)
    : reader_(std::move(reader)), lock_(std::move(lock)), writable_(writable) {}

// The reader may hold the last reference to the client.
CountersReaderWrapper::~CountersReaderWrapper() {
    ConductorLock::Guard guard(lock_, true);
    reader_.reset();
}

void CountersReaderWrapper::validateCounterId(int32_t id) const {
    if (id < 0 || id > reader_->maxCounterId()) {
        throw aeron::util::IllegalArgumentException(
            "counter id " + std::to_string(id) + " out of range: maxCounterId=" + std::to_string(reader_->maxCounterId()),
            SOURCEINFO, EINVAL);
    }
}

rust::Vec<uint8_t> CountersReaderWrapper::getCounterKey(int32_t id) const {
    validateCounterId(id);
    aeron::AtomicBuffer metadata = reader_->metaDataBuffer();
    const auto offset = aeron::CountersReader::metadataOffset(id) + aeron::CountersReader::KEY_OFFSET;
    rust::Vec<uint8_t> key;
    key.reserve(aeron::CountersReader::MAX_KEY_LENGTH);
    for (std::int32_t i = 0; i < aeron::CountersReader::MAX_KEY_LENGTH; i++) {
        key.push_back(metadata.getUInt8(offset + i));
    }
    return key;
}

std::unique_ptr<CounterWrapper> CountersReaderWrapper::counter(int64_t registration_id, int32_t counter_id) const {
    validateCounterId(counter_id); // the C++ constructor does not check it
    if (!writable_) {
        // Writing through the counter would fault on the read-only mapping.
        throw aeron::util::UnsupportedOperationException(
            "counters read from a CnC file are read-only", SOURCEINFO, EPERM);
    }
    auto view = std::make_shared<aeron::Counter>(*reader_, registration_id, counter_id);
    int64_t *addr = reader_->getCounterAddress(counter_id);
    return std::unique_ptr<CounterWrapper>(new CounterWrapper(std::move(view), addr, reader_, lock_));
}

int32_t CountersReaderWrapper::maxCounterId() const {
    return reader_->maxCounterId();
}

int64_t CountersReaderWrapper::getCounterValue(int32_t id) const {
    return reader_->getCounterValue(id);
}

int32_t CountersReaderWrapper::getCounterState(int32_t id) const {
    return reader_->getCounterState(id);
}

int32_t CountersReaderWrapper::getCounterTypeId(int32_t id) const {
    return reader_->getCounterTypeId(id);
}

rust::String CountersReaderWrapper::getCounterLabel(int32_t id) const {
    return rust::String::lossy(reader_->getCounterLabel(id));
}

void CountersReaderWrapper::forEach(CounterFn handler, size_t ctx) const {
    reader_->forEach([&](int32_t counter_id, int32_t type_id, const aeron::concurrent::AtomicBuffer& keyBuffer, const std::string& label) {
        rust::Slice<const uint8_t> key_slice(keyBuffer.buffer(), keyBuffer.capacity());
        rust::Slice<const uint8_t> label_slice(reinterpret_cast<const uint8_t *>(label.data()), label.size());
        handler(ctx, counter_id, type_id, key_slice, label_slice);
    });
}

CncFileWrapper::CncFileWrapper(rust::Str directory, int64_t timeout_ms) {
    std::string dir(directory);
    if (dir.find('\0') != std::string::npos) {
        throw aeron::util::IllegalArgumentException("directory contains a NUL character", SOURCEINFO, EINVAL);
    }
    aeron_cnc_t *cnc = nullptr;
    if (aeron_cnc_init(&cnc, dir.c_str(), timeout_ms) < 0) {
        throw aeron::util::IOException(
            "failed to open existing cnc file in: " + dir + ": " + aeron_errmsg(), SOURCEINFO, aeron_errcode());
    }
    std::unique_ptr<aeron_cnc_t, void (*)(aeron_cnc_t *)> owned(cnc, aeron_cnc_close);
    // The C client sums the buffer lengths as size_t, so negative lengths in a
    // corrupt file can pass its size check and place buffers outside the mapping.
    aeron_cnc_constants_t c = {};
    std::error_code ec;
    const auto fileLength = std::filesystem::file_size(aeron_cnc_filename(cnc), ec);
    bool valid = aeron_cnc_constants(cnc, &c) == 0 && !ec;
    int64_t total = AERON_CNC_VERSION_AND_META_DATA_LENGTH;
    for (int32_t length : {c.to_driver_buffer_length, c.to_clients_buffer_length, c.counter_metadata_buffer_length,
                           c.counter_values_buffer_length, c.error_log_buffer_length}) {
        valid = valid && length >= 0;
        total += length;
    }
    if (!valid || static_cast<uint64_t>(total) > fileLength) {
        throw aeron::util::IOException("invalid cnc file in: " + dir, SOURCEINFO, EINVAL);
    }
    state_ = std::make_shared<State>(cnc);
    owned.release();
}

std::unique_ptr<CountersReaderWrapper> CncFileWrapper::countersReader() const {
    std::shared_ptr<aeron::CountersReader> reader(state_, &state_->reader);
    return std::unique_ptr<CountersReaderWrapper>(new CountersReaderWrapper(std::move(reader), nullptr, false));
}

namespace {
struct ErrorLogConsumer {
    ErrorLogFn handler;
    size_t ctx;
    int32_t count;
};

// Called from C: noexcept, and builds nothing that could throw.
void errorLogCallback(int32_t observations, int64_t first, int64_t last, const char *error, size_t length,
                      void *clientd) noexcept {
    auto *consumer = static_cast<ErrorLogConsumer *>(clientd);
    // The driver publishes an entry before its first observation is recorded.
    if (observations <= 0) {
        return;
    }
    consumer->count++;
    consumer->handler(consumer->ctx, observations, first, last,
                      rust::Slice<const uint8_t>(reinterpret_cast<const uint8_t *>(error), length));
}
} // namespace

int32_t CncFileWrapper::readErrorLog(ErrorLogFn handler, size_t ctx, int64_t since_timestamp) const {
    ErrorLogConsumer consumer{handler, ctx, 0};
    aeron_cnc_error_log_read(state_->cnc, errorLogCallback, &consumer, since_timestamp);
    return consumer.count;
}

CncConstants CncFileWrapper::constants() const {
    using namespace aeron::util;
    aeron_cnc_constants_t c = {};
    if (aeron_cnc_constants(state_->cnc, &c) < 0) {
        AERON_MAP_ERRNO_TO_SOURCED_EXCEPTION_AND_THROW;
    }
    CncConstants out;
    out.cnc_version = c.cnc_version;
    out.to_driver_buffer_length = c.to_driver_buffer_length;
    out.to_clients_buffer_length = c.to_clients_buffer_length;
    out.counter_metadata_buffer_length = c.counter_metadata_buffer_length;
    out.counter_values_buffer_length = c.counter_values_buffer_length;
    out.error_log_buffer_length = c.error_log_buffer_length;
    out.client_liveness_timeout_ns = c.client_liveness_timeout;
    out.start_timestamp_ms = c.start_timestamp;
    out.pid = c.pid;
    out.file_page_size = c.file_page_size;
    return out;
}

std::unique_ptr<CncFileWrapper> mapCncFile(rust::Str directory, int64_t timeout_ms) {
    return std::unique_ptr<CncFileWrapper>(new CncFileWrapper(directory, timeout_ms));
}

int64_t AeronWrapper::addPublication(rust::Str channel, int32_t stream_id) const {
    ConductorLock::Guard guard(lock_);
    return aeron->addPublication(std::string(channel.data(), channel.size()), stream_id);
}

int64_t AeronWrapper::addExclusivePublication(rust::Str channel, int32_t stream_id) const {
    ConductorLock::Guard guard(lock_);
    return aeron->addExclusivePublication(std::string(channel.data(), channel.size()), stream_id);
}

int64_t AeronWrapper::addSubscription(rust::Str channel, int32_t stream_id) const {
    ConductorLock::Guard guard(lock_);
    return aeron->addSubscription(std::string(channel.data(), channel.size()), stream_id);
}

std::unique_ptr<PublicationWrapper> AeronWrapper::findPublication(int64_t registration_id) const {
    ConductorLock::Guard guard(lock_);
    auto pub = aeron->findPublication(registration_id);
    return pub ? std::unique_ptr<PublicationWrapper>(new PublicationWrapper(pub, lock_)) : nullptr;
}

std::unique_ptr<ExclusivePublicationWrapper> AeronWrapper::findExclusivePublication(int64_t registration_id) const {
    ConductorLock::Guard guard(lock_);
    auto pub = aeron->findExclusivePublication(registration_id);
    return pub ? std::unique_ptr<ExclusivePublicationWrapper>(new ExclusivePublicationWrapper(pub, lock_)) : nullptr;
}

std::unique_ptr<SubscriptionWrapper> AeronWrapper::findSubscription(int64_t registration_id) const {
    ConductorLock::Guard guard(lock_);
    auto sub = aeron->findSubscription(registration_id);
    return sub ? std::unique_ptr<SubscriptionWrapper>(new SubscriptionWrapper(sub, lock_)) : nullptr;
}

int64_t AeronWrapper::addCounter(int32_t type_id, rust::Slice<const uint8_t> key, rust::Str label) const {
    ConductorLock::Guard guard(lock_);
    return aeron->addCounter(type_id, key.data(), key.size(), std::string(label));
}

int64_t AeronWrapper::addStaticCounter(
    int32_t type_id, rust::Slice<const uint8_t> key, rust::Str label, int64_t registration_id) const {
    ConductorLock::Guard guard(lock_);
    return aeron->addStaticCounter(type_id, key.data(), key.size(), std::string(label), registration_id);
}

std::unique_ptr<CounterWrapper> AeronWrapper::findCounter(int64_t registration_id) const {
    ConductorLock::Guard guard(lock_);
    auto counter = aeron->findCounter(registration_id);
    // Upstream bug (1.53.3): aeron::Counter releases its reference to the client
    // (a member) before ~AtomicCounter (its base) closes the C counter, which the
    // client has freed if that was the last reference. Keep the client alive
    // until the counter is destroyed.
    if (!counter) {
        return nullptr;
    }
    int64_t *addr = aeron_counter_addr(counter->c_counter());
    return std::unique_ptr<CounterWrapper>(new CounterWrapper(std::move(counter), addr, aeron, lock_));
}

std::unique_ptr<CountersReaderWrapper> AeronWrapper::countersReader() const {
    // Aliasing: points at the client's reader and keeps the client alive.
    std::shared_ptr<aeron::CountersReader> reader(aeron, &aeron->countersReader());
    return std::unique_ptr<CountersReaderWrapper>(new CountersReaderWrapper(std::move(reader), lock_));
}

std::unique_ptr<ContextWrapper> create_context() {
    return std::unique_ptr<ContextWrapper>(new ContextWrapper());
}

std::unique_ptr<AeronWrapper> create_aeron(std::unique_ptr<ContextWrapper> context) {
    auto shared_ctx = std::shared_ptr<ContextWrapper>(std::move(context));
    return std::unique_ptr<AeronWrapper>(new AeronWrapper(shared_ctx));
}

std::unique_ptr<MediaDriverWrapper> create_media_driver() {
    return std::unique_ptr<MediaDriverWrapper>(new MediaDriverWrapper());
}

} // namespace aeron_rs (close before archive include to avoid double namespace)
