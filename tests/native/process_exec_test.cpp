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
using atlas::capabilities::StructuredValue;

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
      .description = {},
      .schema = {},
  };
}

DiscoveryRequest queryRequest(std::string query) {
  return {.path = std::nullopt, .query = std::move(query)};
}

DiscoveryRequest pathRequest(std::string path) {
  return {.path = std::move(path), .query = std::nullopt};
}

const StructuredValue* objectField(const StructuredValue& value, std::string_view name) {
  const auto* object = std::get_if<StructuredValue::Object>(&value.value);
  if (object == nullptr) {
    return nullptr;
  }
  const auto iterator = object->find(name);
  return iterator == object->end() ? nullptr : &iterator->second;
}

void testRegistryAndDiscovery() {
  Registry registry;
  Discovery discovery(registry);
  Loader loader(registry);

  require(!registry.get("missing.capability").has_value(), "missing capability should not be returned");
  require(
      loader.scan("src/capabilities/tools/process"),
      "process group and process.exec should be loaded from their manifests");
  require(
      !loader.load("src/capabilities/tools/process/exec/capability.json"),
      "duplicate capability loading should fail");

  const auto group = registry.get("process");
  require(group.has_value(), "process group should be registered");
  require(group->type == "group", "process should be registered as a group");
  require(
      group->implementation.kind.empty() && group->implementation.entrypoint.empty(),
      "groups should not have an implementation");

  const auto registered = registry.get("process.exec");
  require(registered.has_value(), "registered capability should be returned");
  require(registered->type == "tool", "registered capability should expose its type");
  require(
      registered->summary == "executa um processo diretamente sem shell",
      "registered capability should expose its summary");
  require(
      registered->description == "executar um programa local diretamente, sem shell",
      "registered capability should expose its description");
  require(registered->parent == "process", "registered capability should expose its parent");
  require(
      registered->implementation.kind == "executable",
      "manifest should expose an executable implementation");
  require(
      registered->implementation.entrypoint.find("src/capabilities/tools/process/exec/implementation") !=
          std::string::npos,
      "manifest entrypoint should resolve relative to its capability");
  const StructuredValue* schemaProperties = objectField(registered->schema, "properties");
  require(schemaProperties != nullptr, "registered capability should preserve its schema");
  require(
      objectField(*schemaProperties, "program") != nullptr &&
          objectField(*schemaProperties, "timeout_ms") != nullptr,
      "registered capability should preserve complete schema properties");

  const auto definition = registry.getDefinition("process.exec");
  require(
      definition.has_value() && definition->description == registered->description &&
          objectField(definition->schema, "required") != nullptr,
      "complete capability definition should be retrievable from the Registry");

  const auto listed = registry.list();
  require(listed.size() == 2, "Registry list should contain the group and its capability");

  const auto root = discovery.discover();
  require(
      root.size() == 1 && root.front().id == "process" && root.front().type == "group" &&
          root.front().summary == "executar e gerenciar processos",
      "discover without a request should return only the process root group");

  const auto discoveredChildren = discovery.discover(pathRequest("process"));
  require(
      discoveredChildren.size() == 1 && discoveredChildren.front().id == "process.exec",
      "discover by path should return the process.exec capability");

  const auto directSearch = discovery.discover(queryRequest("executar programa"));
  require(
      directSearch.size() == 1 && directSearch.front().id == "process.exec",
      "direct Discovery search should find process.exec");

  const auto processChildren = registry.children("process");
  require(
      processChildren.size() == 1 && processChildren.front().id == "process.exec",
      "children should return direct children of a path");
  require(
      registry.children("unknown.path").empty(),
      "children should be empty for an unknown path");

  const auto processSearch = registry.search("EXECUTA PROCESSO DIRETAMENTE");
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
              .description = {},
              .schema = {},
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
      discovery.discover(queryRequest("EXECUTAR PROGRAMA")).empty(),
      "unloaded manifest capability should leave Discovery");
  require(loader.unload("process"), "unload should remove the process group");
  require(!registry.get("process").has_value(), "unloaded group should leave Registry");
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
