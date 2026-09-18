#include "status.hpp"

namespace atlas::capabilities {

extern "C" ExecutionResult atlas_executable_dispatch(
    const NativeRequest& request,
    const ExecutionOutputCallback& on_output) {
  return tools::git::dispatch(request, on_output);
}

}  // namespace atlas::capabilities
