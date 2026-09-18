#include "diff.hpp"

#include "../../../runtime/executable/adapter.hpp"

namespace atlas::capabilities::runtime::executable {

Dispatch dispatch() {
  return &tools::git::dispatchDiff;
}

}  // namespace atlas::capabilities::runtime::executable
