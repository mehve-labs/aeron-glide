#include "shim.h"
#include <iostream>
#include <thread>
#include "aeron-glide/src/lib.rs.h"

extern "C" {
#include <aeronmd.h>
}

namespace aeron_rs {

// Throws the Aeron exception matching aeron_errcode(), prefixed with `what`.
[[noreturn]] static void throwDriverError(const char *what) {
    using namespace aeron::util;
    std::string message = std::string(what) + ": " + aeron_errmsg();
    AERON_MAP_TO_SOURCED_EXCEPTION_AND_THROW(aeron_errcode(), message);
    throw AeronException(message, SOURCEINFO, aeron_errcode()); // unreachable
}

MediaDriverWrapper::MediaDriverWrapper() : context_(nullptr), driver_(nullptr) {
    if (aeron_driver_context_init(&context_) < 0) {
        throwDriverError("Failed to init driver context");
    }
}

MediaDriverWrapper::~MediaDriverWrapper() {
    if (driver_) { aeron_driver_close(driver_); driver_ = nullptr; }
    if (context_) { aeron_driver_context_close(context_); context_ = nullptr; }
}

void MediaDriverWrapper::start() {
    if (aeron_driver_init(&driver_, context_) < 0) {
        throwDriverError("Failed to init driver");
    }
    if (aeron_driver_start(driver_, false) < 0) {
        throwDriverError("Failed to start driver");
    }
}

void MediaDriverWrapper::setDir(rust::Str dir) {
    std::string s(dir.data(), dir.size());
    if (aeron_driver_context_set_dir(context_, s.c_str()) < 0) {
        throwDriverError("Failed to set dir");
    }
}

void MediaDriverWrapper::setDirDeleteOnStart(bool value) {
    if (aeron_driver_context_set_dir_delete_on_start(context_, value) < 0) {
        throwDriverError("Failed to set dir_delete_on_start");
    }
}

void MediaDriverWrapper::setDirDeleteOnShutdown(bool value) {
    if (aeron_driver_context_set_dir_delete_on_shutdown(context_, value) < 0) {
        throwDriverError("Failed to set dir_delete_on_shutdown");
    }
}

void MediaDriverWrapper::setThreadingMode(int32_t mode) {
    if (aeron_driver_context_set_threading_mode(context_, static_cast<aeron_threading_mode_t>(mode)) < 0) {
        throwDriverError("Failed to set threading_mode");
    }
}

void MediaDriverWrapper::setConductorIdleStrategy(rust::Str name) {
    std::string s(name.data(), name.size());
    if (aeron_driver_context_set_conductor_idle_strategy(context_, s.c_str()) < 0) {
        throwDriverError("Failed to set conductor_idle_strategy");
    }
}

void MediaDriverWrapper::setSenderIdleStrategy(rust::Str name) {
    std::string s(name.data(), name.size());
    if (aeron_driver_context_set_sender_idle_strategy(context_, s.c_str()) < 0) {
        throwDriverError("Failed to set sender_idle_strategy");
    }
}

void MediaDriverWrapper::setReceiverIdleStrategy(rust::Str name) {
    std::string s(name.data(), name.size());
    if (aeron_driver_context_set_receiver_idle_strategy(context_, s.c_str()) < 0) {
        throwDriverError("Failed to set receiver_idle_strategy");
    }
}

void MediaDriverWrapper::setTermBufferLength(size_t value) {
    if (aeron_driver_context_set_term_buffer_length(context_, value) < 0) {
        throwDriverError("Failed to set term_buffer_length");
    }
}

void MediaDriverWrapper::setIpcTermBufferLength(size_t value) {
    if (aeron_driver_context_set_ipc_term_buffer_length(context_, value) < 0) {
        throwDriverError("Failed to set ipc_term_buffer_length");
    }
}

void MediaDriverWrapper::setMtuLength(size_t value) {
    if (aeron_driver_context_set_mtu_length(context_, value) < 0) {
        throwDriverError("Failed to set mtu_length");
    }
}

void MediaDriverWrapper::setIpcMtuLength(size_t value) {
    if (aeron_driver_context_set_ipc_mtu_length(context_, value) < 0) {
        throwDriverError("Failed to set ipc_mtu_length");
    }
}

void MediaDriverWrapper::setSocketSoRcvbuf(size_t value) {
    if (aeron_driver_context_set_socket_so_rcvbuf(context_, value) < 0) {
        throwDriverError("Failed to set socket_so_rcvbuf");
    }
}

void MediaDriverWrapper::setSocketSoSndbuf(size_t value) {
    if (aeron_driver_context_set_socket_so_sndbuf(context_, value) < 0) {
        throwDriverError("Failed to set socket_so_sndbuf");
    }
}

void MediaDriverWrapper::setPrintConfiguration(bool value) {
    if (aeron_driver_context_set_print_configuration(context_, value) < 0) {
        throwDriverError("Failed to set print_configuration");
    }
}

void MediaDriverWrapper::setConductorCpuAffinity(int32_t cpu_id) {
    if (aeron_driver_context_set_conductor_cpu_affinity(context_, cpu_id) < 0) {
        throwDriverError("Failed to set conductor_cpu_affinity");
    }
}

void MediaDriverWrapper::setSenderCpuAffinity(int32_t cpu_id) {
    if (aeron_driver_context_set_sender_cpu_affinity(context_, cpu_id) < 0) {
        throwDriverError("Failed to set sender_cpu_affinity");
    }
}

void MediaDriverWrapper::setReceiverCpuAffinity(int32_t cpu_id) {
    if (aeron_driver_context_set_receiver_cpu_affinity(context_, cpu_id) < 0) {
        throwDriverError("Failed to set receiver_cpu_affinity");
    }
}

ContextWrapper::ContextWrapper() : ctx(std::make_shared<aeron::Context>()) {}

ContextWrapper::~ContextWrapper() {}

AeronWrapper::AeronWrapper(std::shared_ptr<ContextWrapper> context) 
    : aeron(aeron::Aeron::connect(*context->ctx)) {}

AeronWrapper::~AeronWrapper() {}

void AeronWrapper::start() {
    // connect handles starting under the hood in C++
}

bool AeronWrapper::isClosed() const {
    if (aeron) {
        return aeron->isClosed();
    }
    return true;
}

PublicationWrapper::PublicationWrapper(std::shared_ptr<aeron::Publication> pub) : pub(pub) {}

PublicationWrapper::~PublicationWrapper() {}

int64_t PublicationWrapper::offer(rust::Slice<const uint8_t> buffer) const {
    aeron::AtomicBuffer atomic_buffer(const_cast<uint8_t*>(buffer.data()), buffer.size());
    return pub->offer(atomic_buffer);
}

int64_t PublicationWrapper::tryClaim(size_t length, ClaimFn handler, size_t ctx) const {
    aeron::concurrent::logbuffer::BufferClaim bufferClaim;
    int64_t position = pub->tryClaim(static_cast<aeron::util::index_t>(length), bufferClaim);
    if (position > 0) {
        rust::Slice<uint8_t> slice(
            bufferClaim.buffer().buffer() + bufferClaim.offset(),
            bufferClaim.length()
        );
        bool commit = handler(ctx, slice);
        if (commit) {
            bufferClaim.commit();
        } else {
            bufferClaim.abort();
        }
    }
    return position;
}

bool PublicationWrapper::isConnected() const {
    return pub->isConnected();
}

int32_t PublicationWrapper::sessionId() const {
    return pub->sessionId();
}

ExclusivePublicationWrapper::ExclusivePublicationWrapper(std::shared_ptr<aeron::ExclusivePublication> pub) : pub(pub) {}

ExclusivePublicationWrapper::~ExclusivePublicationWrapper() {}

int64_t ExclusivePublicationWrapper::offer(rust::Slice<const uint8_t> buffer) {
    aeron::AtomicBuffer atomic_buffer(const_cast<uint8_t*>(buffer.data()), buffer.size());
    return pub->offer(atomic_buffer);
}

int64_t ExclusivePublicationWrapper::tryClaim(size_t length, ClaimFn handler, size_t ctx) {
    aeron::concurrent::logbuffer::BufferClaim bufferClaim;
    int64_t position = pub->tryClaim(static_cast<aeron::util::index_t>(length), bufferClaim);
    if (position > 0) {
        rust::Slice<uint8_t> slice(
            bufferClaim.buffer().buffer() + bufferClaim.offset(),
            bufferClaim.length()
        );
        bool commit = handler(ctx, slice);
        if (commit) {
            bufferClaim.commit();
        } else {
            bufferClaim.abort();
        }
    }
    return position;
}

bool ExclusivePublicationWrapper::isConnected() const {
    return pub->isConnected();
}

namespace {

// Points `slot` at the handler for the duration of a poll, and clears it on exit
// (including by exception) so the assembler never sees a dangling handler.
struct ScopedHandler {
    ScopedHandler(const ControlledFragmentFn *&slot, size_t &ctx_slot, const ControlledFragmentFn &handler, size_t ctx)
        : slot_(slot) {
        slot = &handler;
        ctx_slot = ctx;
    }
    ~ScopedHandler() { slot_ = nullptr; }
    const ControlledFragmentFn *&slot_;
};

aeron::ControlledPollAction dispatchControlled(
    const ControlledFragmentFn *handler, size_t ctx, aeron::AtomicBuffer& buffer, aeron::util::index_t offset, aeron::util::index_t length) {
    if (handler == nullptr) {
        return aeron::ControlledPollAction::ABORT;
    }
    rust::Slice<const uint8_t> slice(buffer.buffer() + offset, length);
    return static_cast<aeron::ControlledPollAction>((*handler)(ctx, slice));
}

} // namespace

SubscriptionWrapper::SubscriptionWrapper(std::shared_ptr<aeron::Subscription> sub)
    : sub(sub),
      controlled_assembler_([this](aeron::AtomicBuffer& buffer, aeron::util::index_t offset, aeron::util::index_t length, aeron::Header& header) {
          return dispatchControlled(controlled_handler_, controlled_ctx_, buffer, offset, length);
      }) {}

SubscriptionWrapper::~SubscriptionWrapper() {}

int SubscriptionWrapper::poll(int fragment_limit, FragmentFn handler, size_t ctx) {
    auto fragment_handler = [&](const aeron::AtomicBuffer& buffer, aeron::util::index_t offset, aeron::util::index_t length, aeron::Header& header) {
        rust::Slice<const uint8_t> slice(buffer.buffer() + offset, length);
        handler(ctx, slice);
    };
    return sub->poll(fragment_handler, fragment_limit);
}

int SubscriptionWrapper::controlledPollAssembled(int fragment_limit, ControlledFragmentFn handler, size_t ctx) {
    ScopedHandler scope(controlled_handler_, controlled_ctx_, handler, ctx);
    return sub->controlledPoll(controlled_assembler_.handler(), fragment_limit);
}

bool SubscriptionWrapper::isConnected() const {
    return sub->isConnected();
}

bool SubscriptionWrapper::deleteSessionBuffer(int32_t session_id) {
    return controlled_assembler_.deleteSessionBuffer(session_id);
}

int SubscriptionWrapper::imageCount() const {
    return static_cast<int>(sub->imageCount());
}

// Not bridged as `Result`: any failure (e.g. the std::logic_error the C++ wrapper
// throws for an index that is out of range) means "no such image".
std::unique_ptr<ImageWrapper> SubscriptionWrapper::imageByIndex(size_t index) const {
    try {
        auto image = sub->imageByIndex(index);
        return image ? std::unique_ptr<ImageWrapper>(new ImageWrapper(image, sub)) : nullptr;
    } catch (...) {
        return nullptr;
    }
}

std::unique_ptr<ImageWrapper> SubscriptionWrapper::imageBySessionId(int32_t session_id) const {
    try {
        auto image = sub->imageBySessionId(session_id);
        return image ? std::unique_ptr<ImageWrapper>(new ImageWrapper(image, sub)) : nullptr;
    } catch (...) {
        return nullptr;
    }
}

// ImageWrapper

ImageWrapper::ImageWrapper(std::shared_ptr<aeron::Image> image, std::shared_ptr<aeron::Subscription> subscription)
    : subscription_(std::move(subscription)),
      image_(std::move(image)),
      controlled_assembler_([this](aeron::AtomicBuffer& buffer, aeron::util::index_t offset, aeron::util::index_t length, aeron::Header& header) {
          return dispatchControlled(controlled_handler_, controlled_ctx_, buffer, offset, length);
      }) {}

ImageWrapper::~ImageWrapper() {}

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
    auto fragment_handler = [&](const aeron::AtomicBuffer& buffer, aeron::util::index_t offset, aeron::util::index_t length, aeron::Header& header) {
        rust::Slice<const uint8_t> slice(buffer.buffer() + offset, length);
        handler(ctx, slice);
    };
    return image_->poll(fragment_handler, fragment_limit);
}

