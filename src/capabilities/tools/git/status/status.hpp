#pragma once

#include "../../../core/execution.hpp"

#include "../git.hpp"

#include <cstdint>
#include <optional>
#include <string>
#include <vector>

namespace atlas::capabilities::tools::git {

struct StatusFile {
  std::string path;
  std::string status;
};

struct StatusRequest {
  std::string target;
  std::string path;
};

struct StatusResult {
  std::string target;
  std::string branch;
  std::optional<std::string> upstream;
  bool clean{true};
  std::vector<StatusFile> staged;
  std::vector<StatusFile> unstaged;
  std::vector<std::string> untracked;
  std::optional<std::int64_t> ahead;
  std::optional<std::int64_t> behind;
  GitStatus status{GitStatus::failed};
  std::string error;
};

// Roda git status sem tocar no repositorio; indisponivel sem o executavel.
StatusResult status(const StatusRequest& request);

atlas::capabilities::ExecutionResult dispatch(
    const atlas::capabilities::NativeRequest& request,
    const atlas::capabilities::ExecutionOutputCallback& on_output = {});

}  // namespace atlas::capabilities::tools::git
