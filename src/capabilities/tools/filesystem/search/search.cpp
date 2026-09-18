#include "../filesystem.hpp"

namespace atlas::capabilities {

extern "C" ExecutionResult atlas_executable_dispatch(
    const NativeRequest& request,
    const ExecutionOutputCallback& /* unused */) {
  return tools::filesystem::searchDispatch(request);
}

}  // namespace atlas::capabilities
