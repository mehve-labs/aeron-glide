// C, not C++: the client conductor's header includes Aeron's C11 atomics
// header, which does not compile as C++ with GCC on ARM.
#include "aeron_client.h"
#include "aeron_client_conductor.h"

#if defined(_MSC_VER)
#include <intrin.h>
static bool time_out(volatile aeron_client_registration_status_t *status)
{
    return AERON_CLIENT_REGISTRATION_STATUS_AWAITING == (aeron_client_registration_status_t)_InterlockedCompareExchange(
        (volatile long *)status, AERON_CLIENT_REGISTRATION_STATUS_TIMED_OUT, AERON_CLIENT_REGISTRATION_STATUS_AWAITING);
}
#else
static bool time_out(volatile aeron_client_registration_status_t *status)
{
    aeron_client_registration_status_t expected = AERON_CLIENT_REGISTRATION_STATUS_AWAITING;
    return __atomic_compare_exchange_n(
        status, &expected, AERON_CLIENT_REGISTRATION_STATUS_TIMED_OUT, false, __ATOMIC_ACQ_REL, __ATOMIC_ACQUIRE);
}
#endif

// Once a client's conductor has closed on a fatal error (driver timeout,
// conductor service timeout, ...), it no longer times out pending registrations
// (still so in 1.54.0), so polling one
// (e.g. the next session id the archive client waits for while connecting,
// without a deadline of its own) never ends. Time them out as the conductor
// would have: the poll then fails with "no response from media driver".
//
// The closed conductor no longer touches its registering resources, and
// they are only freed when the client is closed, which the caller prevents.
int aeron_glide_time_out_registrations(aeron_t *client)
{
    if (NULL == client)
    {
        return 0;
    }
    aeron_client_conductor_t *conductor = &client->conductor;
    if (!aeron_client_conductor_is_closed(conductor))
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
