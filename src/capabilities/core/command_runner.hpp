#pragma once

#include "execution.hpp"
#include "spawn.hpp"

#include <chrono>
#include <optional>
#include <string>
#include <vector>

namespace atlas::capabilities {

struct CommandRequest {
  std::string program;
  std::vector<std::string> args;
  std::optional<std::string> cwd;
  std::optional<std::chrono::milliseconds> timeout;
};

enum class CommandStatus {
  success,
  failed,
  timed_out,
  executable_not_found,
};

struct CommandResult {
  std::string stdout;
  std::string stderr;
  bool stdout_truncated{false};
  bool stderr_truncated{false};
  int exit_code{-1};
  std::chrono::milliseconds duration{0};
  CommandStatus status{CommandStatus::failed};
  std::string error;
};

// Oferece a semantica comum de comando sem acoplar o core a uma ferramenta.
CommandResult runCommand(
    const CommandRequest& request,
    const ExecutionOutputCallback& on_output = {});

}  // namespace atlas::capabilities
