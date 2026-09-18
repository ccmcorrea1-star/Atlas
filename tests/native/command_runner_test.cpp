#include "../../src/capabilities/core/command_runner.hpp"

#include <cstdlib>
#include <iostream>
#include <string>

namespace {

using atlas::capabilities::CommandRequest;
using atlas::capabilities::CommandStatus;
using atlas::capabilities::runCommand;

void require(bool condition, const std::string& message) {
  if (!condition) {
    std::cerr << "command runner test failed: " << message << '\n';
    std::exit(EXIT_FAILURE);
  }
}

void testSuccessfulCommand() {
  CommandRequest request;
  request.program = "/bin/printf";
  request.args = {"runner:%s", "ok"};
  const auto result = runCommand(request);
  require(result.status == CommandStatus::success, "successful command should succeed");
  require(result.stdout == "runner:ok", "stdout should be captured");
  require(result.stderr.empty(), "stderr should be empty");
  require(result.exit_code == 0, "successful command should expose exit code");
}

void testExecutableNotFoundIsExplicit() {
  CommandRequest request;
  request.program = "/atlas/command-that-does-not-exist";
  const auto result = runCommand(request);
  require(
      result.status == CommandStatus::executable_not_found,
      "missing executable should have an explicit status");
  require(result.exit_code == 127, "missing executable should preserve the process exit code");
}

void testExitCode127IsNotEnough() {
  CommandRequest request;
  request.program = "/bin/sh";
  request.args = {"-c", "exit 127"};
  const auto result = runCommand(request);
  require(result.status == CommandStatus::failed, "a real exit 127 should be a normal failure");
  require(result.status != CommandStatus::executable_not_found, "exit 127 alone must not imply missing executable");
  require(result.exit_code == 127, "the real exit code should be preserved");
}

}  // namespace

int main() {
  testSuccessfulCommand();
  testExecutableNotFoundIsExplicit();
  testExitCode127IsNotEnough();
  return EXIT_SUCCESS;
}