int ImageWrapper::controlledPollAssembled(int fragment_limit, ControlledFragmentFn handler, size_t ctx) {
    ScopedHandler scope(controlled_handler_, controlled_ctx_, handler, ctx);
    return image_->controlledPoll(controlled_assembler_.handler(), fragment_limit);
}

CountersReaderWrapper::CountersReaderWrapper(std::shared_ptr<aeron::Aeron> aeron) : aeron(aeron) {}

CountersReaderWrapper::~CountersReaderWrapper() {}

int32_t CountersReaderWrapper::maxCounterId() const {
    return aeron->countersReader().maxCounterId();
}

int64_t CountersReaderWrapper::getCounterValue(int32_t id) const {
    return aeron->countersReader().getCounterValue(id);
}

int32_t CountersReaderWrapper::getCounterState(int32_t id) const {
    return aeron->countersReader().getCounterState(id);
}

int32_t CountersReaderWrapper::getCounterTypeId(int32_t id) const {
    return aeron->countersReader().getCounterTypeId(id);
}

rust::String CountersReaderWrapper::getCounterLabel(int32_t id) const {
    return rust::String::lossy(aeron->countersReader().getCounterLabel(id));
}

void CountersReaderWrapper::forEach(CounterFn handler, size_t ctx) const {
    aeron->countersReader().forEach([&](int32_t counter_id, int32_t type_id, const aeron::concurrent::AtomicBuffer& keyBuffer, const std::string& label) {
        rust::Slice<const uint8_t> key_slice(keyBuffer.buffer(), keyBuffer.capacity());
        rust::Slice<const uint8_t> label_slice(reinterpret_cast<const uint8_t *>(label.data()), label.size());
        handler(ctx, counter_id, type_id, key_slice, label_slice);
    });
}

