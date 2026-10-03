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

void MediaDriverWrapper::start(bool manual_main_loop) {
    if (driver_ != nullptr) {
        throw aeron::util::IllegalStateException("media driver already started", SOURCEINFO, EPERM);
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
    return v->validator(v->owner.ctx(), rust::Slice<const uint8_t>(buffer, length < 0 ? 0 : length));
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
