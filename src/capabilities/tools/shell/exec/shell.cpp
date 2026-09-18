#include "shell.hpp"

#include "../../../core/spawn.hpp"

#include <string_view>
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
  atlas::capabilities::SpawnRequest spawn_request;
  spawn_request.program = kShellProgram;
  spawn_request.args = {"-c", request.command};
  spawn_request.cwd = request.cwd;
  spawn_request.timeout = request.timeout;

  const atlas::capabilities::SpawnResult spawned =
      atlas::capabilities::spawn(spawn_request, on_output);

  result.stdout = spawned.stdout;
  result.stderr = spawned.stderr;
  result.exit_code = spawned.exit_code;
  result.duration = spawned.duration;
  result.status = spawned.status == atlas::capabilities::SpawnStatus::success
      ? ShellStatus::success
      : spawned.status == atlas::capabilities::SpawnStatus::timed_out
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

const StructuredValue* argument(
    const atlas::capabilities::NativeRequest& request,
    std::string_view name) {
  const auto iterator = request.arguments.find(name);
  return iterator == request.arguments.end() ? nullptr : &iterator->second;
}

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
  // O runtime ja validou o JSON; esta camada valida somente o contrato do shell.
  const StructuredValue* commandValue = argument(request, "command");
  const auto* command = commandValue == nullptr
      ? nullptr
      : std::get_if<std::string>(&commandValue->value);
  if (command == nullptr || command->empty()) {
    return requestFailure(request.target, "field 'command' must be a non-empty string");
  }

  ShellRequest shellRequest;
  shellRequest.target = request.target;
  shellRequest.command = *command;

  if (const StructuredValue* cwdValue = argument(request, "cwd"); cwdValue != nullptr) {
    const auto* cwd = std::get_if<std::string>(&cwdValue->value);
    if (cwd == nullptr) {
      return requestFailure(request.target, "field 'cwd' must be a string");
    }
    shellRequest.cwd = *cwd;
  }

  const StructuredValue* timeoutValue = argument(request, "timeout_ms");
  if (timeoutValue == nullptr) {
    timeoutValue = argument(request, "timeout");
  }
  if (timeoutValue != nullptr) {
    const auto* timeout = std::get_if<std::int64_t>(&timeoutValue->value);
    if (timeout == nullptr || *timeout < 0) {
      return requestFailure(request.target, "field 'timeout_ms' must be a non-negative integer");
    }
    shellRequest.timeout = std::chrono::milliseconds(*timeout);
  }

  return resultFromShell(exec(shellRequest, on_output));
}

}  // namespace atlas::capabilities::tools::shell

namespace atlas::capabilities {

extern "C" ExecutionResult atlas_executable_dispatch(
    const NativeRequest& request,
    const ExecutionOutputCallback& on_output) {
  return tools::shell::dispatch(request, on_output);
}

}  // namespace atlas::capabilities
