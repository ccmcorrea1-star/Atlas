#include "diff.hpp"

namespace atlas::capabilities {

extern "C" ExecutionResult atlas_executable_dispatch(
    const NativeRequest& request,
    const ExecutionOutputCallback& on_output) {
  return tools::git::dispatchDiff(request, on_output);
}

}  // namespace atlas::capabilities
