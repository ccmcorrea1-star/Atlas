#include "../../src/capabilities/core/executor.hpp"
#include "../../src/capabilities/core/loader.hpp"

#include <cstdint>
#include <cstdlib>
#include <filesystem>
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
       .description = {},
       .schema = {},
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

void testShellExecution() {
  Registry registry;
  Loader loader(registry);
  const std::filesystem::path shellDirectory = "src/capabilities/tools/shell/exec";
  require(
      !std::filesystem::exists(shellDirectory / "implementation.cpp"),
      "shell.exec should not have a tool-specific implementation.cpp");
  require(
      loader.load("src/capabilities/tools/shell/exec/capability.json"),
      "shell.exec should be loaded from its manifest");

  Executor executor(registry);
  StructuredArguments arguments{
      {"command", "printf 'executor:%s' native"},
  };
  const ExecutionResult result = executor.execute("shell.exec", "local", std::move(arguments));

  require(result.status == ExecutionStatus::success, "shell.exec should succeed through Executor");
  require(result.target == "local", "Executor should preserve the target");
  require(result.error.empty(), "successful execution should not have an error");
  require(
      stringOutput(result, "stdout") != nullptr && *stringOutput(result, "stdout") == "executor:native",
      "shell output should be structured under stdout");
  require(
      stringOutput(result, "status") != nullptr && *stringOutput(result, "status") == "success",
      "structured output should expose the shell status");
  const StructuredValue* exit_code = outputField(result, "exit_code");
  require(
      exit_code != nullptr && std::get_if<std::int64_t>(&exit_code->value) != nullptr &&
          *std::get_if<std::int64_t>(&exit_code->value) == 0,
      "structured output should expose the exit code");

  StructuredArguments timeoutArguments{
      {"command", "sleep 2"},
      {"timeout_ms", 100},
  };
  const ExecutionResult timeout = executor.execute("shell.exec", "local", std::move(timeoutArguments));
  require(timeout.status == ExecutionStatus::timed_out, "shell.exec should preserve executable timeouts");

  StructuredArguments legacyTimeoutArguments{
      {"command", "sleep 2"},
      {"timeout", 100},
  };
  const ExecutionResult legacyTimeout = executor.execute(
      "shell.exec", "local", std::move(legacyTimeoutArguments));
  require(
      legacyTimeout.status == ExecutionStatus::timed_out,
      "shell.exec should preserve the legacy timeout alias");
}

void testGroupIsNotExecutable() {
  Registry registry;
  Loader loader(registry);
  require(
      loader.load("src/capabilities/tools/process/group.json"),
      "process group should be loaded from its group manifest");

  const ExecutionResult result = Executor(registry).execute("process", "local", {});
  require(result.status == ExecutionStatus::failed, "a group should not be executable");
  require(
      result.error == "capability 'process' of type 'group' is not executable",
      "group execution should be rejected by type");
}

void testSkillDoesNotEnterToolRegistry() {
  Registry registry;
  Capability skill = descriptor("process.procedure", "native", "not-used");
  skill.type = "skill";
  require(!registry.registerCapability(std::move(skill)), "a Skill should not enter the Tool Registry");
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

void testCentralSchemaValidation() {
  Registry registry;
  bool called = false;
  Capability capability = descriptor("tests.schema", "native", "tests/schema");
  capability.schema = StructuredValue::Object{
      {"type", "object"},
      {"properties", StructuredValue::Object{
          {"name", StructuredValue::Object{{"type", "string"}}},
          {"count", StructuredValue::Object{{"type", "integer"}, {"minimum", 1}, {"maximum", 3}}},
          {"tags", StructuredValue::Object{{"type", "array"}, {"items", StructuredValue::Object{{"type", "string"}}}}},
      }},
      {"required", StructuredValue::Array{"name", "count", "tags"}},
      {"additionalProperties", false},
  };
  require(
      registry.registerNativeEntrypoint(
          "tests/schema",
          [&called](const NativeRequest& request) {
            called = true;
            ExecutionResult result;
            result.target = request.target;
            result.status = ExecutionStatus::success;
            return result;
          }),
      "schema test entrypoint should be registered");
  require(registry.registerCapability(std::move(capability)), "schema capability should be registered");

  const auto checkRejected = [&](StructuredArguments arguments, std::string_view detail) {
    called = false;
    const ExecutionResult result = Executor(registry).execute("tests.schema", "local", std::move(arguments));
    require(result.status == ExecutionStatus::failed, "invalid schema arguments should fail");
    require(result.error.find(detail) != std::string::npos, "schema error should identify the violation");
    require(!called, "schema validation should happen before the native runtime");
  };
  checkRejected({{"count", 2}, {"tags", StructuredValue::Array{"ok"}}}, "name is required");
  checkRejected({{"name", 7}, {"count", 2}, {"tags", StructuredValue::Array{"ok"}}}, "name must be a string");
  checkRejected({{"name", "ok"}, {"count", 0}, {"tags", StructuredValue::Array{"ok"}}}, "violates minimum");
  checkRejected({{"name", "ok"}, {"count", 4}, {"tags", StructuredValue::Array{"ok"}}}, "violates maximum");
  checkRejected({{"name", "ok"}, {"count", 2}, {"tags", StructuredValue::Array{7}}}, "tags[0] must be a string");
  checkRejected({{"name", "ok"}, {"count", 2}, {"tags", StructuredValue::Array{"ok"}}, {"extra", true}}, "extra is not allowed");

  const ExecutionResult valid = Executor(registry).execute(
      "tests.schema",
      "local",
      {{"name", "ok"}, {"count", 2}, {"tags", StructuredValue::Array{"ok"}}});
  require(valid.status == ExecutionStatus::success && called, "valid schema arguments should reach the runtime");
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
  testShellExecution();
  testGroupIsNotExecutable();
  testSkillDoesNotEnterToolRegistry();
  testMissingCapability();
  testUnsupportedKinds();
  testInvalidEntrypoint();
  testExecutableFailure();
  testCentralSchemaValidation();
  testCapabilityError();
  testInvalidImplementationIsNotRunnable();
  return EXIT_SUCCESS;
}
