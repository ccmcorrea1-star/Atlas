#include "command_runner.hpp"

namespace atlas::capabilities {

CommandResult runCommand(
    const CommandRequest& request,
    const ExecutionOutputCallback& on_output) {
  SpawnRequest spawn_request;
  spawn_request.program = request.program;
  spawn_request.args = request.args;
  spawn_request.cwd = request.cwd;
  spawn_request.timeout = request.timeout;
  const SpawnResult spawned = spawn(spawn_request, on_output);

  CommandResult result;
  result.stdout = spawned.stdout;
  result.stderr = spawned.stderr;
  result.exit_code = spawned.exit_code;
  result.duration = spawned.duration;
  result.error = spawned.error;
  if (spawned.status == SpawnStatus::success) {
    result.status = CommandStatus::success;
  } else if (spawned.status == SpawnStatus::timed_out) {
    result.status = CommandStatus::timed_out;
  } else if (spawned.error_kind == SpawnErrorKind::executable_not_found) {
    result.status = CommandStatus::executable_not_found;
  } else {
    result.status = CommandStatus::failed;
  }
  return result;
}

}  // namespace atlas::capabilities
