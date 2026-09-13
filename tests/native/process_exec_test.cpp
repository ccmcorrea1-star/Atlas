#include "../../src/capabilities/tools/process/exec/exec.hpp"
#include "../../src/capabilities/core/discovery.hpp"
#include "../../src/capabilities/core/loader.hpp"
#include "../../src/capabilities/core/registry.hpp"

#include <chrono>
#include <cstdlib>
#include <filesystem>
#include <iostream>
#include <optional>
#include <string>
#include <utility>
#include <unistd.h>

namespace {

using atlas::capabilities::tools::process::ExecRequest;
using atlas::capabilities::tools::process::ExecStatus;
using atlas::capabilities::tools::process::exec;
using atlas::capabilities::Capability;
using atlas::capabilities::Discovery;
using atlas::capabilities::DiscoveryRequest;
using atlas::capabilities::Loader;
using atlas::capabilities::Registry;

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

Capability runtimeCapability() {
  return {
      .id = "runtime.echo",
      .type = "tool",
      .summary = "repete um texto",
      .parent = "runtime",
      .aliases = {"echo"},
      .implementation = "test://runtime/echo",
  };
}

DiscoveryRequest queryRequest(std::string query) {
  return {.path = std::nullopt, .query = std::move(query)};
}

DiscoveryRequest pathRequest(std::string path) {
  return {.path = std::move(path), .query = std::nullopt};
}

void testRegistryAndDiscovery() {
  Registry registry;
  Discovery discovery(registry);
  Loader loader(registry);

  require(!registry.get("missing.capability").has_value(), "missing capability should not be returned");
  require(
      loader.load("src/capabilities/tools/process/exec/capability.json"),
      "process.exec should be loaded from its manifest");
  require(
      !loader.load("src/capabilities/tools/process/exec/capability.json"),
      "duplicate capability loading should fail");

  const auto registered = registry.get("process.exec");
  require(registered.has_value(), "registered capability should be returned");
  require(registered->type == "tool", "registered capability should expose its type");
  require(
      registered->summary == "executa um processo diretamente sem shell",
      "registered capability should expose its summary");
  require(registered->parent == "process", "registered capability should expose its parent");
  require(
      registered->implementation.kind == "executable",
      "manifest should expose an executable implementation");
  require(
      registered->implementation.entrypoint.find("src/capabilities/tools/process/exec/implementation") !=
          std::string::npos,
      "manifest entrypoint should resolve relative to its capability");

  const auto listed = registry.list();
  require(listed.size() == 1 && listed.front().id == "process.exec", "list should contain process.exec");

  const auto processChildren = registry.children("process");
  require(
      processChildren.size() == 1 && processChildren.front().id == "process.exec",
      "children should return direct children of a path");
  require(
      registry.children("unknown.path").empty(),
      "children should be empty for an unknown path");

  const auto processSearch = registry.search("EXECUTA PROCESSO");
  require(
      processSearch.size() == 1 && processSearch.front().id == "process.exec",
      "search should match summary tokens case-insensitively");

  Capability dynamic = runtimeCapability();
  require(registry.registerCapability(dynamic), "runtime capability should be registered");
  require(
      discovery.discover(queryRequest("echo")).size() == 1,
      "new capability should appear in Discovery search");
  const auto runtimeChildren = discovery.discover(pathRequest("runtime"));
  require(
      runtimeChildren.size() == 1 && runtimeChildren.front().id == "runtime.echo" &&
          runtimeChildren.front().type == "tool" &&
          runtimeChildren.front().summary == "repete um texto",
      "new capability should appear in Discovery children");

  dynamic.summary = "repete texto atualizado";
  dynamic.aliases = {"repeat"};
  require(registry.update(dynamic), "existing capability should be updated");
  const auto updated = registry.get("runtime.echo");
  require(
      updated.has_value() && updated->summary == "repete texto atualizado",
      "get should return the updated capability");
  require(
      discovery.discover(queryRequest("repeat")).front().summary ==
          "repete texto atualizado",
      "Discovery should read updated Registry state");
  require(
      !registry.update(
          "runtime.echo",
          Capability{
              .id = "other.id",
              .type = "tool",
              .summary = "invalid identity",
              .parent = std::nullopt,
              .aliases = {},
              .implementation = "test://other",
          }),
      "update should not change a capability identity");
  Capability missing = runtimeCapability();
  missing.id = "missing.capability";
  require(!registry.update(missing), "update should fail for a missing capability");

  require(registry.unregister("runtime.echo"), "existing capability should be removed");
  require(!registry.get("runtime.echo").has_value(), "removed capability should not be returned");
  require(
      discovery.discover(queryRequest("repeat")).empty(),
      "removed capability should disappear from Discovery");
  require(!registry.unregister("missing.capability"), "unregister should fail for missing capability");
  require(loader.unload("process.exec"), "unload should remove the manifest capability");
  require(!registry.get("process.exec").has_value(), "unloaded manifest capability should leave Registry");
  require(
      discovery.discover(queryRequest("EXECUTA PROCESSO")).empty(),
      "unloaded manifest capability should leave Discovery");
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
  testRegistryAndDiscovery();

  std::filesystem::remove_all(directory);
  return EXIT_SUCCESS;
}