std::unique_ptr<PublicationWrapper> AeronWrapper::addPublication(rust::Str channel, int32_t stream_id) const {
    int64_t reg_id = aeron->addPublication(std::string(channel.data(), channel.size()), stream_id);
    
    // We must poll for the publication to be created
    std::shared_ptr<aeron::Publication> pub;
    while (!(pub = aeron->findPublication(reg_id))) {
        std::this_thread::yield();
    }
    
    return std::unique_ptr<PublicationWrapper>(new PublicationWrapper(pub));
}

std::unique_ptr<ExclusivePublicationWrapper> AeronWrapper::addExclusivePublication(rust::Str channel, int32_t stream_id) const {
    int64_t reg_id = aeron->addExclusivePublication(std::string(channel.data(), channel.size()), stream_id);

    std::shared_ptr<aeron::ExclusivePublication> pub;
    while (!(pub = aeron->findExclusivePublication(reg_id))) {
        std::this_thread::yield();
    }

    return std::unique_ptr<ExclusivePublicationWrapper>(new ExclusivePublicationWrapper(pub));
}

std::unique_ptr<SubscriptionWrapper> AeronWrapper::addSubscription(rust::Str channel, int32_t stream_id) const {
    int64_t reg_id = aeron->addSubscription(std::string(channel.data(), channel.size()), stream_id);
    
    std::shared_ptr<aeron::Subscription> sub;
    while (!(sub = aeron->findSubscription(reg_id))) {
        std::this_thread::yield();
    }

    return std::unique_ptr<SubscriptionWrapper>(new SubscriptionWrapper(sub));
}

