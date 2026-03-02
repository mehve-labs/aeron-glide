#pragma once

// We need to include Aeron.h, but we must hide the methods in Subscription
// that return std::shared_ptr<std::vector<Image>> because autocxx
// does not support std::shared_ptr containing std::vector.

#define AERON_HIDE_UNSUPPORTED_METHODS 1

#include "Aeron.h"
