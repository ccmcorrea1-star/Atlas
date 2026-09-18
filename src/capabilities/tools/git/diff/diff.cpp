#include "diff.hpp"

#include "../git.hpp"
#include "../../../core/command_runner.hpp"

#include <string_view>
#include <utility>

namespace atlas::capabilities::tools::git {
namespace {

// Teto da saida para nao inundar o contexto do agente.
inline constexpr std::size_t kMaxDiffBytes = 64u * 1024u;

const StructuredValue* argument(
    const atlas::capabilities::NativeRequest& request,
    std::string_view name) {
  const auto iterator = request.arguments.find(name);
  return iterator == request.arguments.end() ? nullptr : &iterator->second;
}

atlas::capabilities::ExecutionResult resultFromDiff(const DiffResult& diffResult) {
  atlas::capabilities::ExecutionResult result;
  result.target = diffResult.target;
  result.status = diffResult.status == GitStatus::success
      ? atlas::capabilities::ExecutionStatus::success
      : diffResult.status == GitStatus::timed_out
      ? atlas::capabilities::ExecutionStatus::timed_out
      : diffResult.status == GitStatus::unavailable
      ? atlas::capabilities::ExecutionStatus::unavailable
      : atlas::capabilities::ExecutionStatus::failed;
  result.error = diffResult.error;
  result.output = StructuredValue::Object{
      {"diff", diffResult.diff},
      {"truncated", diffResult.truncated},
  };
  return result;
}

atlas::capabilities::ExecutionResult requestFailure(std::string target, std::string error) {
  DiffResult diffResult;
  diffResult.target = std::move(target);
  diffResult.error = std::move(error);
  return resultFromDiff(diffResult);
}

}  // namespace

DiffResult diff(const DiffRequest& request) {
  DiffResult result;
  result.target = request.target;
  auto failure = [&](std::string message) {
    result.status = GitStatus::failed;
    result.error = std::move(message);
    return result;
  };
  if (request.target != kLocalTarget) {
    return failure("only the local target is supported");
  }
  if (request.path.empty()) {
    return failure("field 'path' must be a non-empty string");
  }
  atlas::capabilities::CommandRequest commandRequest;
  commandRequest.program = "git";
  commandRequest.args = {"diff", "--no-color", "--no-ext-diff"};
  if (request.staged) {
    commandRequest.args.push_back("--staged");
  }
  commandRequest.cwd = request.path;
  const atlas::capabilities::CommandResult spawned = atlas::capabilities::runCommand(commandRequest);
  if (spawned.status == atlas::capabilities::CommandStatus::executable_not_found) {
    result.status = GitStatus::unavailable;
    result.error = "git executable not found";
    return result;
  }
  if (spawned.status != atlas::capabilities::CommandStatus::success) {
    return failure(spawned.stderr.empty() ? spawned.error : spawned.stderr);
  }
  result.status = GitStatus::success;
  result.diff = spawned.stdout;
  if (result.diff.size() > kMaxDiffBytes) {
    result.diff.resize(kMaxDiffBytes);
    result.truncated = true;
  }
  return result;
}

atlas::capabilities::ExecutionResult dispatchDiff(
    const atlas::capabilities::NativeRequest& request,
    const atlas::capabilities::ExecutionOutputCallback& on_output) {
  static_cast<void>(on_output);
  const StructuredValue* pathValue = argument(request, "path");
  const auto* path = pathValue == nullptr ? nullptr : std::get_if<std::string>(&pathValue->value);
  if (path == nullptr || path->empty()) {
    return requestFailure(request.target, "field 'path' must be a non-empty string");
  }
  DiffRequest diffRequest;
  diffRequest.target = request.target;
  diffRequest.path = *path;
  if (const StructuredValue* stagedValue = argument(request, "staged"); stagedValue != nullptr) {
    const auto* staged = std::get_if<bool>(&stagedValue->value);
    if (staged == nullptr) {
      return requestFailure(request.target, "field 'staged' must be a boolean");
    }
    diffRequest.staged = *staged;
  }
  return resultFromDiff(diff(diffRequest));
}

}  // namespace atlas::capabilities::tools::git
