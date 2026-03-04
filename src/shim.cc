#include "shim.h"
#include <iostream>
#include <thread>
#include "aeron-rs/src/lib.rs.h"

extern "C" {
#include <aeronmd.h>
}

namespace aeron_rs {

MediaDriverWrapper::MediaDriverWrapper() : context_(nullptr), driver_(nullptr) {
    if (aeron_driver_context_init(&context_) < 0) {
        throw std::runtime_error(std::string("Failed to init driver context: ") + aeron_errmsg());
    }
}

MediaDriverWrapper::~MediaDriverWrapper() {
    if (driver_) { aeron_driver_close(driver_); driver_ = nullptr; }
    if (context_) { aeron_driver_context_close(context_); context_ = nullptr; }
}

void MediaDriverWrapper::start() {
    if (aeron_driver_init(&driver_, context_) < 0) {
        throw std::runtime_error(std::string("Failed to init driver: ") + aeron_errmsg());
    }
    if (aeron_driver_start(driver_, false) < 0) {
        throw std::runtime_error(std::string("Failed to start driver: ") + aeron_errmsg());
    }
}

void MediaDriverWrapper::setDir(rust::Str dir) {
    std::string s(dir.data(), dir.size());
    if (aeron_driver_context_set_dir(context_, s.c_str()) < 0) {
        throw std::runtime_error(std::string("Failed to set dir: ") + aeron_errmsg());
    }
}

void MediaDriverWrapper::setDirDeleteOnStart(bool value) {
    if (aeron_driver_context_set_dir_delete_on_start(context_, value) < 0) {
        throw std::runtime_error(std::string("Failed to set dir_delete_on_start: ") + aeron_errmsg());
    }
}

void MediaDriverWrapper::setDirDeleteOnShutdown(bool value) {
    if (aeron_driver_context_set_dir_delete_on_shutdown(context_, value) < 0) {
        throw std::runtime_error(std::string("Failed to set dir_delete_on_shutdown: ") + aeron_errmsg());
    }
}

void MediaDriverWrapper::setThreadingMode(int32_t mode) {
    if (aeron_driver_context_set_threading_mode(context_, static_cast<aeron_threading_mode_t>(mode)) < 0) {
        throw std::runtime_error(std::string("Failed to set threading_mode: ") + aeron_errmsg());
    }
}

void MediaDriverWrapper::setConductorIdleStrategy(rust::Str name) {
    std::string s(name.data(), name.size());
    if (aeron_driver_context_set_conductor_idle_strategy(context_, s.c_str()) < 0) {
        throw std::runtime_error(std::string("Failed to set conductor_idle_strategy: ") + aeron_errmsg());
    }
}

void MediaDriverWrapper::setSenderIdleStrategy(rust::Str name) {
    std::string s(name.data(), name.size());
    if (aeron_driver_context_set_sender_idle_strategy(context_, s.c_str()) < 0) {
        throw std::runtime_error(std::string("Failed to set sender_idle_strategy: ") + aeron_errmsg());
    }
}

void MediaDriverWrapper::setReceiverIdleStrategy(rust::Str name) {
    std::string s(name.data(), name.size());
    if (aeron_driver_context_set_receiver_idle_strategy(context_, s.c_str()) < 0) {
        throw std::runtime_error(std::string("Failed to set receiver_idle_strategy: ") + aeron_errmsg());
    }
}

void MediaDriverWrapper::setTermBufferLength(size_t value) {
    if (aeron_driver_context_set_term_buffer_length(context_, value) < 0) {
        throw std::runtime_error(std::string("Failed to set term_buffer_length: ") + aeron_errmsg());
    }
}

void MediaDriverWrapper::setIpcTermBufferLength(size_t value) {
    if (aeron_driver_context_set_ipc_term_buffer_length(context_, value) < 0) {
        throw std::runtime_error(std::string("Failed to set ipc_term_buffer_length: ") + aeron_errmsg());
    }
}

void MediaDriverWrapper::setMtuLength(size_t value) {
    if (aeron_driver_context_set_mtu_length(context_, value) < 0) {
        throw std::runtime_error(std::string("Failed to set mtu_length: ") + aeron_errmsg());
    }
}

void MediaDriverWrapper::setIpcMtuLength(size_t value) {
    if (aeron_driver_context_set_ipc_mtu_length(context_, value) < 0) {
        throw std::runtime_error(std::string("Failed to set ipc_mtu_length: ") + aeron_errmsg());
    }
}

void MediaDriverWrapper::setSocketSoRcvbuf(size_t value) {
    if (aeron_driver_context_set_socket_so_rcvbuf(context_, value) < 0) {
        throw std::runtime_error(std::string("Failed to set socket_so_rcvbuf: ") + aeron_errmsg());
    }
}

void MediaDriverWrapper::setSocketSoSndbuf(size_t value) {
    if (aeron_driver_context_set_socket_so_sndbuf(context_, value) < 0) {
        throw std::runtime_error(std::string("Failed to set socket_so_sndbuf: ") + aeron_errmsg());
    }
}

void MediaDriverWrapper::setPrintConfiguration(bool value) {
    if (aeron_driver_context_set_print_configuration(context_, value) < 0) {
        throw std::runtime_error(std::string("Failed to set print_configuration: ") + aeron_errmsg());
    }
}

void MediaDriverWrapper::setConductorCpuAffinity(int32_t cpu_id) {
    if (aeron_driver_context_set_conductor_cpu_affinity(context_, cpu_id) < 0) {
        throw std::runtime_error(std::string("Failed to set conductor_cpu_affinity: ") + aeron_errmsg());
    }
}

void MediaDriverWrapper::setSenderCpuAffinity(int32_t cpu_id) {
    if (aeron_driver_context_set_sender_cpu_affinity(context_, cpu_id) < 0) {
        throw std::runtime_error(std::string("Failed to set sender_cpu_affinity: ") + aeron_errmsg());
    }
}

void MediaDriverWrapper::setReceiverCpuAffinity(int32_t cpu_id) {
    if (aeron_driver_context_set_receiver_cpu_affinity(context_, cpu_id) < 0) {
        throw std::runtime_error(std::string("Failed to set receiver_cpu_affinity: ") + aeron_errmsg());
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

int64_t PublicationWrapper::offer(rust::Slice<const uint8_t> buffer) {
    aeron::AtomicBuffer atomic_buffer(const_cast<uint8_t*>(buffer.data()), buffer.size());
    return pub->offer(atomic_buffer);
}

int64_t PublicationWrapper::tryClaim(size_t length, size_t handler_id) {
    aeron::concurrent::logbuffer::BufferClaim bufferClaim;
    int64_t position = pub->tryClaim(static_cast<aeron::util::index_t>(length), bufferClaim);
    if (position > 0) {
        rust::Slice<uint8_t> slice(
            bufferClaim.buffer().buffer() + bufferClaim.offset(),
            bufferClaim.length()
        );
        bool commit = aeron_rs::handle_claim(handler_id, slice);
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

ExclusivePublicationWrapper::ExclusivePublicationWrapper(std::shared_ptr<aeron::ExclusivePublication> pub) : pub(pub) {}

ExclusivePublicationWrapper::~ExclusivePublicationWrapper() {}

int64_t ExclusivePublicationWrapper::offer(rust::Slice<const uint8_t> buffer) {
    aeron::AtomicBuffer atomic_buffer(const_cast<uint8_t*>(buffer.data()), buffer.size());
    return pub->offer(atomic_buffer);
}

int64_t ExclusivePublicationWrapper::tryClaim(size_t length, size_t handler_id) {
    aeron::concurrent::logbuffer::BufferClaim bufferClaim;
    int64_t position = pub->tryClaim(static_cast<aeron::util::index_t>(length), bufferClaim);
    if (position > 0) {
        rust::Slice<uint8_t> slice(
            bufferClaim.buffer().buffer() + bufferClaim.offset(),
            bufferClaim.length()
        );
        bool commit = aeron_rs::handle_claim(handler_id, slice);
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

SubscriptionWrapper::SubscriptionWrapper(std::shared_ptr<aeron::Subscription> sub)
    : sub(sub),
      assembler_([this](aeron::AtomicBuffer& buffer, aeron::util::index_t offset, aeron::util::index_t length, aeron::Header& header) {
          rust::Slice<const uint8_t> slice(buffer.buffer() + offset, length);
          aeron_rs::handle_fragment(this->assembled_handler_id_, slice);
      }),
      controlled_assembler_([this](aeron::AtomicBuffer& buffer, aeron::util::index_t offset, aeron::util::index_t length, aeron::Header& header) -> aeron::ControlledPollAction {
          rust::Slice<const uint8_t> slice(buffer.buffer() + offset, length);
          int32_t action = aeron_rs::handle_controlled_fragment(this->controlled_handler_id_, slice);
          return static_cast<aeron::ControlledPollAction>(action);
      }) {}

SubscriptionWrapper::~SubscriptionWrapper() {}

int SubscriptionWrapper::poll(int fragment_limit, size_t handler_id) {
    auto fragment_handler = [&](const aeron::AtomicBuffer& buffer, aeron::util::index_t offset, aeron::util::index_t length, aeron::Header& header) {
        rust::Slice<const uint8_t> slice(buffer.buffer() + offset, length);
        aeron_rs::handle_fragment(handler_id, slice);
    };
    return sub->poll(fragment_handler, fragment_limit);
}

int SubscriptionWrapper::pollAssembled(int fragment_limit, size_t handler_id) {
    assembled_handler_id_ = handler_id;
    return sub->poll(assembler_.handler(), fragment_limit);
}

int SubscriptionWrapper::controlledPollAssembled(int fragment_limit, size_t handler_id) {
    controlled_handler_id_ = handler_id;
    return sub->controlledPoll(controlled_assembler_.handler(), fragment_limit);
}

bool SubscriptionWrapper::isConnected() const {
    return sub->isConnected();
}

int SubscriptionWrapper::imageCount() const {
    return static_cast<int>(sub->imageCount());
}

std::unique_ptr<ImageWrapper> SubscriptionWrapper::imageByIndex(size_t index) {
    auto image = sub->imageByIndex(index);
    if (!image) {
        throw std::runtime_error("No image at index " + std::to_string(index));
    }
    return std::unique_ptr<ImageWrapper>(new ImageWrapper(image));
}

std::unique_ptr<ImageWrapper> SubscriptionWrapper::imageBySessionId(int32_t session_id) {
    auto image = sub->imageBySessionId(session_id);
    if (!image) {
        throw std::runtime_error("No image for session_id " + std::to_string(session_id));
    }
    return std::unique_ptr<ImageWrapper>(new ImageWrapper(image));
}

// ImageWrapper

ImageWrapper::ImageWrapper(std::shared_ptr<aeron::Image> image)
    : image_(image),
      controlled_assembler_([this](aeron::AtomicBuffer& buffer, aeron::util::index_t offset, aeron::util::index_t length, aeron::Header& header) -> aeron::ControlledPollAction {
          rust::Slice<const uint8_t> slice(buffer.buffer() + offset, length);
          int32_t action = aeron_rs::handle_controlled_fragment(this->controlled_handler_id_, slice);
          return static_cast<aeron::ControlledPollAction>(action);
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
    return rust::String(image_->sourceIdentity());
}

int64_t ImageWrapper::position() const {
    return image_->position();
}

void ImageWrapper::setPosition(int64_t new_position) {
    image_->position(new_position);
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

int ImageWrapper::poll(int fragment_limit, size_t handler_id) {
    auto fragment_handler = [&](const aeron::AtomicBuffer& buffer, aeron::util::index_t offset, aeron::util::index_t length, aeron::Header& header) {
        rust::Slice<const uint8_t> slice(buffer.buffer() + offset, length);
        aeron_rs::handle_fragment(handler_id, slice);
    };
    return image_->poll(fragment_handler, fragment_limit);
}

int ImageWrapper::controlledPollAssembled(int fragment_limit, size_t handler_id) {
    controlled_handler_id_ = handler_id;
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
    return rust::String(aeron->countersReader().getCounterLabel(id));
}

void CountersReaderWrapper::forEach(size_t handler_id) const {
    aeron->countersReader().forEach([&](int32_t counter_id, int32_t type_id, const aeron::concurrent::AtomicBuffer& keyBuffer, const std::string& label) {
        rust::Slice<const uint8_t> key_slice(keyBuffer.buffer(), keyBuffer.capacity());
        aeron_rs::handle_counters_metadata(handler_id, counter_id, type_id, key_slice, rust::String(label));
    });
}

std::unique_ptr<PublicationWrapper> AeronWrapper::addPublication(rust::Str channel, int32_t stream_id) {
    int64_t reg_id = aeron->addPublication(std::string(channel.data(), channel.size()), stream_id);
    
    // We must poll for the publication to be created
    std::shared_ptr<aeron::Publication> pub;
    while (!(pub = aeron->findPublication(reg_id))) {
        std::this_thread::yield();
    }
    
    return std::unique_ptr<PublicationWrapper>(new PublicationWrapper(pub));
}

std::unique_ptr<ExclusivePublicationWrapper> AeronWrapper::addExclusivePublication(rust::Str channel, int32_t stream_id) {
    int64_t reg_id = aeron->addExclusivePublication(std::string(channel.data(), channel.size()), stream_id);

    std::shared_ptr<aeron::ExclusivePublication> pub;
    while (!(pub = aeron->findExclusivePublication(reg_id))) {
        std::this_thread::yield();
    }

    return std::unique_ptr<ExclusivePublicationWrapper>(new ExclusivePublicationWrapper(pub));
}

std::unique_ptr<SubscriptionWrapper> AeronWrapper::addSubscription(rust::Str channel, int32_t stream_id) {
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
    try {
        auto shared_ctx = std::shared_ptr<ContextWrapper>(std::move(context));
        return std::unique_ptr<AeronWrapper>(new AeronWrapper(shared_ctx));
    } catch (const std::exception& e) {
        throw std::runtime_error(std::string("Aeron C++ error: ") + e.what());
    }
}

std::unique_ptr<MediaDriverWrapper> create_media_driver() {
    return std::unique_ptr<MediaDriverWrapper>(new MediaDriverWrapper());
}

} // namespace aeron_rs (close before archive include to avoid double namespace)

#ifdef AERON_ARCHIVE
#include "aeron-rs/src/archive.rs.h"

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

int32_t ArchiveWrapper::listRecordings(int64_t from_recording_id, int32_t record_count, size_t handler_id) {
    auto consumer = [handler_id](aeron::archive::client::RecordingDescriptor& rd) {
        aeron_rs::handle_recording_descriptor(
            handler_id,
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
            ::rust::String(rd.m_strippedChannel),
            ::rust::String(rd.m_originalChannel));
    };
    return archive_->listRecordings(from_recording_id, record_count, consumer);
}

int32_t ArchiveWrapper::listRecordingsForUri(int64_t from_recording_id, int32_t record_count, ::rust::Str channel_fragment, int32_t stream_id, size_t handler_id) {
    auto consumer = [handler_id](aeron::archive::client::RecordingDescriptor& rd) {
        aeron_rs::handle_recording_descriptor(
            handler_id,
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
            ::rust::String(rd.m_strippedChannel),
            ::rust::String(rd.m_originalChannel));
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
    return ::rust::String(archive_->pollForErrorResponse());
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

} // namespace aeron_rs
#endif
