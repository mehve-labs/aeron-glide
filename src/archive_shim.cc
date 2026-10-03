#include "archive_shim.h"
#include <iostream>
#include "aeron-glide/src/archive/mod.rs.h"

namespace aeron_rs {

namespace {

// Handlers called from C must not unwind (see noUnwind in shim.cc).
template <typename F>
void noUnwind(const char *what, F &&f) noexcept {
    try {
        f();
    } catch (const std::exception &e) {
        std::cerr << "aeron-glide: archive " << what << " handler failed: " << e.what() << std::endl;
    } catch (...) {
        std::cerr << "aeron-glide: archive " << what << " handler failed" << std::endl;
    }
}

rust::Slice<const uint8_t> bytesOf(const char *data, size_t length) {
    return rust::Slice<const uint8_t>(reinterpret_cast<const uint8_t *>(data), length);
}

rust::Slice<const uint8_t> bytesOf(const std::string &s) { return bytesOf(s.data(), s.size()); }

std::string str(rust::Str s) { return detail::cString(s); }

arc::AeronArchive::SourceLocation sourceLocation(int32_t value) {
    return value == 0 ? arc::AeronArchive::SourceLocation::LOCAL : arc::AeronArchive::SourceLocation::REMOTE;
}

// Credentials handed to the archive client, freed by the default onFree (delete[]).
std::pair<const char *, std::uint32_t> ownedCredentials(const rust::Vec<uint8_t> &bytes) {
    if (bytes.empty()) {
        return {nullptr, 0};
    }
    char *copy = new char[bytes.size()];
    std::memcpy(copy, bytes.data(), bytes.size());
    return {copy, static_cast<std::uint32_t>(bytes.size())};
}

RecordingDescriptorInfo descriptorInfo(const arc::RecordingDescriptor &d) {
    RecordingDescriptorInfo info;
    info.control_session_id = d.m_controlSessionId;
    info.correlation_id = d.m_correlationId;
    info.recording_id = d.m_recordingId;
    info.start_timestamp = d.m_startTimestamp;
    info.stop_timestamp = d.m_stopTimestamp;
    info.start_position = d.m_startPosition;
    info.stop_position = d.m_stopPosition;
    info.initial_term_id = d.m_initialTermId;
    info.segment_file_length = d.m_segmentFileLength;
    info.term_buffer_length = d.m_termBufferLength;
    info.mtu_length = d.m_mtuLength;
    info.session_id = d.m_sessionId;
    info.stream_id = d.m_streamId;
    info.stripped_channel = rust::String::lossy(d.m_strippedChannel);
    info.original_channel = rust::String::lossy(d.m_originalChannel);
    info.source_identity = rust::String::lossy(d.m_sourceIdentity);
    return info;
}

// Listing consumers are called from C: keep exceptions (e.g. bad_alloc building
// the descriptor) from unwinding through it, and rethrow them afterwards.
class ConsumerFailure {
public:
    template <typename F>
    void run(F &&f) noexcept {
        if (failure_) {
            return;
        }
        try {
            f();
        } catch (...) {
            failure_ = std::current_exception();
        }
    }
    void rethrow() const {
        if (failure_) {
            std::rethrow_exception(failure_);
        }
    }

private:
    std::exception_ptr failure_;
};

// The C context behind an archive::client::Context (a private member):
// explicit instantiation may name private members.
struct CContextTag {
    using type = aeron_archive_context_t *arc::Context::*;
    friend type member(CContextTag);
};
template <typename Tag, typename Tag::type M>
struct PrivateMember {
    friend typename Tag::type member(Tag) { return M; }
};
template struct PrivateMember<CContextTag, &arc::Context::m_aeron_archive_ctx_t>;

aeron_archive_context_t *cContext(const arc::Context &context) { return context.*member(CContextTag()); }

struct CArchiveTag {
    using type = aeron_archive_t *arc::AeronArchive::*;
    friend type member(CArchiveTag);
};
template struct PrivateMember<CArchiveTag, &arc::AeronArchive::m_aeron_archive_t>;

struct CAsyncConnectTag {
    using type = aeron_archive_async_connect_t *arc::AeronArchive::AsyncConnect::*;
    friend type member(CAsyncConnectTag);
};
template struct PrivateMember<CAsyncConnectTag, &arc::AeronArchive::AsyncConnect::m_async>;

struct CCounterTag {
    using type = aeron_counter_t *aeron::concurrent::AtomicCounter::*;
    friend type member(CCounterTag);
};
template struct PrivateMember<CCounterTag, &aeron::concurrent::AtomicCounter::m_counter>;

// Some archive client loops wait for the client without a deadline or a closed
// check (upstream): fail first if the client is closed. (It can still close
// while one runs; then the client's own timeouts apply.)
void ensureOpen(const std::shared_ptr<aeron::Aeron> &aeron) {
    if (aeron && aeron->isClosed()) {
        throw aeron::util::IllegalStateException("the Aeron client is closed", SOURCEINFO, EPERM);
    }
}

rust::String lossyOrEmpty(const char *s) { return s ? rust::String::lossy(s) : rust::String(); }

std::string aeronDirectoryName(const arc::Context &context) {
    const char *dir = aeron_archive_context_get_aeron_directory_name(cContext(context));
    return dir ? dir : "";
}

} // namespace

// ArchiveContextWrapper

ArchiveContextWrapper::ArchiveContextWrapper() : ctx(std::make_shared<arc::Context>()) {}

void ArchiveContextWrapper::setAeron(const AeronWrapper &client) {
    ctx->aeron(client.sharedAeron());
    lock = client.conductorLock();
}

void ArchiveContextWrapper::setAeronDirectoryName(rust::Str name) { ctx->aeronDirectoryName(str(name)); }
void ArchiveContextWrapper::setControlRequestChannel(rust::Str channel) { ctx->controlRequestChannel(str(channel)); }
void ArchiveContextWrapper::setControlRequestStreamId(int32_t stream_id) { ctx->controlRequestStreamId(stream_id); }
void ArchiveContextWrapper::setControlResponseChannel(rust::Str channel) { ctx->controlResponseChannel(str(channel)); }
void ArchiveContextWrapper::setControlResponseStreamId(int32_t stream_id) { ctx->controlResponseStreamId(stream_id); }
void ArchiveContextWrapper::setRecordingEventsChannel(rust::Str channel) { ctx->recordingEventsChannel(str(channel)); }
void ArchiveContextWrapper::setMessageTimeoutNs(int64_t timeout_ns) {
    ctx->messageTimeoutNs(static_cast<std::uint64_t>(timeout_ns));
}
void ArchiveContextWrapper::setMessageRetryAttempts(uint32_t attempts) { ctx->messageRetryAttempts(attempts); }
void ArchiveContextWrapper::setMaxErrorMessageLength(uint32_t length) { ctx->maxErrorMessageLength(length); }

void ArchiveContextWrapper::setIdleStrategy(IdleFn idle_fn, ReleaseFn release, size_t context) {
    idle = std::make_shared<RustIdleStrategy>(idle_fn, release, context);
    ctx->idleStrategy(*idle);
}

void ArchiveContextWrapper::setDelegatingInvoker(InvokerFn invoker, ReleaseFn release, size_t context) {
    auto owner = std::make_shared<RustOwned>(release, context);
    ctx->delegatingInvoker([owner, invoker]() { noUnwind("delegating invoker", [&] { invoker(owner->ctx()); }); });
}

void ArchiveContextWrapper::setErrorHandler(ErrorFn handler, ReleaseFn release, size_t context) {
    auto owner = std::make_shared<RustOwned>(release, context);
    ctx->errorHandler([owner, handler](const std::exception &e) {
        noUnwind("error", [&] {
            std::string encoded = detail::encode_exception(e);
            handler(owner->ctx(), bytesOf(encoded));
        });
    });
}

void ArchiveContextWrapper::setRecordingSignalConsumer(RecordingSignalFn consumer, ReleaseFn release, size_t context) {
    auto owner = std::make_shared<RustOwned>(release, context);
    ctx->recordingSignalConsumer([owner, consumer](arc::RecordingSignal &signal) {
        noUnwind("recording signal", [&] {
            RecordingSignalInfo info;
            info.control_session_id = signal.m_controlSessionId;
            info.recording_id = signal.m_recordingId;
            info.subscription_id = signal.m_subscriptionId;
            info.position = signal.m_position;
            info.code = signal.m_recordingSignalCode;
            consumer(owner->ctx(), info);
        });
    });
}

void ArchiveContextWrapper::setCredentialsSupplier(
    CredentialsFn credentials, ChallengeFn on_challenge, ReleaseFn release, size_t context) {
    auto owner = std::make_shared<RustOwned>(release, context);
    arc::CredentialsSupplier supplier(
        [owner, credentials]() { return ownedCredentials(credentials(owner->ctx())); },
        [owner, on_challenge](std::pair<const char *, std::uint32_t> challenge) {
            return ownedCredentials(on_challenge(owner->ctx(), bytesOf(challenge.first, challenge.second)));
        });
    ctx->credentialsSupplier(supplier);
}

void ArchiveContextWrapper::conclude() {
    if (ctx->aeron()) {
        if (ctx->aeron()->usesAgentInvoker() && !std::dynamic_pointer_cast<InvokingIdleStrategy>(idle)) {
            idle = std::make_shared<InvokingIdleStrategy>(ctx->aeron(), idle);
            ctx->idleStrategy(*idle);
        }
        return;
    }
    aeron::Context clientContext;
    clientContext.errorHandler([](const std::exception &e) {
        std::cerr << "aeron-glide: Aeron archive client error: " << e.what() << std::endl;
    });
    const std::string dir = aeronDirectoryName(*ctx);
    if (!dir.empty()) {
        clientContext.aeronDir(dir);
    }
    ctx->aeron(aeron::Aeron::connect(clientContext));
}

std::unique_ptr<ArchiveContextWrapper> create_archive_context() {
    return std::unique_ptr<ArchiveContextWrapper>(new ArchiveContextWrapper());
}

namespace {
std::unique_ptr<ArchiveWrapper> wrapArchive(std::shared_ptr<arc::AeronArchive> archive, ArchiveContextWrapper &context) {
    if (context.idle) {
        // Upstream: the archive's own context starts with the default (yielding)
        // idle strategy instead of the one configured.
        const_cast<arc::Context &>(archive->context()).idleStrategy(*context.idle);
    }
    return std::unique_ptr<ArchiveWrapper>(new ArchiveWrapper(std::move(archive), context.lock, context.idle));
}
} // namespace

std::unique_ptr<ArchiveWrapper> archive_connect(ArchiveContextWrapper &context) {
    context.conclude();
    ensureOpen(context.ctx->aeron());
    ConductorLock::Guard guard(context.lock);
    return wrapArchive(arc::AeronArchive::connect(*context.ctx), context);
}

// ArchiveAsyncConnectWrapper

ArchiveAsyncConnectWrapper::ArchiveAsyncConnectWrapper(std::unique_ptr<ArchiveContextWrapper> context)
    : context_(std::move(context)) {
    context_->conclude();
    ensureOpen(context_->ctx->aeron());
    ConductorLock::Guard guard(context_->lock);
    async_ = arc::AeronArchive::asyncConnect(*context_->ctx);
}

// Upstream (1.53.3): AsyncConnect has no destructor, so a pending connect would
// leak; release it with the C function its failure path uses.
ArchiveAsyncConnectWrapper::~ArchiveAsyncConnectWrapper() {
    ConductorLock::Guard guard(context_->lock, true);
    aeron_archive_async_connect_t *&pending = (*async_).*member(CAsyncConnectTag());
    if (pending != nullptr) {
        aeron_archive_async_connect_delete(pending);
        pending = nullptr;
    }
    async_.reset();
}

std::unique_ptr<ArchiveWrapper> ArchiveAsyncConnectWrapper::poll() {
    ConductorLock::Guard guard(context_->lock);
    aeron_archive_async_connect_t *&pending = (*async_).*member(CAsyncConnectTag());
    if (pending == nullptr) {
        throw aeron::util::IllegalStateException("the archive connection already completed", SOURCEINFO, EPERM);
    }
    const auto &aeron = context_->ctx->aeron();
    ensureOpen(aeron);
    if (aeron->usesAgentInvoker()) {
        // The blocking connect runs the conductor while it waits; so does poll.
        aeron->conductorAgentInvoker().invoke();
    }
    std::shared_ptr<arc::AeronArchive> archive;
    try {
        archive = async_->poll();
    } catch (...) {
        // A failed poll freed the C connection (upstream keeps the pointer).
        pending = nullptr;
        throw;
    }
    return archive ? wrapArchive(std::move(archive), *context_) : nullptr;
}

std::unique_ptr<ArchiveAsyncConnectWrapper> archive_async_connect(std::unique_ptr<ArchiveContextWrapper> context) {
    return std::unique_ptr<ArchiveAsyncConnectWrapper>(new ArchiveAsyncConnectWrapper(std::move(context)));
}

// ArchiveWrapper

ArchiveWrapper::ArchiveWrapper(std::shared_ptr<arc::AeronArchive> archive, std::shared_ptr<ConductorLock> lock,
                               std::shared_ptr<IdleStrategy> idle)
    : archive_(std::move(archive)), lock_(std::move(lock)), idle_(std::move(idle)) {}

// Closing the archive closes its publication and subscription (conductor work).
ArchiveWrapper::~ArchiveWrapper() {
    ConductorLock::Guard guard(lock_, true);
    archive_.reset();
}

// Read through the C getters: the C++ string getters construct a std::string
// from the C value, which is null for an unset channel (upstream bug).
ArchiveContextInfo ArchiveWrapper::context() const {
    const arc::Context &context = archive_->context();
    aeron_archive_context_t *c = cContext(context);
    ArchiveContextInfo info;
    info.aeron_directory_name = lossyOrEmpty(aeron_archive_context_get_aeron_directory_name(c));
    info.control_request_channel = lossyOrEmpty(aeron_archive_context_get_control_request_channel(c));
    info.control_request_stream_id = aeron_archive_context_get_control_request_stream_id(c);
    info.control_response_channel = lossyOrEmpty(aeron_archive_context_get_control_response_channel(c));
    info.control_response_stream_id = aeron_archive_context_get_control_response_stream_id(c);
    info.recording_events_channel = lossyOrEmpty(aeron_archive_context_get_recording_events_channel(c));
    info.message_timeout_ns = static_cast<int64_t>(aeron_archive_context_get_message_timeout_ns(c));
    info.message_retry_attempts = aeron_archive_context_get_message_retry_attempts(c);
    info.max_error_message_length = context.maxErrorMessageLength();
    return info;
}

int64_t ArchiveWrapper::archiveId() const { return archive_->archiveId(); }
int64_t ArchiveWrapper::controlSessionId() const { return archive_->controlSessionId(); }

int32_t ArchiveWrapper::pollForRecordingSignals() const {
    ensureOpen(archive_->context().aeron());
    ConductorLock::Guard guard(lock_);
    return archive_->pollForRecordingSignals();
}

// Through the C function with a local buffer: the C++ method reads its shared
// buffer after the archive's lock is released (racing concurrent callers), and
// overreads a zero-length one.
rust::String ArchiveWrapper::pollForErrorResponse() const {
    ensureOpen(archive_->context().aeron());
    ConductorLock::Guard guard(lock_);
    // At least the default length: a shorter buffer truncates messages to nothing.
    std::vector<char> buffer(std::max<std::uint32_t>(archive_->context().maxErrorMessageLength(), 1000) + 1, '\0');
    if (aeron_archive_poll_for_error_response((*archive_).*member(CArchiveTag()), buffer.data(), buffer.size() - 1) < 0) {
        using namespace aeron::util;
        ARCHIVE_MAP_ERRNO_TO_SOURCED_EXCEPTION_AND_THROW;
    }
    return rust::String::lossy(std::string(buffer.data(), strnlen(buffer.data(), buffer.size())));
}

void ArchiveWrapper::checkForErrorResponse() const {
    ensureOpen(archive_->context().aeron());
    ConductorLock::Guard guard(lock_);
    archive_->checkForErrorResponse();
}

std::unique_ptr<PublicationWrapper> ArchiveWrapper::addRecordedPublication(rust::Str channel, int32_t stream_id) const {
    ensureOpen(archive_->context().aeron());
    ConductorLock::Guard guard(lock_);
    auto publication = archive_->addRecordedPublication(str(channel), stream_id);
    return std::unique_ptr<PublicationWrapper>(new PublicationWrapper(std::move(publication), lock_));
}

std::unique_ptr<ExclusivePublicationWrapper> ArchiveWrapper::addRecordedExclusivePublication(
    rust::Str channel, int32_t stream_id) const {
    ensureOpen(archive_->context().aeron());
    ConductorLock::Guard guard(lock_);
    auto publication = archive_->addRecordedExclusivePublication(str(channel), stream_id);
    return std::unique_ptr<ExclusivePublicationWrapper>(new ExclusivePublicationWrapper(std::move(publication), lock_));
}

int64_t ArchiveWrapper::startRecording(rust::Str channel, int32_t stream_id, int32_t source_location,
                                       bool auto_stop) const {
    ensureOpen(archive_->context().aeron());
    ConductorLock::Guard guard(lock_);
    return archive_->startRecording(str(channel), stream_id, sourceLocation(source_location), auto_stop);
}

int64_t ArchiveWrapper::extendRecording(int64_t recording_id, rust::Str channel, int32_t stream_id,
                                        int32_t source_location, bool auto_stop) const {
    ensureOpen(archive_->context().aeron());
    ConductorLock::Guard guard(lock_);
    return archive_->extendRecording(recording_id, str(channel), stream_id, sourceLocation(source_location), auto_stop);
}

void ArchiveWrapper::stopRecording(int64_t subscription_id) const {
    ensureOpen(archive_->context().aeron());
    ConductorLock::Guard guard(lock_);
    archive_->stopRecording(subscription_id);
}

bool ArchiveWrapper::tryStopRecording(int64_t subscription_id) const {
    ensureOpen(archive_->context().aeron());
    ConductorLock::Guard guard(lock_);
    return archive_->tryStopRecording(subscription_id);
}

void ArchiveWrapper::stopRecordingByChannelAndStream(rust::Str channel, int32_t stream_id) const {
    ensureOpen(archive_->context().aeron());
    ConductorLock::Guard guard(lock_);
    archive_->stopRecording(str(channel), stream_id);
}

bool ArchiveWrapper::tryStopRecordingByChannelAndStream(rust::Str channel, int32_t stream_id) const {
    ensureOpen(archive_->context().aeron());
    ConductorLock::Guard guard(lock_);
    return archive_->tryStopRecording(str(channel), stream_id);
}

bool ArchiveWrapper::tryStopRecordingByIdentity(int64_t recording_id) const {
    ensureOpen(archive_->context().aeron());
    ConductorLock::Guard guard(lock_);
    return archive_->tryStopRecordingByIdentity(recording_id);
}

void ArchiveWrapper::stopRecordingPublication(const PublicationWrapper &publication) const {
    ensureOpen(archive_->context().aeron());
    ConductorLock::Guard guard(lock_);
    archive_->stopRecording(publication.shared());
}

void ArchiveWrapper::stopRecordingExclusivePublication(const ExclusivePublicationWrapper &publication) const {
    ensureOpen(archive_->context().aeron());
    ConductorLock::Guard guard(lock_);
    archive_->stopRecording(publication.shared());
}

int64_t ArchiveWrapper::purgeRecording(int64_t recording_id) const {
    ensureOpen(archive_->context().aeron());
    ConductorLock::Guard guard(lock_);
    return archive_->purgeRecording(recording_id);
}

void ArchiveWrapper::updateChannel(int64_t recording_id, rust::Str channel) const {
    ensureOpen(archive_->context().aeron());
    ConductorLock::Guard guard(lock_);
    archive_->updateChannel(recording_id, str(channel));
}

int64_t ArchiveWrapper::truncateRecording(int64_t recording_id, int64_t position) const {
    ensureOpen(archive_->context().aeron());
    ConductorLock::Guard guard(lock_);
    return archive_->truncateRecording(recording_id, position);
}

int64_t ArchiveWrapper::getRecordingPosition(int64_t recording_id) const {
    ensureOpen(archive_->context().aeron());
    ConductorLock::Guard guard(lock_);
    return archive_->getRecordingPosition(recording_id);
}

int64_t ArchiveWrapper::getStartPosition(int64_t recording_id) const {
    ensureOpen(archive_->context().aeron());
    ConductorLock::Guard guard(lock_);
    return archive_->getStartPosition(recording_id);
}

int64_t ArchiveWrapper::getStopPosition(int64_t recording_id) const {
    ensureOpen(archive_->context().aeron());
    ConductorLock::Guard guard(lock_);
    return archive_->getStopPosition(recording_id);
}

int64_t ArchiveWrapper::getMaxRecordedPosition(int64_t recording_id) const {
    ensureOpen(archive_->context().aeron());
    ConductorLock::Guard guard(lock_);
    return archive_->getMaxRecordedPosition(recording_id);
}

int64_t ArchiveWrapper::findLastMatchingRecording(int64_t min_recording_id, rust::Str channel_fragment,
                                                  int32_t stream_id, int32_t session_id) const {
    ensureOpen(archive_->context().aeron());
    ConductorLock::Guard guard(lock_);
    return archive_->findLastMatchingRecording(min_recording_id, str(channel_fragment), stream_id, session_id);
}

int32_t ArchiveWrapper::listRecording(int64_t recording_id, RecordingDescriptorFn handler, size_t ctx) const {
    ensureOpen(archive_->context().aeron());
    ConductorLock::Guard guard(lock_);
    ConsumerFailure failure;
    int32_t count = archive_->listRecording(recording_id, [&](arc::RecordingDescriptor &d) {
        failure.run([&] { handler(ctx, descriptorInfo(d)); });
    });
    failure.rethrow();
    return count;
}

int32_t ArchiveWrapper::listRecordings(int64_t from_recording_id, int32_t record_count,
                                       RecordingDescriptorFn handler, size_t ctx) const {
    ensureOpen(archive_->context().aeron());
    ConductorLock::Guard guard(lock_);
    ConsumerFailure failure;
    int32_t count = archive_->listRecordings(from_recording_id, record_count, [&](arc::RecordingDescriptor &d) {
        failure.run([&] { handler(ctx, descriptorInfo(d)); });
    });
    failure.rethrow();
    return count;
}

int32_t ArchiveWrapper::listRecordingsForUri(int64_t from_recording_id, int32_t record_count,
                                             rust::Str channel_fragment, int32_t stream_id,
                                             RecordingDescriptorFn handler, size_t ctx) const {
    ensureOpen(archive_->context().aeron());
    ConductorLock::Guard guard(lock_);
    ConsumerFailure failure;
    int32_t count = archive_->listRecordingsForUri(
        from_recording_id, record_count, str(channel_fragment), stream_id,
        [&](arc::RecordingDescriptor &d) { failure.run([&] { handler(ctx, descriptorInfo(d)); }); });
    failure.rethrow();
    return count;
}

int32_t ArchiveWrapper::listRecordingSubscriptions(int32_t pseudo_index, int32_t subscription_count,
                                                   rust::Str channel_fragment, int32_t stream_id,
                                                   bool apply_stream_id, RecordingSubscriptionFn handler,
                                                   size_t ctx) const {
    ensureOpen(archive_->context().aeron());
    ConductorLock::Guard guard(lock_);
    ConsumerFailure failure;
    int32_t count = archive_->listRecordingSubscriptions(
        pseudo_index, subscription_count, str(channel_fragment), stream_id, apply_stream_id,
        [&](arc::RecordingSubscriptionDescriptor &d) {
            failure.run([&] {
                RecordingSubscriptionInfo info;
                info.control_session_id = d.m_controlSessionId;
                info.correlation_id = d.m_correlationId;
                info.subscription_id = d.m_subscriptionId;
                info.stream_id = d.m_streamId;
                info.stripped_channel = rust::String::lossy(d.m_strippedChannel);
                handler(ctx, info);
            });
        });
    failure.rethrow();
    return count;
}

namespace {
arc::ReplayParams replayParams(int64_t position, int64_t length, int32_t bounding_limit_counter_id,
                               int32_t file_io_max_length, int64_t replay_token,
                               int64_t subscription_registration_id) {
    arc::ReplayParams params;
    params.position(position)
        .length(length)
        .boundingLimitCounterId(bounding_limit_counter_id)
        .fileIoMaxLength(file_io_max_length)
        .replayToken(replay_token)
        .subscriptionRegistrationId(subscription_registration_id);
    return params;
}
} // namespace

int64_t ArchiveWrapper::startReplay(int64_t recording_id, rust::Str channel, int32_t stream_id, int64_t position,
                                    int64_t length, int32_t bounding_limit_counter_id, int32_t file_io_max_length,
                                    int64_t replay_token, int64_t subscription_registration_id) const {
    ensureOpen(archive_->context().aeron());
    ConductorLock::Guard guard(lock_);
    auto params = replayParams(position, length, bounding_limit_counter_id, file_io_max_length, replay_token,
                               subscription_registration_id);
    return archive_->startReplay(recording_id, str(channel), stream_id, params);
}

std::unique_ptr<SubscriptionWrapper> ArchiveWrapper::replay(
    int64_t recording_id, rust::Str channel, int32_t stream_id, int64_t position, int64_t length,
    int32_t bounding_limit_counter_id, int32_t file_io_max_length, int64_t replay_token,
    int64_t subscription_registration_id) const {
    ensureOpen(archive_->context().aeron());
    ConductorLock::Guard guard(lock_);
    auto params = replayParams(position, length, bounding_limit_counter_id, file_io_max_length, replay_token,
                               subscription_registration_id);
    auto subscription = archive_->replay(recording_id, str(channel), stream_id, params);
    return std::unique_ptr<SubscriptionWrapper>(new SubscriptionWrapper(std::move(subscription), lock_));
}

void ArchiveWrapper::stopReplay(int64_t replay_session_id) const {
    ensureOpen(archive_->context().aeron());
    ConductorLock::Guard guard(lock_);
    archive_->stopReplay(replay_session_id);
}

void ArchiveWrapper::stopAllReplays(int64_t recording_id) const {
    ensureOpen(archive_->context().aeron());
    ConductorLock::Guard guard(lock_);
    archive_->stopAllReplays(recording_id);
}

int64_t ArchiveWrapper::replicate(int64_t src_recording_id, int32_t src_control_stream_id,
                                  rust::Str src_control_channel, int64_t stop_position, int64_t dst_recording_id,
                                  rust::Str live_destination, rust::Str replication_channel, int64_t channel_tag_id,
                                  int64_t subscription_tag_id, int32_t file_io_max_length,
                                  int32_t replication_session_id,
                                  rust::Slice<const uint8_t> encoded_credentials) const {
    ensureOpen(archive_->context().aeron());
    ConductorLock::Guard guard(lock_);
    // The params keep a pointer to the credentials; they outlive the call.
    std::string credentials(reinterpret_cast<const char *>(encoded_credentials.data()), encoded_credentials.size());
    arc::ReplicationParams params;
    params.stopPosition(stop_position)
        .dstRecordingId(dst_recording_id)
        .liveDestination(str(live_destination))
        .replicationChannel(str(replication_channel))
        .channelTagId(channel_tag_id)
        .subscriptionTagId(subscription_tag_id)
        .fileIoMaxLength(file_io_max_length)
        .replicationSessionId(replication_session_id);
    // Always set: upstream's ReplicationParams() points the C params at an
    // uninitialised credentials struct, which replicate() then copies from.
    params.encodedCredentials(
        {credentials.empty() ? nullptr : credentials.data(), static_cast<std::uint32_t>(credentials.size())});
    return archive_->replicate(src_recording_id, src_control_stream_id, str(src_control_channel), params);
}

void ArchiveWrapper::stopReplication(int64_t replication_id) const {
    ensureOpen(archive_->context().aeron());
    ConductorLock::Guard guard(lock_);
    archive_->stopReplication(replication_id);
}

bool ArchiveWrapper::tryStopReplication(int64_t replication_id) const {
    ensureOpen(archive_->context().aeron());
    ConductorLock::Guard guard(lock_);
    return archive_->tryStopReplication(replication_id);
}

void ArchiveWrapper::detachSegments(int64_t recording_id, int64_t new_start_position) const {
    ensureOpen(archive_->context().aeron());
    ConductorLock::Guard guard(lock_);
    archive_->detachSegments(recording_id, new_start_position);
}

int64_t ArchiveWrapper::deleteDetachedSegments(int64_t recording_id) const {
    ensureOpen(archive_->context().aeron());
    ConductorLock::Guard guard(lock_);
    return archive_->deleteDetachedSegments(recording_id);
}

int64_t ArchiveWrapper::purgeSegments(int64_t recording_id, int64_t new_start_position) const {
    ensureOpen(archive_->context().aeron());
    ConductorLock::Guard guard(lock_);
    return archive_->purgeSegments(recording_id, new_start_position);
}

int64_t ArchiveWrapper::attachSegments(int64_t recording_id) const {
    ensureOpen(archive_->context().aeron());
    ConductorLock::Guard guard(lock_);
    return archive_->attachSegments(recording_id);
}

int64_t ArchiveWrapper::migrateSegments(int64_t src_recording_id, int64_t dst_recording_id) const {
    ensureOpen(archive_->context().aeron());
    ConductorLock::Guard guard(lock_);
    return archive_->migrateSegments(src_recording_id, dst_recording_id);
}


// RecordingPos

int32_t recordingPosFindCounterIdByRecordingId(const CountersReaderWrapper &reader, int64_t recording_id) {
    return arc::RecordingPos::findCounterIdByRecordingId(reader.reader(), recording_id);
}

int32_t recordingPosFindCounterIdBySessionId(const CountersReaderWrapper &reader, int32_t session_id) {
    return arc::RecordingPos::findCounterIdBySessionId(reader.reader(), session_id);
}

int64_t recordingPosGetRecordingId(const CountersReaderWrapper &reader, int32_t counter_id) {
    return arc::RecordingPos::getRecordingId(reader.reader(), counter_id);
}

// Read here: the C function trusts the source identity length in the counter's
// key (shared memory), and a negative one overflows its stack buffer.
rust::String recordingPosGetSourceIdentity(const CountersReaderWrapper &wrapper, int32_t counter_id) {
    aeron::CountersReader &reader = wrapper.reader();
    if (counter_id < 0 || counter_id > reader.maxCounterId()) {
        throw aeron::util::IllegalArgumentException(
            "counter id " + std::to_string(counter_id) + " out of range: maxCounterId=" +
                std::to_string(reader.maxCounterId()),
            SOURCEINFO, EINVAL);
    }
    if (reader.getCounterState(counter_id) != aeron::CountersReader::RECORD_ALLOCATED ||
        reader.getCounterTypeId(counter_id) != aeron::AeronCounters::ARCHIVE_RECORDING_POSITION_TYPE_ID) {
        return rust::String();
    }
    aeron::AtomicBuffer metadata = reader.metaDataBuffer();
    // Key: recording id (8), session id (4), source identity length (4), source identity.
    const auto key = aeron::CountersReader::metadataOffset(counter_id) + aeron::CountersReader::KEY_OFFSET;
    const std::int32_t maxLength = aeron::CountersReader::MAX_KEY_LENGTH - 16;
    const std::int32_t length = std::min(std::max(metadata.getInt32(key + 12), 0), maxLength);
    std::string identity(length, '\0');
    for (std::int32_t i = 0; i < length; i++) {
        identity[i] = static_cast<char>(metadata.getUInt8(key + 16 + i));
    }
    return rust::String::lossy(identity);
}

bool recordingPosIsActive(const CountersReaderWrapper &reader, int32_t counter_id, int64_t recording_id) {
    return arc::RecordingPos::isActive(reader.reader(), counter_id, recording_id);
}

// ReplayMergeWrapper

ReplayMergeWrapper::ReplayMergeWrapper(
    const std::shared_ptr<aeron::Subscription> &subscription, std::shared_ptr<AssemblerState> assembly,
    const std::shared_ptr<arc::AeronArchive> &archive, std::shared_ptr<IdleStrategy> idle,
    const std::string &replayChannel, const std::string &replayDestination, const std::string &liveDestination,
    int64_t recordingId, int64_t startPosition, int64_t mergeProgressTimeoutMs)
    : subscription_(subscription), assembly_(std::move(assembly)), idle_(std::move(idle)) {
    ConductorLock::Guard guard(assembly_->conductorLock);
    merge_ = std::make_unique<arc::ReplayMerge>(subscription, archive, replayChannel, replayDestination,
                                                liveDestination, recordingId, startPosition, aeron::currentTimeMillis,
                                                mergeProgressTimeoutMs);
}

// The merge may hold the last reference to its subscription.
ReplayMergeWrapper::~ReplayMergeWrapper() {
    ConductorLock::Guard guard(assembly_->conductorLock, true);
    merge_.reset();
    subscription_.reset();
}

int ReplayMergeWrapper::doWork() {
    ConductorLock::Guard guard(assembly_->conductorLock);
    return merge_->doWork();
}

int ReplayMergeWrapper::poll(int fragment_limit, FragmentFn handler, size_t ctx) {
    ConductorLock::Guard guard(assembly_->conductorLock);
    auto fragment_handler = [&](const aeron::AtomicBuffer &buffer, aeron::util::index_t offset,
                                aeron::util::index_t length, aeron::Header &header) {
        rust::Slice<const uint8_t> slice(buffer.buffer() + offset, length);
        handler(ctx, slice, header);
    };
    return merge_->poll(fragment_handler, fragment_limit);
}

std::unique_ptr<ImageWrapper> ReplayMergeWrapper::image() {
    try {
        auto img = merge_->image();
        return img ? std::unique_ptr<ImageWrapper>(new ImageWrapper(img, subscription_, assembly_)) : nullptr;
    } catch (...) {
        return nullptr;
    }
}

bool ReplayMergeWrapper::isMerged() const { return merge_->isMerged(); }
bool ReplayMergeWrapper::hasFailed() const { return merge_->hasFailed(); }
bool ReplayMergeWrapper::isLiveAdded() const { return merge_->isLiveAdded(); }

std::unique_ptr<ReplayMergeWrapper> create_replay_merge(
    SubscriptionWrapper &subscription, const ArchiveWrapper &archive, rust::Str replay_channel,
    rust::Str replay_destination, rust::Str live_destination, int64_t recording_id, int64_t start_position,
    int64_t merge_progress_timeout_ms) {
    // The merge runs archive requests and adds destinations to the subscription
    // under the subscription's conductor lock: in agent invoker mode both must
    // belong to the same client.
    const auto &subscriptionLock = subscription.sharedAssembly()->conductorLock;
    const auto &archiveLock = archive.conductorLock();
    if (archiveLock && archiveLock != subscriptionLock &&
        (archiveLock->enabled() || (subscriptionLock && subscriptionLock->enabled()))) {
        throw aeron::util::IllegalArgumentException(
            "in agent invoker mode the subscription must belong to the archive's client", SOURCEINFO, EINVAL);
    }
    return std::make_unique<ReplayMergeWrapper>(
        subscription.sharedSubscription(), subscription.sharedAssembly(), archive.sharedArchive(),
        archive.sharedIdle(), str(replay_channel), str(replay_destination), str(live_destination), recording_id, start_position,
        merge_progress_timeout_ms);
}

// PersistentSubscription

PersistentSubscriptionContextWrapper::PersistentSubscriptionContextWrapper() = default;

void PersistentSubscriptionContextWrapper::setArchiveContext(std::unique_ptr<ArchiveContextWrapper> context) {
    archive = std::move(context);
    ctx.archiveContext(archive->ctx);
}

void PersistentSubscriptionContextWrapper::setAeron(const AeronWrapper &aeronClient) {
    ctx.aeron(aeronClient.sharedAeron());
    lock = aeronClient.conductorLock();
    client = aeronClient.sharedAeron();
    hasClient = true;
}

void PersistentSubscriptionContextWrapper::setAeronDirectoryName(rust::Str name) {
    aeronDir = str(name);
    ctx.aeronDirectoryName(aeronDir);
}
void PersistentSubscriptionContextWrapper::setRecordingId(int64_t recording_id) { ctx.recordingId(recording_id); }
void PersistentSubscriptionContextWrapper::setStartPosition(int64_t position) { ctx.startPosition(position); }
void PersistentSubscriptionContextWrapper::setLiveChannel(rust::Str channel) { ctx.liveChannel(str(channel)); }
void PersistentSubscriptionContextWrapper::setLiveStreamId(int32_t stream_id) { ctx.liveStreamId(stream_id); }
void PersistentSubscriptionContextWrapper::setReplayChannel(rust::Str channel) { ctx.replayChannel(str(channel)); }
void PersistentSubscriptionContextWrapper::setReplayStreamId(int32_t stream_id) { ctx.replayStreamId(stream_id); }

void PersistentSubscriptionContextWrapper::setCounter(int32_t which, const CounterWrapper &counter) {
    if (counter.sharedCounter()->c_counter() == nullptr) {
        throw aeron::util::IllegalArgumentException(
            "a counter handle from a CountersReader cannot be used, only an added counter", SOURCEINFO, EINVAL);
    }
    counters.push_back(counter.sharedCounter());
    switch (which) {
        case 0: ctx.stateCounter(counter.sharedCounter()); break;
        case 1: ctx.joinDifferenceCounter(counter.sharedCounter()); break;
        case 2: ctx.liveLeftCounter(counter.sharedCounter()); break;
        default: ctx.liveJoinedCounter(counter.sharedCounter()); break;
    }
    keepalive.push_back(counter.keepalive());
}

void PersistentSubscriptionContextWrapper::setOnLiveJoined(InvokerFn callback, ReleaseFn release, size_t context) {
    auto owner = std::make_shared<RustOwned>(release, context);
    ctx.onLiveJoined([owner, callback]() { callback(owner->ctx()); });
}

void PersistentSubscriptionContextWrapper::setOnLiveLeft(InvokerFn callback, ReleaseFn release, size_t context) {
    auto owner = std::make_shared<RustOwned>(release, context);
    ctx.onLiveLeft([owner, callback]() { callback(owner->ctx()); });
}

void PersistentSubscriptionContextWrapper::setOnError(PersistentErrorFn callback, ReleaseFn release, size_t context) {
    auto owner = std::make_shared<RustOwned>(release, context);
    ctx.onError([owner, callback](int code, const std::string &message) {
        callback(owner->ctx(), code, bytesOf(message));
    });
}

std::unique_ptr<PersistentSubscriptionContextWrapper> create_persistent_subscription_context() {
    return std::unique_ptr<PersistentSubscriptionContextWrapper>(new PersistentSubscriptionContextWrapper());
}

std::unique_ptr<PersistentSubscriptionWrapper> create_persistent_subscription(
    std::unique_ptr<PersistentSubscriptionContextWrapper> context) {
    if (!context->archive) {
        throw aeron::util::IllegalArgumentException("archive context must be set", SOURCEINFO, EINVAL);
    }
    if (!context->hasClient) {
        if (context->archive->ctx->aeron()) {
            context->ctx.aeron(context->archive->ctx->aeron());
            context->lock = context->archive->lock;
            context->client = context->archive->ctx->aeron();
        } else {
            // As the C client would (an agent invoker client, driven by poll), but
            // with a non-exiting error handler instead of Aeron's default.
            aeron::Context clientContext;
            clientContext.useConductorAgentInvoker(true);
            clientContext.errorHandler([](const std::exception &e) {
                std::cerr << "aeron-glide: persistent subscription client error: " << e.what() << std::endl;
            });
            clientContext.aeronDir(
                context->aeronDir.empty() ? aeronDirectoryName(*context->archive->ctx) : context->aeronDir);
            context->client = aeron::Aeron::connect(clientContext);
            context->ctx.aeron(context->client);
        }
        context->hasClient = true;
    }
    // The C context closes the counters with their client's conductor, under this
    // client's lock: they must belong to it.
    for (std::size_t i = 0; i < context->counters.size(); i++) {
        if (context->keepalive[i].get() != context->client.get()) {
            throw aeron::util::IllegalArgumentException(
                "the counters must belong to the persistent subscription's client", SOURCEINFO, EINVAL);
        }
    }
    ConductorLock::Guard guard(context->lock);
    // The C context takes the counters over (it closes them when it is closed,
    // also if creating fails): stop their C++ handles from closing them again.
    auto handOver = [&] {
        for (auto &counter : context->counters) {
            (*counter).*member(CCounterTag()) = nullptr;
        }
    };
    std::shared_ptr<arc::PersistentSubscription> subscription;
    try {
        subscription = arc::PersistentSubscription::create(context->ctx);
    } catch (...) {
        handOver();
        throw;
    }
    handOver();
    return std::unique_ptr<PersistentSubscriptionWrapper>(
        new PersistentSubscriptionWrapper(std::move(subscription), std::move(context)));
}

PersistentSubscriptionWrapper::PersistentSubscriptionWrapper(
    std::shared_ptr<arc::PersistentSubscription> subscription,
    std::unique_ptr<PersistentSubscriptionContextWrapper> context)
    : subscription_(std::move(subscription)), context_(std::move(context)) {}

// Closing it closes its subscriptions and archive connection; its counters may
// hold the last reference to the client.
PersistentSubscriptionWrapper::~PersistentSubscriptionWrapper() {
    ConductorLock::Guard guard(context_->lock, true);
    subscription_.reset();
    context_.reset();
}

int PersistentSubscriptionWrapper::poll(int fragment_limit, FragmentFn handler, size_t ctx) {
    ensureOpen(context_->client);
    ConductorLock::Guard guard(context_->lock);
    auto fragment_handler = [&](const aeron::AtomicBuffer &buffer, aeron::util::index_t offset,
                                aeron::util::index_t length, aeron::Header &header) {
        rust::Slice<const uint8_t> slice(buffer.buffer() + offset, length);
        handler(ctx, slice, header);
    };
    return subscription_->poll(fragment_handler, fragment_limit);
}

int PersistentSubscriptionWrapper::controlledPoll(int fragment_limit, ControlledFragmentFn handler, size_t ctx) {
    ensureOpen(context_->client);
    ConductorLock::Guard guard(context_->lock);
    auto fragment_handler = [&](const aeron::AtomicBuffer &buffer, aeron::util::index_t offset,
                                aeron::util::index_t length, aeron::Header &header) {
        rust::Slice<const uint8_t> slice(buffer.buffer() + offset, length);
        return static_cast<aeron::ControlledPollAction>(handler(ctx, slice, header));
    };
    return subscription_->controlledPoll(fragment_handler, fragment_limit);
}

bool PersistentSubscriptionWrapper::isLive() const { return subscription_->isLive(); }
bool PersistentSubscriptionWrapper::isReplaying() const { return subscription_->isReplaying(); }
bool PersistentSubscriptionWrapper::hasFailed() const { return subscription_->hasFailed(); }

} // namespace aeron_rs
