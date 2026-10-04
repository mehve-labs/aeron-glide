// Embedded media driver shim (the `driver` feature): the C media driver
// (aeronmd.h), bridged in src/driver.rs and src/driver_gen.rs.
#pragma once
#include <cstdlib>
#include <deque>
#include <map>
#include "shim.h"

// Forward declarations for C driver types (defined in aeronmd.h)
extern "C" {
    struct aeron_driver_context_stct;
    typedef struct aeron_driver_context_stct aeron_driver_context_t;
    struct aeron_driver_stct;
    typedef struct aeron_driver_stct aeron_driver_t;
}

namespace aeron_rs {

// Throws the Aeron exception matching aeron_errcode(), prefixed with `what`.
[[noreturn]] void throwDriverError(const char *what);

// Settings are applied by the generated driver_gen.h functions.
// (ctx, token) -> whether to terminate.
using TerminationValidatorFn = rust::Fn<bool(size_t, rust::Slice<const uint8_t>)>;

class MediaDriverWrapper {
public:
    MediaDriverWrapper();
    ~MediaDriverWrapper();

    // `manual_main_loop`: the caller runs the conductor with doWork (required in
    // the INVOKER threading mode).
    void start(bool manual_main_loop);
    // One duty cycle of a manually run driver (aeron_driver_main_do_work).
    // const: the Rust side serialises calls; the driver is reached through a pointer.
    int32_t doWork() const;
    // Its idle strategy (aeron_driver_main_idle_strategy).
    void idle(int32_t work_count) const;
    // Termination requests (D3), called on the driver conductor thread. Each
    // takes ownership of its ctx, released after the driver is closed.
    void setTerminationValidator(TerminationValidatorFn validator, ReleaseFn release, size_t ctx);
    void setTerminationHook(CloseClientFn hook, ReleaseFn release, size_t ctx);
    // Throws IllegalStateException once started: the driver's threads read the context.
    void ensureNotStarted() const;
    aeron_driver_context_t *context() const { return context_; }
    // A copy of `value` that lives as long as this wrapper: some C setters keep
    // the pointer they are given instead of copying the string.
    const char *keep(rust::Str value) {
        strings_.push_back(detail::cString(value));
        return strings_.back().c_str();
    }
    const char *keep(const std::string &value) {
        strings_.push_back(value);
        return strings_.back().c_str();
    }

    void setThreadingMode(int32_t mode);
    void setDir(rust::Str dir);
    // Close the driver and its context now, reporting a failure (the
    // destructor, which also closes them, cannot).
    void closeDriver();

    // The idle strategy chosen for `base` (e.g. "sender_idle_strategy") through
    // the builder, to reload it with new init args: Aeron records "backoff" as
    // the name even when the strategy came from its environment variable.
    void chooseStrategy(const char *base, const std::string &strategy) { strategies_[base] = strategy; }
    // The strategy in effect for `base`: chosen here, else from `env_var`, else
    // Aeron's recorded name (empty if none).
    std::string effectiveStrategy(const char *base, const char *env_var, const char *recorded) const {
        auto chosen = strategies_.find(base);
        if (chosen != strategies_.end()) {
            return chosen->second;
        }
        if (const char *from_env = std::getenv(env_var)) {
            return from_env;
        }
        return recorded ? recorded : "";
    }

    struct TerminationValidator {
        TerminationValidatorFn validator;
        RustOwned owner;
    };
    struct TerminationHook {
        CloseClientFn hook;
        RustOwned owner;
    };
private:
    // Destroyed after the driver and context are closed (members outlive the
    // destructor's body), so the driver never calls into released handlers.
    std::unique_ptr<TerminationValidator> validator_;
    std::unique_ptr<TerminationHook> hook_;
    aeron_driver_context_t* context_;
    aeron_driver_t* driver_;
    bool manual_ = false;
    std::deque<std::string> strings_; // destroyed after the driver and context are closed
    std::map<std::string, std::string> strategies_;
};

std::unique_ptr<MediaDriverWrapper> create_media_driver();

} // namespace aeron_rs
