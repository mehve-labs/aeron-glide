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

} // namespace aeron_rs
