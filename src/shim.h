#pragma once
#include <memory>
#include <string>
#include <Aeron.h>
#include <FragmentAssembler.h>
#include <ControlledFragmentAssembler.h>
#include "rust/cxx.h"

// Forward declarations for C driver types (defined in aeronmd.h)
extern "C" {
    struct aeron_driver_context_stct;
    typedef struct aeron_driver_context_stct aeron_driver_context_t;
    struct aeron_driver_stct;
    typedef struct aeron_driver_stct aeron_driver_t;
}

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

    // Directory
    void setDir(rust::Str dir);
    void setDirDeleteOnStart(bool value);
    void setDirDeleteOnShutdown(bool value);

    // Threading
    void setThreadingMode(int32_t mode);
    void setConductorIdleStrategy(rust::Str name);
    void setSenderIdleStrategy(rust::Str name);
    void setReceiverIdleStrategy(rust::Str name);

    // Buffer sizes
    void setTermBufferLength(size_t value);
    void setIpcTermBufferLength(size_t value);
    void setMtuLength(size_t value);
    void setIpcMtuLength(size_t value);

    // Socket
    void setSocketSoRcvbuf(size_t value);
    void setSocketSoSndbuf(size_t value);

    // Debug
    void setPrintConfiguration(bool value);

    // CPU Affinity
    void setConductorCpuAffinity(int32_t cpu_id);
    void setSenderCpuAffinity(int32_t cpu_id);
    void setReceiverCpuAffinity(int32_t cpu_id);

private:
    aeron_driver_context_t* context_;
    aeron_driver_t* driver_;
};

class PublicationWrapper {
public:
    PublicationWrapper(std::shared_ptr<aeron::Publication> pub);
    ~PublicationWrapper();
    
    int64_t offer(rust::Slice<const uint8_t> buffer);
    int64_t tryClaim(size_t length, size_t handler_id);
    bool isConnected() const;

private:
    std::shared_ptr<aeron::Publication> pub;
};

class ExclusivePublicationWrapper {
public:
    ExclusivePublicationWrapper(std::shared_ptr<aeron::ExclusivePublication> pub);
    ~ExclusivePublicationWrapper();

    int64_t offer(rust::Slice<const uint8_t> buffer);
    int64_t tryClaim(size_t length, size_t handler_id);
    bool isConnected() const;

private:
    std::shared_ptr<aeron::ExclusivePublication> pub;
};

class SubscriptionWrapper {
public:
    SubscriptionWrapper(std::shared_ptr<aeron::Subscription> sub);
    ~SubscriptionWrapper();

    int poll(int fragment_limit, size_t handler_id);
    int pollAssembled(int fragment_limit, size_t handler_id);
    int controlledPollAssembled(int fragment_limit, size_t handler_id);
    bool isConnected() const;

private:
    std::shared_ptr<aeron::Subscription> sub;
    size_t assembled_handler_id_ = 0;
    size_t controlled_handler_id_ = 0;
    aeron::FragmentAssembler assembler_;
    aeron::ControlledFragmentAssembler controlled_assembler_;
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
