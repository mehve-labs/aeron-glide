#include "shim.h"
#include <iostream>
#include <thread>
#include "aeron-rs/src/lib.rs.h"

extern "C" {
#include <aeronmd.h>
}

namespace aeron_rs {

MediaDriverWrapper::MediaDriverWrapper() {}

MediaDriverWrapper::~MediaDriverWrapper() {}

void MediaDriverWrapper::start() {
    aeron_driver_context_t *context = NULL;
    aeron_driver_context_init(&context);
    
    aeron_driver_t *driver = NULL;
    aeron_driver_init(&driver, context);
    aeron_driver_start(driver, false);
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
