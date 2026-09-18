#include "../../src/capabilities/tools/system/info/info.hpp"
#include "../../src/capabilities/core/discovery.hpp"
#include "../../src/capabilities/core/loader.hpp"
#include "../../src/capabilities/core/executor.hpp"
#include "../../src/capabilities/core/registry.hpp"

#include <algorithm>
#include <cstdlib>
#include <iostream>
#include <optional>
#include <string>
#include <utility>

namespace {

using atlas::capabilities::Discovery;
using atlas::capabilities::ExecutionResult;
using atlas::capabilities::ExecutionStatus;
using atlas::capabilities::Executor;
using atlas::capabilities::Loader;
using atlas::capabilities::Registry;
using atlas::capabilities::StructuredArguments;
using atlas::capabilities::StructuredValue;
using atlas::capabilities::tools::system::SystemInfo;
using atlas::capabilities::tools::system::systemInfo;

void require(bool condition, std::string_view message) {
  if (!condition) {
    std::cerr << "system.info test failed: " << message << '\n';
    std::exit(EXIT_FAILURE);
  }
}

const StructuredValue* outputField(const ExecutionResult& result, std::string_view name) {
  const auto* output = std::get_if<StructuredValue::Object>(&result.output.value);
  if (output == nullptr) {
    return nullptr;
  }
  const auto iterator = output->find(name);
  return iterator == output->end() ? nullptr : &iterator->second;
}

const std::string* stringOutput(const ExecutionResult& result, std::string_view name) {
  const StructuredValue* field = outputField(result, name);
  return field == nullptr ? nullptr : std::get_if<std::string>(&field->value);
}

void testSystemInfoFields() {
  const SystemInfo info = systemInfo();

  require(!info.platform.empty(), "platform should be collected without external commands");
  require(info.platform == "linux", "platform should be the lowered kernel name");
  require(!info.os_name.empty(), "os_name should be collected");
  require(!info.os_version.empty(), "os_version should be collected");
  require(!info.kernel_version.empty(), "kernel_version should come from uname");
  require(!info.architecture.empty(), "architecture should come from uname");
  require(!info.hostname.empty(), "hostname should come from uname");
  require(!info.username.empty(), "username should come from the effective user");
  require(!info.shell.empty(), "shell should come from the effective user");
  require(!info.timezone.empty(), "timezone should resolve to a name or UTC");
}

void testDiscoveryAndExecution() {
  Registry registry;
  Loader loader(registry);
  Discovery discovery(registry);

  require(
      loader.scan("src/capabilities/tools/system"),
      "system group and system.info should be loaded from their manifests");
  require(registry.get("system").has_value(), "system group should be registered");
  const auto registered = registry.get("system.info");
  require(registered.has_value(), "system.info should be registered");
  require(registered->parent == "system", "system.info should belong to the system group");
  require(
      registered->implementation.kind == "executable",
      "system.info should have an executable implementation");
  require(
      registered->implementation.entrypoint.find(
          "src/capabilities/tools/system/info/runtime") != std::string::npos,
      "system.info entrypoint should resolve relative to its capability");

  const auto discoverable = discovery.discover();
  require(
      std::any_of(
          discoverable.begin(),
          discoverable.end(),
          [](const auto& item) { return item.id == "system.info"; }),
      "system.info should be discoverable");

  Executor executor(registry);
  const ExecutionResult result = executor.execute("system.info", "local", {});
  require(result.status == ExecutionStatus::success, "system.info should succeed through Executor");
  require(result.target == "local", "Executor should preserve the target");
  require(result.error.empty(), "successful execution should not have an error");
  require(
      stringOutput(result, "platform") != nullptr && !stringOutput(result, "platform")->empty(),
      "platform should be exposed in the structured output");
  require(
      stringOutput(result, "kernel_version") != nullptr &&
          !stringOutput(result, "kernel_version")->empty(),
      "kernel_version should be exposed in the structured output");
  require(
      stringOutput(result, "username") != nullptr && !stringOutput(result, "username")->empty(),
      "username should be exposed in the structured output");
}

}  // namespace

int main() {
  testSystemInfoFields();
  testDiscoveryAndExecution();
  return EXIT_SUCCESS;
}