std::unique_ptr<CountersReaderWrapper> AeronWrapper::countersReader() const {
    return std::unique_ptr<CountersReaderWrapper>(new CountersReaderWrapper(aeron));
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

#ifdef AERON_ARCHIVE
#include "aeron-glide/src/archive.rs.h"

namespace aeron_rs {

ArchiveWrapper::ArchiveWrapper(std::shared_ptr<aeron::archive::client::AeronArchive> archive)
    : archive_(archive) {}

ArchiveWrapper::~ArchiveWrapper() {}

int64_t ArchiveWrapper::startRecording(::rust::Str channel, int32_t stream_id, int32_t source_location, bool auto_stop) {
    return archive_->startRecording(
        std::string(channel.data(), channel.size()),
        stream_id,
        static_cast<aeron::archive::client::AeronArchive::SourceLocation>(source_location),
        auto_stop);
}

void ArchiveWrapper::stopRecording(int64_t subscription_id) {
    archive_->stopRecording(subscription_id);
}

void ArchiveWrapper::stopRecordingByChannelAndStream(::rust::Str channel, int32_t stream_id) {
    archive_->stopRecording(std::string(channel.data(), channel.size()), stream_id);
}

int64_t ArchiveWrapper::getRecordingPosition(int64_t recording_id) {
    return archive_->getRecordingPosition(recording_id);
}

int64_t ArchiveWrapper::getStartPosition(int64_t recording_id) {
    return archive_->getStartPosition(recording_id);
}

int64_t ArchiveWrapper::getStopPosition(int64_t recording_id) {
    return archive_->getStopPosition(recording_id);
}

int64_t ArchiveWrapper::getMaxRecordedPosition(int64_t recording_id) {
    return archive_->getMaxRecordedPosition(recording_id);
}

static rust::Slice<const uint8_t> asSlice(const std::string &s) {
    return rust::Slice<const uint8_t>(reinterpret_cast<const uint8_t *>(s.data()), s.size());
}

int32_t ArchiveWrapper::listRecordings(int64_t from_recording_id, int32_t record_count, RecordingDescriptorFn handler, size_t ctx) {
    auto consumer = [&](aeron::archive::client::RecordingDescriptor& rd) {
        handler(
            ctx,
            rd.m_controlSessionId,
            rd.m_correlationId,
            rd.m_recordingId,
            rd.m_startTimestamp,
            rd.m_stopTimestamp,
            rd.m_startPosition,
            rd.m_stopPosition,
            rd.m_initialTermId,
            rd.m_segmentFileLength,
            rd.m_termBufferLength,
            rd.m_mtuLength,
            rd.m_sessionId,
            rd.m_streamId,
            asSlice(rd.m_strippedChannel),
            asSlice(rd.m_originalChannel));
    };
    return archive_->listRecordings(from_recording_id, record_count, consumer);
}

int32_t ArchiveWrapper::listRecordingsForUri(int64_t from_recording_id, int32_t record_count, ::rust::Str channel_fragment, int32_t stream_id, RecordingDescriptorFn handler, size_t ctx) {
    auto consumer = [&](aeron::archive::client::RecordingDescriptor& rd) {
        handler(
            ctx,
            rd.m_controlSessionId,
            rd.m_correlationId,
            rd.m_recordingId,
            rd.m_startTimestamp,
            rd.m_stopTimestamp,
            rd.m_startPosition,
            rd.m_stopPosition,
            rd.m_initialTermId,
            rd.m_segmentFileLength,
            rd.m_termBufferLength,
            rd.m_mtuLength,
            rd.m_sessionId,
            rd.m_streamId,
            asSlice(rd.m_strippedChannel),
            asSlice(rd.m_originalChannel));
    };
    return archive_->listRecordingsForUri(
        from_recording_id, record_count,
        std::string(channel_fragment.data(), channel_fragment.size()),
        stream_id, consumer);
}

int64_t ArchiveWrapper::findLastMatchingRecording(int64_t min_recording_id, ::rust::Str channel_fragment, int32_t stream_id, int32_t session_id) {
    return archive_->findLastMatchingRecording(
        min_recording_id,
        std::string(channel_fragment.data(), channel_fragment.size()),
        stream_id, session_id);
}

int64_t ArchiveWrapper::startReplay(int64_t recording_id, ::rust::Str replay_channel, int32_t replay_stream_id, int64_t position, int64_t length) {
    aeron::archive::client::ReplayParams params;
    params.position(position).length(length);
    return archive_->startReplay(
        recording_id,
        std::string(replay_channel.data(), replay_channel.size()),
        replay_stream_id, params);
}

void ArchiveWrapper::stopReplay(int64_t replay_session_id) {
    archive_->stopReplay(replay_session_id);
}

void ArchiveWrapper::stopAllReplays(int64_t recording_id) {
    archive_->stopAllReplays(recording_id);
}

int64_t ArchiveWrapper::truncateRecording(int64_t recording_id, int64_t position) {
    return archive_->truncateRecording(recording_id, position);
}

::rust::String ArchiveWrapper::pollForErrorResponse() {
    return ::rust::String::lossy(archive_->pollForErrorResponse());
}

void ArchiveWrapper::checkForErrorResponse() {
    archive_->checkForErrorResponse();
}

int64_t ArchiveWrapper::archiveId() const {
    return archive_->archiveId();
}

int64_t ArchiveWrapper::controlSessionId() const {
    return archive_->controlSessionId();
}

std::unique_ptr<ArchiveWrapper> connect_archive(
    ::rust::Str control_request_channel, int32_t control_request_stream_id,
    ::rust::Str control_response_channel, int32_t control_response_stream_id) {
    aeron::archive::client::Context ctx;
    ctx.controlRequestChannel(std::string(control_request_channel.data(), control_request_channel.size()));
    ctx.controlRequestStreamId(control_request_stream_id);
    ctx.controlResponseChannel(std::string(control_response_channel.data(), control_response_channel.size()));
    ctx.controlResponseStreamId(control_response_stream_id);
    auto archive = aeron::archive::client::AeronArchive::connect(ctx);
    return std::unique_ptr<ArchiveWrapper>(new ArchiveWrapper(archive));
}

// ReplayMergeWrapper

ReplayMergeWrapper::ReplayMergeWrapper(
    const std::shared_ptr<aeron::Subscription>& subscription,
    const std::shared_ptr<aeron::archive::client::AeronArchive>& archive,
    const std::string& replayChannel,
    const std::string& replayDestination,
    const std::string& liveDestination,
    int64_t recordingId,
    int64_t startPosition,
    int64_t mergeProgressTimeoutMs)
    : subscription_(subscription),
      merge_(std::make_unique<aeron::archive::client::ReplayMerge>(
          subscription, archive, replayChannel, replayDestination,
          liveDestination, recordingId, startPosition,
          aeron::currentTimeMillis, mergeProgressTimeoutMs)) {}

ReplayMergeWrapper::~ReplayMergeWrapper() {}

int ReplayMergeWrapper::doWork() {
    return merge_->doWork();
}

int ReplayMergeWrapper::poll(int fragment_limit, FragmentFn handler, size_t ctx) {
    auto fragment_handler = [&](const aeron::AtomicBuffer& buffer, aeron::util::index_t offset,
                                aeron::util::index_t length, aeron::Header& header) {
        rust::Slice<const uint8_t> slice(buffer.buffer() + offset, length);
        handler(ctx, slice);
    };
    return merge_->poll(fragment_handler, fragment_limit);
}

std::unique_ptr<ImageWrapper> ReplayMergeWrapper::image() {
    try {
        auto img = merge_->image();
        return img ? std::unique_ptr<ImageWrapper>(new ImageWrapper(img, subscription_)) : nullptr;
    } catch (...) {
        return nullptr;
    }
}

bool ReplayMergeWrapper::isMerged() const {
    return merge_->isMerged();
}

bool ReplayMergeWrapper::hasFailed() const {
    return merge_->hasFailed();
}

bool ReplayMergeWrapper::isLiveAdded() const {
    return merge_->isLiveAdded();
}

std::unique_ptr<ReplayMergeWrapper> create_replay_merge(
    SubscriptionWrapper& subscription,
    ArchiveWrapper& archive,
    ::rust::Str replay_channel,
    ::rust::Str replay_destination,
    ::rust::Str live_destination,
    int64_t recording_id,
    int64_t start_position,
    int64_t merge_progress_timeout_ms) {
    return std::make_unique<ReplayMergeWrapper>(
        subscription.sharedSubscription(),
        archive.sharedArchive(),
        std::string(replay_channel.data(), replay_channel.size()),
        std::string(replay_destination.data(), replay_destination.size()),
        std::string(live_destination.data(), live_destination.size()),
        recording_id, start_position, merge_progress_timeout_ms);
}

} // namespace aeron_rs
#endif
