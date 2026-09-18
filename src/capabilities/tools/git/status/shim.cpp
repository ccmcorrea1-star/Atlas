#include "status.hpp"

#include "../../../runtime/executable/adapter.hpp"

namespace atlas::capabilities::runtime::executable {

Dispatch dispatch() {
  return &tools::git::dispatch;
}

}  // namespace atlas::capabilities::runtime::executable
