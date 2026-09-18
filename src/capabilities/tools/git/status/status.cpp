#include "status.hpp"

#include "../../../core/spawn.hpp"

#include <string_view>
#include <utility>

namespace atlas::capabilities::tools::git {
namespace {

const StructuredValue* argument(
    const atlas::capabilities::NativeRequest& request,
    std::string_view name) {
  const auto iterator = request.arguments.find(name);
  return iterator == request.arguments.end() ? nullptr : &iterator->second;
}

bool gitMissing(const atlas::capabilities::SpawnResult& spawned) {
  return spawned.exit_code == 127;
}

atlas::capabilities::ExecutionResult resultFromStatus(const StatusResult& statusResult) {
  atlas::capabilities::ExecutionResult result;
  result.target = statusResult.target;
  result.status = statusResult.status == GitStatus::success
      ? atlas::capabilities::ExecutionStatus::success
      : statusResult.status == GitStatus::timed_out
      ? atlas::capabilities::ExecutionStatus::timed_out
      : statusResult.status == GitStatus::unavailable
      ? atlas::capabilities::ExecutionStatus::unavailable
      : atlas::capabilities::ExecutionStatus::failed;
  result.error = statusResult.error;
  StructuredValue::Object output{
      {"branch", statusResult.branch},
      {"clean", statusResult.clean},
  };
  if (statusResult.upstream.has_value()) {
    output.emplace("upstream", *statusResult.upstream);
  }
  StructuredValue::Array staged;
  for (const StatusFile& file : statusResult.staged) {
    staged.push_back(StructuredValue::Object{{"path", file.path}, {"status", file.status}});
  }
  output.emplace("staged", std::move(staged));
  StructuredValue::Array unstaged;
  for (const StatusFile& file : statusResult.unstaged) {
    unstaged.push_back(StructuredValue::Object{{"path", file.path}, {"status", file.status}});
  }
  output.emplace("unstaged", std::move(unstaged));
  StructuredValue::Array untracked;
  for (const std::string& path : statusResult.untracked) {
    untracked.push_back(path);
  }
  output.emplace("untracked", std::move(untracked));
  if (statusResult.ahead.has_value()) {
    output.emplace("ahead", *statusResult.ahead);
  }
  if (statusResult.behind.has_value()) {
    output.emplace("behind", *statusResult.behind);
  }
  result.output = std::move(output);
  return result;
}

atlas::capabilities::ExecutionResult requestFailure(std::string target, std::string error) {
  StatusResult statusResult;
  statusResult.target = std::move(target);
  statusResult.error = std::move(error);
  return resultFromStatus(statusResult);
}

// "## main...origin/main [ahead 1]": branch, upstream e contadores.
void parseHeader(std::string_view line, StatusResult& result) {
  if (line.starts_with("## ")) {
    line.remove_prefix(3);
  }
  if (line.starts_with("No commits yet on ")) {
    result.branch = std::string(line.substr(17));
    return;
  }
  const std::size_t dots = line.find("...");
  if (dots == std::string_view::npos) {
    result.branch = std::string(line);
    return;
  }
  result.branch = std::string(line.substr(0, dots));
  std::string_view rest = line.substr(dots + 3);
  const std::size_t space = rest.find(' ');
  result.upstream = std::string(rest.substr(0, space));
  if (space == std::string_view::npos) {
    return;
  }
  rest = rest.substr(space + 1);
  if (rest.size() < 2 || rest.front() != '[' || rest.back() != ']') {
    return;
  }
  rest = rest.substr(1, rest.size() - 2);
  const std::string_view aheadKey = "ahead ";
  const std::string_view behindKey = "behind ";
  std::size_t pos = 0;
  while (pos < rest.size()) {
    const std::size_t comma = rest.find(", ", pos);
    const std::string_view part = rest.substr(pos, comma == std::string_view::npos ? comma : comma - pos);
    const auto count = [](std::string_view digits) -> std::optional<std::int64_t> {
      if (digits.empty()) {
        return std::nullopt;
      }
      std::int64_t value = 0;
      for (const char digit : digits) {
        if (digit < '0' || digit > '9') {
          return std::nullopt;
        }
        value = value * 10 + (digit - '0');
      }
      return value;
    };
    if (part.starts_with(aheadKey)) {
      result.ahead = count(part.substr(aheadKey.size()));
    } else if (part.starts_with(behindKey)) {
      result.behind = count(part.substr(behindKey.size()));
    }
    if (comma == std::string_view::npos) {
      break;
    }
    pos = comma + 2;
  }
}

}  // namespace

StatusResult status(const StatusRequest& request) {
  StatusResult result;
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
  atlas::capabilities::SpawnRequest spawnRequest;
  spawnRequest.program = "git";
  spawnRequest.args = {"status", "--porcelain=v1", "-b"};
  spawnRequest.cwd = request.path;
  const atlas::capabilities::SpawnResult spawned = atlas::capabilities::spawn(spawnRequest);
  if (gitMissing(spawned)) {
    result.status = GitStatus::unavailable;
    result.error = "git executable not found";
    return result;
  }
  if (spawned.status != atlas::capabilities::SpawnStatus::success) {
    return failure(spawned.stderr.empty() ? spawned.error : spawned.stderr);
  }
  result.status = GitStatus::success;
  std::string_view output(spawned.stdout);
  bool first = true;
  while (!output.empty()) {
    const std::size_t newline = output.find('\n');
    const std::string_view line = output.substr(0, newline);
    if (newline == std::string_view::npos) {
      output.remove_prefix(output.size());
    } else {
      output.remove_prefix(newline + 1);
    }
    if (first) {
      first = false;
      parseHeader(line, result);
      continue;
    }
    if (line.size() < 4) {
      continue;
    }
    const char staged = line[0];
    const char unstaged = line[1];
    std::string path{line.substr(3)};
    if (path.size() >= 2 && path.front() == '"' && path.back() == '"') {
      path = path.substr(1, path.size() - 2);
    }
    const std::size_t arrow = path.find(" -> ");
    if (arrow != std::string::npos) {
      path = path.substr(arrow + 4);
    }
    if (staged == '?' && unstaged == '?') {
      result.untracked.push_back(path);
      result.clean = false;
      continue;
    }
    if (staged != ' ' && staged != '?') {
      result.staged.push_back({path, std::string(1, staged)});
      result.clean = false;
    }
    if (unstaged != ' ' && unstaged != '?') {
      result.unstaged.push_back({path, std::string(1, unstaged)});
      result.clean = false;
    }
  }
  return result;
}

atlas::capabilities::ExecutionResult dispatch(
    const atlas::capabilities::NativeRequest& request,
    const atlas::capabilities::ExecutionOutputCallback& on_output) {
  static_cast<void>(on_output);
  const StructuredValue* pathValue = argument(request, "path");
  const auto* path = pathValue == nullptr ? nullptr : std::get_if<std::string>(&pathValue->value);
  if (path == nullptr || path->empty()) {
    return requestFailure(request.target, "field 'path' must be a non-empty string");
  }
  StatusRequest statusRequest;
  statusRequest.target = request.target;
  statusRequest.path = *path;
  return resultFromStatus(status(statusRequest));
}

}  // namespace atlas::capabilities::tools::git
