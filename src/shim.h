#pragma once
#include <memory>
#include <string>
#include <Aeron.h>
#include "rust/cxx.h"

namespace aeron_rs {

// We wrap the aeron::Context because it's required to initialize Aeron
class ContextWrapper {
public:
    ContextWrapper();
    ~ContextWrapper();
    
    std::shared_ptr<aeron::Context> ctx;
};

class MediaDriverWrapper {
public:
    MediaDriverWrapper();
    ~MediaDriverWrapper();

    void start();
};

class PublicationWrapper {
public:
    PublicationWrapper(std::shared_ptr<aeron::Publication> pub);
    ~PublicationWrapper();
    
    int64_t offer(rust::Slice<const uint8_t> buffer);
    bool isConnected() const;

private:
    std::shared_ptr<aeron::Publication> pub;
};

class ExclusivePublicationWrapper {
public:
    ExclusivePublicationWrapper(std::shared_ptr<aeron::ExclusivePublication> pub);
    ~ExclusivePublicationWrapper();

    int64_t offer(rust::Slice<const uint8_t> buffer);
    bool isConnected() const;

private:
    std::shared_ptr<aeron::ExclusivePublication> pub;
};

class SubscriptionWrapper {
public:
    SubscriptionWrapper(std::shared_ptr<aeron::Subscription> sub);
    ~SubscriptionWrapper();
    
    int poll(int fragment_limit, size_t handler_id);
    bool isConnected() const;

private:
    std::shared_ptr<aeron::Subscription> sub;
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
    void forEach(size_t handler_id) const;

private:
    std::shared_ptr<aeron::Aeron> aeron;
};

class AeronWrapper {
public:
    AeronWrapper(std::shared_ptr<ContextWrapper> context);
    ~AeronWrapper();
    
    void start();
    bool isClosed() const;
    
    std::unique_ptr<PublicationWrapper> addPublication(rust::Str channel, int32_t stream_id);
    std::unique_ptr<ExclusivePublicationWrapper> addExclusivePublication(rust::Str channel, int32_t stream_id);
    std::unique_ptr<SubscriptionWrapper> addSubscription(rust::Str channel, int32_t stream_id);
    std::unique_ptr<CountersReaderWrapper> countersReader() const;
    
private:
    std::shared_ptr<aeron::Aeron> aeron;
};

// Factory functions that cxx can safely bind to
std::unique_ptr<ContextWrapper> create_context();
std::unique_ptr<AeronWrapper> create_aeron(std::unique_ptr<ContextWrapper> context);
std::unique_ptr<MediaDriverWrapper> create_media_driver();

} // namespace aeron_rs
