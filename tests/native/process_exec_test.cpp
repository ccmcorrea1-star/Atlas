#include "../../src/capabilities/tools/process/exec.hpp"

#include <chrono>
#include <cstdlib>
#include <filesystem>
#include <iostream>
#include <string>
#include <utility>
#include <unistd.h>

namespace {

using atlas::capabilities::tools::process::ExecRequest;
using atlas::capabilities::tools::process::ExecStatus;
using atlas::capabilities::tools::process::exec;

void require(bool condition, const std::string& message) {
  if (!condition) {
    std::cerr << "process.exec test failed: " << message << '\n';
    std::exit(EXIT_FAILURE);
  }
}

ExecRequest localRequest(std::string program) {
  ExecRequest request;
  request.target = "local";
  request.program = std::move(program);
  return request;
}

void testSuccessfulExecution() {
  ExecRequest request = localRequest("/bin/printf");
  request.args = {"atlas:%s", "argument; remains literal"};

  const auto result = exec(request);

  require(result.status == ExecStatus::success, "successful process should have success status");
  require(result.stdout == "atlas:argument; remains literal", "stdout should be captured");
  require(result.stderr.empty(), "stderr should be empty");
  require(result.exit_code == 0, "successful process should exit with code zero");
  require(result.target == "local", "target should be preserved in the result");
  require(result.duration >= std::chrono::milliseconds::zero(), "duration should not be negative");
}

void testMissingProcess() {
  const auto result = exec(localRequest("/atlas/process-that-does-not-exist"));

  require(result.status == ExecStatus::failed, "missing process should fail");
  require(result.exit_code == 127, "missing process should use the exec failure code");
  require(result.error.find("failed to execute") != std::string::npos, "exec error should be returned");
}

void testWorkingDirectory(const std::filesystem::path& directory) {
  ExecRequest request = localRequest("/bin/pwd");
  request.cwd = directory.string();

  const auto result = exec(request);

  require(result.status == ExecStatus::success, "process with valid cwd should succeed");
  require(result.stdout == directory.string() + "\n", "process should run in the requested cwd");
}

void testTimeout() {
  ExecRequest request = localRequest("/bin/sleep");
  request.args = {"2"};
  request.timeout = std::chrono::milliseconds(100);

  const auto result = exec(request);

  require(result.status == ExecStatus::timed_out, "timed out process should have timeout status");
  require(result.error == "process timed out", "timeout error should be returned");
  require(result.duration >= std::chrono::milliseconds(50), "timeout should wait for the deadline");
  require(result.duration < std::chrono::seconds(2), "timeout should terminate the process");
}

void testStderrCapture() {
  ExecRequest request = localRequest("/bin/ls");
  request.args = {"/atlas/path-that-does-not-exist"};

  const auto result = exec(request);

  require(result.status == ExecStatus::failed, "non-zero process should have failed status");
  require(result.exit_code != 0, "failed process should have a non-zero exit code");
  require(!result.stderr.empty(), "stderr should be captured");
}

}  // namespace

int main() {
  const std::filesystem::path directory =
      std::filesystem::temp_directory_path() /
      ("atlas-process-exec-" + std::to_string(static_cast<long long>(getpid())));
  std::filesystem::remove_all(directory);
  std::filesystem::create_directories(directory);

  testSuccessfulExecution();
  testMissingProcess();
  testWorkingDirectory(directory);
  testTimeout();
  testStderrCapture();

  std::filesystem::remove_all(directory);
  return EXIT_SUCCESS;
}
