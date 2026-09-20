#include "shell.hpp"

#include "../../../core/arguments.hpp"
#include "../../../core/command_runner.hpp"
#include "../../../runtime/executable/adapter.hpp"

#include <utility>

namespace atlas::capabilities::tools::shell {

ShellResult exec(
    const ShellRequest& request,
    const atlas::capabilities::ExecutionOutputCallback& on_output) {
  const auto started = std::chrono::steady_clock::now();
  ShellResult result;
  result.target = request.target;

  auto requestError = [&](std::string message) {
    result.status = ShellStatus::failed;
    result.error = std::move(message);
    result.duration = std::chrono::duration_cast<std::chrono::milliseconds>(
        std::chrono::steady_clock::now() - started);
    return result;
  };

  if (request.target != kLocalTarget) {
    return requestError("only the local target is supported");
  }
  if (request.command.empty()) {
    return requestError("command cannot be empty");
  }
  if (request.command.find('\0') != std::string::npos) {
    return requestError("command cannot contain NUL bytes");
  }

  // O shell interpreta o comando; pipes, redirecionamentos e expansoes valem aqui.
  atlas::capabilities::CommandRequest command_request;
  command_request.program = kShellProgram;
  command_request.args = {"-c", request.command};
  command_request.cwd = request.cwd;
  command_request.timeout = request.timeout;

  const atlas::capabilities::CommandResult spawned =
      atlas::capabilities::runCommand(command_request, on_output);

  result.stdout = spawned.stdout;
  result.stderr = spawned.stderr;
  result.stdout_truncated = spawned.stdout_truncated;
  result.stderr_truncated = spawned.stderr_truncated;
  result.exit_code = spawned.exit_code;
  result.duration = spawned.duration;
  result.status = spawned.status == atlas::capabilities::CommandStatus::success
      ? ShellStatus::success
      : spawned.status == atlas::capabilities::CommandStatus::timed_out
      ? ShellStatus::timed_out
      : ShellStatus::failed;
  result.error = spawned.error;
  return result;
}

const char* statusName(ShellStatus status) noexcept {
  switch (status) {
    case ShellStatus::success:
      return "success";
    case ShellStatus::failed:
      return "failed";
    case ShellStatus::timed_out:
      return "timed_out";
  }
  return "failed";
}

namespace {

atlas::capabilities::ExecutionResult resultFromShell(const ShellResult& shellResult) {
  atlas::capabilities::ExecutionResult result;
  result.target = shellResult.target;
  result.status = shellResult.status == ShellStatus::success
      ? atlas::capabilities::ExecutionStatus::success
      : shellResult.status == ShellStatus::timed_out
      ? atlas::capabilities::ExecutionStatus::timed_out
      : atlas::capabilities::ExecutionStatus::failed;
  result.error = shellResult.error;
  result.output = StructuredValue::Object{
      {"stdout", shellResult.stdout},
      {"stderr", shellResult.stderr},
      {"stdout_truncated", shellResult.stdout_truncated},
      {"stderr_truncated", shellResult.stderr_truncated},
      {"exit_code", shellResult.exit_code},
      {"duration_ms", static_cast<std::int64_t>(shellResult.duration.count())},
  };
  return result;
}

atlas::capabilities::ExecutionResult requestFailure(
    std::string target,
    std::string error) {
  ShellResult shellResult;
  shellResult.target = std::move(target);
  shellResult.error = std::move(error);
  return resultFromShell(shellResult);
}

}  // namespace

atlas::capabilities::ExecutionResult dispatch(
    const atlas::capabilities::NativeRequest& request,
    const atlas::capabilities::ExecutionOutputCallback& on_output) {
  try {
    const atlas::capabilities::ArgumentReader reader(request.arguments);
    ShellRequest shellRequest;
    shellRequest.target = request.target;
    shellRequest.command = reader.string("command");
    shellRequest.cwd = reader.optionalString("cwd");
    if (const auto timeout = reader.optionalInteger("timeout_ms"); timeout.has_value()) {
      shellRequest.timeout = std::chrono::milliseconds(*timeout);
    } else if (const auto legacyTimeout = reader.optionalInteger("timeout"); legacyTimeout.has_value()) {
      shellRequest.timeout = std::chrono::milliseconds(*legacyTimeout);
    }
    return resultFromShell(exec(shellRequest, on_output));
  } catch (const atlas::capabilities::ArgumentError& error) {
    return requestFailure(request.target, error.what());
  }
}

}  // namespace atlas::capabilities::tools::shell

namespace atlas::capabilities::runtime::executable {

Dispatch dispatch() {
  return &tools::shell::dispatch;
}

}  // namespace atlas::capabilities::runtime::executable
