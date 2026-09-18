#pragma once

#include "../../../core/execution.hpp"

#include "../git.hpp"

#include <cstdint>
#include <string>

namespace atlas::capabilities::tools::git {

struct DiffRequest {
  std::string target;
  std::string path;
  bool staged{false};
};

struct DiffResult {
  std::string target;
  std::string diff;
  bool truncated{false};
  GitStatus status{GitStatus::failed};
  std::string error;
};

// Roda git diff sem tocar no repositorio; indisponivel sem o executavel.
DiffResult diff(const DiffRequest& request);

atlas::capabilities::ExecutionResult dispatchDiff(
    const atlas::capabilities::NativeRequest& request,
    const atlas::capabilities::ExecutionOutputCallback& on_output = {});

}  // namespace atlas::capabilities::tools::git
