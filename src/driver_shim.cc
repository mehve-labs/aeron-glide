#include "driver_shim.h"
#include "aeron-glide/src/driver.rs.h"

extern "C" {
#include <aeronmd.h>
}

namespace aeron_rs {

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

// aeron_driver.h, not in the public aeronmd.h.
extern "C" int aeron_driver_apply_cpuset_affinity(aeron_driver_context_t *context);

void MediaDriverWrapper::start(bool manual_main_loop) {
    if (driver_ != nullptr) {
        throw aeron::util::IllegalStateException("media driver already started", SOURCEINFO, EPERM);
    }
    // Aeron checks this product only for values from environment variables.
    const uint64_t nak_delay = aeron_driver_context_get_nak_unicast_delay_ns(context_);
    const uint64_t nak_ratio = aeron_driver_context_get_nak_unicast_retry_delay_ratio(context_);
    if (nak_ratio != 0 && nak_delay > static_cast<uint64_t>(INT64_MAX) / nak_ratio) {
        throw aeron::util::IllegalArgumentException(
            "nak_unicast_delay_ns * nak_unicast_retry_delay_ratio exceeds INT64_MAX", SOURCEINFO, EINVAL);
    }
    // As aeronmd does: the cpuset and per-thread CPU affinity settings are only
    // applied by these two steps (the affinity by each agent thread on start;
    // in invoker mode the conductor's runs on the thread calling start).
    if (aeron_driver_apply_cpuset_affinity(context_) < 0) {
        throwDriverError("Failed to apply the cpuset affinity");
    }
    if (aeron_driver_context_get_agent_on_start_function(context_) == nullptr &&
        aeron_driver_context_set_agent_on_start_function(context_, aeron_set_thread_affinity_on_start, context_) < 0) {
        throwDriverError("Failed to set the thread affinity start function");
    }
    if (aeron_driver_init(&driver_, context_) < 0) {
        throwDriverError("Failed to init driver");
    }
    if (aeron_driver_start(driver_, manual_main_loop) < 0) {
        throwDriverError("Failed to start driver");
    }
    manual_ = manual_main_loop;
}

int32_t MediaDriverWrapper::doWork() const {
    if (driver_ == nullptr || !manual_) {
        throw aeron::util::IllegalStateException(
            "the media driver is not run manually (ThreadingMode::Invoker)", SOURCEINFO, EPERM);
    }
    int work = aeron_driver_main_do_work(driver_);
    if (work < 0) {
        throwDriverError("Media driver duty cycle failed");
    }
    return work;
}

void MediaDriverWrapper::idle(int32_t work_count) const {
    if (driver_ == nullptr || !manual_) {
        throw aeron::util::IllegalStateException(
            "the media driver is not run manually (ThreadingMode::Invoker)", SOURCEINFO, EPERM);
    }
    aeron_driver_main_idle_strategy(driver_, work_count);
}

namespace {
bool terminationValidator(void *state, uint8_t *buffer, int32_t length) noexcept {
    auto *v = static_cast<MediaDriverWrapper::TerminationValidator *>(state);
    // The length comes from the request in shared memory: clients send at most
    // AERON_MAX_PATH bytes, so reject anything else.
    if (length < 0 || length > AERON_MAX_PATH) {
        return false;
    }
    return v->validator(v->owner.ctx(), rust::Slice<const uint8_t>(buffer, static_cast<size_t>(length)));
}
void terminationHook(void *state) noexcept {
    auto *h = static_cast<MediaDriverWrapper::TerminationHook *>(state);
    h->hook(h->owner.ctx());
}
} // namespace

void MediaDriverWrapper::setTerminationValidator(TerminationValidatorFn validator, ReleaseFn release, size_t ctx) {
    auto owned = std::unique_ptr<TerminationValidator>(new TerminationValidator{validator, RustOwned(release, ctx)});
    ensureNotStarted();
    if (aeron_driver_context_set_driver_termination_validator(context_, terminationValidator, owned.get()) < 0) {
        throwDriverError("Failed to set the termination validator");
    }
    validator_ = std::move(owned);
}

void MediaDriverWrapper::setTerminationHook(CloseClientFn hook, ReleaseFn release, size_t ctx) {
    auto owned = std::unique_ptr<TerminationHook>(new TerminationHook{hook, RustOwned(release, ctx)});
    ensureNotStarted();
    if (aeron_driver_context_set_driver_termination_hook(context_, terminationHook, owned.get()) < 0) {
        throwDriverError("Failed to set the termination hook");
    }
    hook_ = std::move(owned);
}

void MediaDriverWrapper::setDir(rust::Str dir) {
    ensureNotStarted();
    if (aeron_driver_context_set_dir(context_, keep(dir)) < 0) {
        throwDriverError("Failed to set dir");
    }
}

void MediaDriverWrapper::setThreadingMode(int32_t mode) {
    ensureNotStarted();
    if (aeron_driver_context_set_threading_mode(context_, static_cast<aeron_threading_mode_t>(mode)) < 0) {
        throwDriverError("Failed to set threading_mode");
    }
}

std::unique_ptr<MediaDriverWrapper> create_media_driver() {
    return std::unique_ptr<MediaDriverWrapper>(new MediaDriverWrapper());
}

} // namespace aeron_rs
