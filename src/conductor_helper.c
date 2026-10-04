// C, not C++: the client conductor's header includes Aeron's C11 atomics
// header, which does not compile as C++ with GCC on ARM.
#include "aeron_client.h"
#include "aeron_client_conductor.h"

#if defined(_MSC_VER)
#include <intrin.h>
static bool is_terminating(aeron_client_conductor_t *conductor)
{
    bool value = *(volatile bool *)&conductor->is_terminating;
    _ReadWriteBarrier();
    return value;
}
static bool time_out(volatile aeron_client_registration_status_t *status)
{
    return AERON_CLIENT_REGISTRATION_STATUS_AWAITING == (aeron_client_registration_status_t)_InterlockedCompareExchange(
        (volatile long *)status, AERON_CLIENT_REGISTRATION_STATUS_TIMED_OUT, AERON_CLIENT_REGISTRATION_STATUS_AWAITING);
}
#else
static bool is_terminating(aeron_client_conductor_t *conductor)
{
    return __atomic_load_n(&conductor->is_terminating, __ATOMIC_ACQUIRE);
}
static bool time_out(volatile aeron_client_registration_status_t *status)
{
    aeron_client_registration_status_t expected = AERON_CLIENT_REGISTRATION_STATUS_AWAITING;
    return __atomic_compare_exchange_n(
        status, &expected, AERON_CLIENT_REGISTRATION_STATUS_TIMED_OUT, false, __ATOMIC_ACQ_REL, __ATOMIC_ACQUIRE);
}
#endif

// Once a client's conductor is terminating (driver timeout, conductor service
// timeout, ...), it no longer times out pending registrations, so polling one
// (e.g. the next session id the archive client waits for while connecting,
// without a deadline of its own) never ends. Time them out as the conductor
// would have: the poll then fails with "no response from media driver".
//
// The terminating conductor no longer touches its registering resources, and
// they are only freed when the client is closed, which the caller prevents.
int aeron_glide_time_out_registrations(aeron_t *client)
{
    if (NULL == client)
    {
        return 0;
    }
    aeron_client_conductor_t *conductor = &client->conductor;
    if (!is_terminating(conductor))
    {
        return 0;
    }
    int timed_out = 0;
    for (size_t i = 0; i < conductor->registering_resources.length; i++)
    {
        aeron_client_registering_resource_t *resource = conductor->registering_resources.array[i].resource;
        if (time_out(&resource->registration_status))
        {
            timed_out++;
        }
    }
    return timed_out;
}

#include "aeron_archive_client.h"
#include "aeron_archive_context.h"
#include "aeron_archive_recording_signal.h"
#include <inttypes.h>
#include "util/aeron_error.h"

// aeron_archive_poll_for_recording_signals (aeron_archive_client.c) with its
// mutex released on every path: Aeron's returns -1 with the archive's lock
// still held when the control response poller fails (e.g. a message with an
// unknown schema or signal from a newer archive), and every later request from
// another thread then blocks forever. Otherwise the same.
int aeron_glide_archive_poll_for_recording_signals(int32_t *count_p, aeron_archive_t *aeron_archive)
{
    aeron_mutex_lock(&aeron_archive->lock);

    int32_t count = 0;
    int rc = 0;

    aeron_archive_control_response_poller_t *poller = aeron_archive->control_response_poller;

    if (aeron_archive_control_response_poller_poll(poller) < 0)
    {
        aeron_mutex_unlock(&aeron_archive->lock);
        AERON_APPEND_ERR("%s", "");
        return -1;
    }

    if (poller->is_poll_complete && poller->control_session_id == aeron_archive->control_session_id)
    {
        if (poller->is_control_response && poller->is_code_error)
        {
            if (NULL == aeron_archive->ctx->error_handler)
            {
                AERON_SET_ERR(
                    -aeron_archive_client_map_archive_to_client_error_code((int)poller->relevant_id),
                    "response for correlationId=%" PRIi64 ", errorCode=%" PRIi64 ", error: %s",
                    poller->correlation_id,
                    poller->relevant_id,
                    poller->error_message);
                rc = -1;
            }
            else
            {
                aeron_archive_context_invoke_error_handler(
                    aeron_archive->ctx, poller->correlation_id, (int32_t)poller->relevant_id, poller->error_message);
            }
        }
        else if (poller->is_recording_signal)
        {
            aeron_archive_recording_signal_t signal;
            signal.control_session_id = poller->control_session_id;
            signal.recording_id = poller->recording_id;
            signal.subscription_id = poller->subscription_id;
            signal.position = poller->position;
            signal.recording_signal_code = poller->recording_signal_code;
            aeron_archive_recording_signal_dispatch_signal(aeron_archive->ctx, &signal);
            count++;
        }
    }

    aeron_mutex_unlock(&aeron_archive->lock);

    if (NULL != count_p)
    {
        *count_p = count;
    }

    return rc;
}
