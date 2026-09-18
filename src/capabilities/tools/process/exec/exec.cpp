#include "exec.hpp"

#include "../../../core/arguments.hpp"
#include "../../../core/command_runner.hpp"
#include "../../../runtime/executable/adapter.hpp"

#include <utility>

namespace atlas::capabilities::tools::process {

ExecResult exec(
    const ExecRequest& request,
    const atlas::capabilities::ExecutionOutputCallback& on_output) {
  const auto started = std::chrono::steady_clock::now();
  ExecResult result;
  result.target = request.target;

  auto requestError = [&](std::string message) {
    result.status = ExecStatus::failed;
    result.error = std::move(message);
    result.duration = std::chrono::duration_cast<std::chrono::milliseconds>(
        std::chrono::steady_clock::now() - started);
    return result;
  };

  if (request.target != kLocalTarget) {
    return requestError("only the local target is supported");
  }

  // Os argumentos seguem para execve sem interpretacao ou expansao.
  atlas::capabilities::CommandRequest command_request;
  command_request.program = request.program;
  command_request.args = request.args;
  command_request.cwd = request.cwd;
  command_request.timeout = request.timeout;

  const atlas::capabilities::CommandResult spawned =
      atlas::capabilities::runCommand(command_request, on_output);

  result.stdout = spawned.stdout;
  result.stderr = spawned.stderr;
  result.exit_code = spawned.exit_code;
  result.duration = spawned.duration;
  result.status = spawned.status == atlas::capabilities::CommandStatus::success
      ? ExecStatus::success
      : spawned.status == atlas::capabilities::CommandStatus::timed_out
      ? ExecStatus::timed_out
      : ExecStatus::failed;
  result.error = spawned.error;
  return result;
}

const char* statusName(ExecStatus status) noexcept {
  switch (status) {
    case ExecStatus::success:
      return "success";
    case ExecStatus::failed:
      return "failed";
    case ExecStatus::timed_out:
      return "timed_out";
  }
  return "failed";
}

namespace {

atlas::capabilities::ExecutionResult resultFromExec(const ExecResult& processResult) {
  atlas::capabilities::ExecutionResult result;
  result.target = processResult.target;
  result.status = processResult.status == ExecStatus::success
      ? atlas::capabilities::ExecutionStatus::success
      : processResult.status == ExecStatus::timed_out
      ? atlas::capabilities::ExecutionStatus::timed_out
      : atlas::capabilities::ExecutionStatus::failed;
  result.error = processResult.error;
  result.output = StructuredValue::Object{
      {"stdout", processResult.stdout},
      {"stderr", processResult.stderr},
      {"exit_code", processResult.exit_code},
      {"duration_ms", static_cast<std::int64_t>(processResult.duration.count())},
  };
  return result;
}

atlas::capabilities::ExecutionResult requestFailure(
    std::string target,
    std::string error) {
  ExecResult processResult;
  processResult.target = std::move(target);
  processResult.error = std::move(error);
  return resultFromExec(processResult);
}

}  // namespace

atlas::capabilities::ExecutionResult dispatch(
    const atlas::capabilities::NativeRequest& request,
    const atlas::capabilities::ExecutionOutputCallback& on_output) {
  try {
    const atlas::capabilities::ArgumentReader reader(request.arguments);
    ExecRequest processRequest;
    processRequest.target = request.target;
    processRequest.program = reader.string("program");
    processRequest.args = reader.optionalStringArray("args").value_or(std::vector<std::string>{});
    processRequest.cwd = reader.optionalString("cwd");
    if (const auto timeout = reader.optionalInteger("timeout_ms"); timeout.has_value()) {
      processRequest.timeout = std::chrono::milliseconds(*timeout);
    } else if (const auto legacyTimeout = reader.optionalInteger("timeout"); legacyTimeout.has_value()) {
      processRequest.timeout = std::chrono::milliseconds(*legacyTimeout);
    }
    return resultFromExec(exec(processRequest, on_output));
  } catch (const atlas::capabilities::ArgumentError& error) {
    return requestFailure(request.target, error.what());
  }
}

}  // namespace atlas::capabilities::tools::process

namespace atlas::capabilities::runtime::executable {

Dispatch dispatch() {
  return &tools::process::dispatch;
}

}  // namespace atlas::capabilities::runtime::executable
