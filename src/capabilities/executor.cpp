#include "executor.hpp"

#include <exception>
#include <utility>

namespace atlas::capabilities {

const char* executionStatusName(ExecutionStatus status) noexcept {
  switch (status) {
    case ExecutionStatus::success:
      return "success";
    case ExecutionStatus::failed:
      return "failed";
    case ExecutionStatus::timed_out:
      return "timed_out";
    case ExecutionStatus::unavailable:
      return "unavailable";
  }
  return "failed";
}

ExecutionResult Executor::failure(std::string target, std::string error) {
  ExecutionResult result;
  result.target = std::move(target);
  result.status = ExecutionStatus::failed;
  result.error = std::move(error);
  return result;
}

ExecutionResult Executor::unavailable(std::string target, std::string kind) {
  ExecutionResult result;
  result.target = std::move(target);
  result.status = ExecutionStatus::unavailable;
  result.error = "executor adapter for implementation kind '" + kind + "' is not available";
  return result;
}

ExecutionResult Executor::execute(const ExecutionRequest& request) const {
  if (request.capability_id.empty()) {
    return failure(request.target, "capability id cannot be empty");
  }

  const auto capability = registry_.get(request.capability_id);
  if (!capability.has_value()) {
    return failure(
        request.target,
        "capability '" + request.capability_id + "' is not registered");
  }

  const CapabilityImplementation& implementation = capability->implementation;
  if (implementation.empty()) {
    return failure(
        request.target,
        "capability '" + request.capability_id + "' has no valid implementation");
  }
  if (implementation.kind != "native") {
    return unavailable(request.target, implementation.kind);
  }

  const auto entrypoint = registry_.resolveNativeEntrypoint(implementation.entrypoint);
  if (!entrypoint.has_value()) {
    return failure(
        request.target,
        "native entrypoint '" + implementation.entrypoint + "' is not registered");
  }

  try {
    const NativeRequest native_request{request.target, request.arguments};
    ExecutionResult result = entrypoint.value()(native_request);
    result.target = request.target;
    return result;
  } catch (const std::exception& exception) {
    return failure(
        request.target,
        "native entrypoint '" + implementation.entrypoint + "' threw an exception: " +
            exception.what());
  } catch (...) {
    return failure(
        request.target,
        "native entrypoint '" + implementation.entrypoint + "' threw an unknown exception");
  }
}

ExecutionResult Executor::execute(
    std::string_view capability_id,
    std::string target,
    StructuredArguments arguments) const {
  return execute({std::string(capability_id), std::move(target), std::move(arguments)});
}

}  // namespace atlas::capabilities
