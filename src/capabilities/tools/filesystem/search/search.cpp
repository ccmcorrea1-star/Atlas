#include "../filesystem.hpp"

#include "../../../runtime/executable/adapter.hpp"

namespace atlas::capabilities::runtime::executable {

Dispatch dispatch() {
  return [](const NativeRequest& request, const ExecutionOutputCallback&) {
    return tools::filesystem::searchDispatch(request);
  };
}

}  // namespace atlas::capabilities::runtime::executable
