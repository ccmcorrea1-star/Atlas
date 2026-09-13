#include "../../src/capabilities/core/executor.hpp"
#include "../../src/capabilities/core/loader.hpp"

#include <cstdint>
#include <cstdlib>
#include <iostream>
#include <optional>
#include <string>
#include <string_view>
#include <utility>

namespace {

using atlas::capabilities::Capability;
using atlas::capabilities::ExecutionResult;
using atlas::capabilities::ExecutionStatus;
using atlas::capabilities::Executor;
using atlas::capabilities::NativeRequest;
using atlas::capabilities::Registry;
using atlas::capabilities::StructuredArguments;
using atlas::capabilities::StructuredValue;
using atlas::capabilities::Loader;

void require(bool condition, std::string_view message) {
  if (!condition) {
    std::cerr << "capability executor test failed: " << message << '\n';
    std::exit(EXIT_FAILURE);
  }
}

Capability descriptor(
    std::string id,
    std::string kind,
    std::string entrypoint) {
  return {
      .id = std::move(id),
      .type = "tool",
      .summary = "capability de teste",
      .parent = std::nullopt,
      .aliases = {},
      .implementation = {std::move(kind), std::move(entrypoint)},
  };
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

void testProcessExecution() {
  Registry registry;
  Loader loader(registry);
  require(
      loader.load("src/capabilities/tools/process/exec/capability.json"),
      "process.exec should be loaded from its manifest");

  Executor executor(registry);
  StructuredArguments arguments{
      {"program", "/bin/printf"},
      {"args", StructuredValue::Array{"executor:%s", "native"}},
  };
  const ExecutionResult result = executor.execute("process.exec", "local", std::move(arguments));

  require(result.status == ExecutionStatus::success, "process.exec should succeed through Executor");
  require(result.target == "local", "Executor should preserve the target");
  require(result.error.empty(), "successful execution should not have an error");
  require(
      stringOutput(result, "stdout") != nullptr && *stringOutput(result, "stdout") == "executor:native",
      "process output should be structured under stdout");
  require(
      stringOutput(result, "status") != nullptr && *stringOutput(result, "status") == "success",
      "structured output should expose the process status");
  const StructuredValue* exit_code = outputField(result, "exit_code");
  require(
      exit_code != nullptr && std::get_if<std::int64_t>(&exit_code->value) != nullptr &&
          *std::get_if<std::int64_t>(&exit_code->value) == 0,
      "structured output should expose the exit code");

  StructuredArguments timeoutArguments{
      {"program", "/bin/sleep"},
      {"args", StructuredValue::Array{"2"}},
      {"timeout_ms", 100},
  };
  const ExecutionResult timeout = executor.execute("process.exec", "local", std::move(timeoutArguments));
  require(timeout.status == ExecutionStatus::timed_out, "process.exec should preserve executable timeouts");
}

void testMissingCapability() {
  Registry registry;
  const ExecutionResult result = Executor(registry).execute("missing.capability", "local", {});

  require(result.status == ExecutionStatus::failed, "missing capability should fail");
  require(
      result.error == "capability 'missing.capability' is not registered",
      "missing capability should have a clear error");
}

void testUnsupportedKinds() {
  Registry registry;
  for (const std::string_view kind : {"python", "service", "mcp"}) {
    const std::string id = "unsupported." + std::string(kind);
    require(
        registry.registerCapability(descriptor(id, std::string(kind), "not-used")),
        "unsupported capability should be registerable");
    const ExecutionResult result = Executor(registry).execute(id, "local", {});
    require(result.status == ExecutionStatus::unavailable, "unsupported kind should be unavailable");
    require(
        result.error.find("implementation kind '" + std::string(kind) + "'") != std::string::npos,
        "unsupported kind should identify the adapter");
  }
}

void testInvalidEntrypoint() {
  Registry registry;
  require(
      registry.registerCapability(descriptor("invalid.entrypoint", "native", "missing/native")),
      "capability with a valid descriptor should be registerable");

  const ExecutionResult result = Executor(registry).execute("invalid.entrypoint", "local", {});
  require(result.status == ExecutionStatus::failed, "invalid native entrypoint should fail");
  require(
      result.error == "native entrypoint 'missing/native' is not registered",
      "invalid entrypoint should have a clear error");
}

void testExecutableFailure() {
  Registry registry;
  require(
      registry.registerCapability(descriptor("failed.executable", "executable", "/bin/false")),
      "executable capability should be registerable");

  const ExecutionResult result = Executor(registry).execute("failed.executable", "local", {});
  require(result.status == ExecutionStatus::failed, "a failed executable should return failed status");
  require(
      result.error.find("exited with code") != std::string::npos,
      "failed executable should expose its exit code");
}

void testCapabilityError() {
  Registry registry;
  require(
      registry.registerNativeEntrypoint(
          "tests/error",
          [](const NativeRequest& request) {
            ExecutionResult result;
            result.target = request.target;
            result.status = ExecutionStatus::failed;
            result.output = StructuredValue::Object{{"message", "partial output"}};
            result.error = "error returned by capability";
            return result;
          }),
      "native error entrypoint should be registered");
  require(
      registry.registerCapability(descriptor("tests.error", "native", "tests/error")),
      "native error capability should be registered");

  const ExecutionResult result = Executor(registry).execute("tests.error", "local", {});
  require(result.status == ExecutionStatus::failed, "capability error should preserve failed status");
  require(result.error == "error returned by capability", "capability error should be preserved");
  require(
      stringOutput(result, "message") != nullptr && *stringOutput(result, "message") == "partial output",
      "capability output should be preserved structurally");
}

void testInvalidImplementationIsNotRunnable() {
  Registry registry;
  Capability capability = descriptor("invalid.implementation", "native", "");
  require(!registry.registerCapability(std::move(capability)), "invalid implementation should not register");
}

}  // namespace

int main() {
  testProcessExecution();
  testMissingCapability();
  testUnsupportedKinds();
  testInvalidEntrypoint();
  testExecutableFailure();
  testCapabilityError();
  testInvalidImplementationIsNotRunnable();
  return EXIT_SUCCESS;
}
