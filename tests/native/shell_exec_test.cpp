#include "../../src/capabilities/tools/shell/exec/shell.hpp"
#include "../../src/capabilities/core/discovery.hpp"
#include "../../src/capabilities/core/loader.hpp"
#include "../../src/capabilities/core/registry.hpp"

#include <chrono>
#include <cstdlib>
#include <filesystem>
#include <fstream>
#include <iostream>
#include <optional>
#include <string>
#include <unistd.h>

namespace {

using atlas::capabilities::tools::shell::ShellRequest;
using atlas::capabilities::tools::shell::ShellStatus;
using atlas::capabilities::tools::shell::dispatch;
using atlas::capabilities::tools::shell::exec;
using atlas::capabilities::Capability;
using atlas::capabilities::Discovery;
using atlas::capabilities::DiscoveryRequest;
using atlas::capabilities::ExecutionStatus;
using atlas::capabilities::Loader;
using atlas::capabilities::NativeRequest;
using atlas::capabilities::Registry;
using atlas::capabilities::StructuredValue;

void require(bool condition, const std::string& message) {
  if (!condition) {
    std::cerr << "shell.exec test failed: " << message << '\n';
    std::exit(EXIT_FAILURE);
  }
}

ShellRequest localRequest(std::string command) {
  ShellRequest request;
  request.target = "local";
  request.command = std::move(command);
  return request;
}

void testLiteralSuccess() {
  const auto result = exec(localRequest("printf 'shell:%s' 'ok'"));

  require(result.status == ShellStatus::success, "successful command should have success status");
  require(result.stdout == "shell:ok", "stdout should be captured");
  require(result.stderr.empty(), "stderr should be empty");
  require(result.exit_code == 0, "successful command should exit with code zero");
  require(result.target == "local", "target should be preserved in the result");
  require(result.error.empty(), "error should be empty on success");
  require(result.duration >= std::chrono::milliseconds::zero(), "duration should not be negative");
}

void testPipe() {
  const auto result = exec(localRequest("printf 'a\\nb\\nc\\n' | wc -l"));

  require(result.status == ShellStatus::success, "piped command should succeed");
  require(result.stdout == "3\n", "pipe output should be captured");
}

void testRedirect(const std::filesystem::path& directory) {
  ShellRequest request = localRequest("printf redirect-data > out.txt");
  request.cwd = directory.string();

  const auto written = exec(request);
  require(written.status == ShellStatus::success, "redirected command should succeed");

  std::ifstream file(directory / "out.txt", std::ios::binary);
  const std::string contents{
      std::istreambuf_iterator<char>(file),
      std::istreambuf_iterator<char>()};
  require(contents == "redirect-data", "redirection should write the file");

  ShellRequest reader = localRequest("cat < out.txt");
  reader.cwd = directory.string();
  const auto result = exec(reader);
  require(result.status == ShellStatus::success, "input redirect should succeed");
  require(result.stdout == "redirect-data", "input redirect should be captured");
}

void testChaining() {
  const auto result = exec(localRequest("false && printf no || printf yes"));

  require(result.status == ShellStatus::success, "chained command should succeed");
  require(result.stdout == "yes", "shell operators should chain commands");
}

void testGlobbing(const std::filesystem::path& directory) {
  std::ofstream(directory / "glob-a.txt") << "a";
  std::ofstream(directory / "glob-b.txt") << "b";

  ShellRequest request = localRequest("printf '%s\\n' glob-*.txt");
  request.cwd = directory.string();

  const auto result = exec(request);
  require(result.status == ShellStatus::success, "glob command should succeed");
  require(
      result.stdout == "glob-a.txt\nglob-b.txt\n",
      "globbing should expand matching files");
}

void testVariableExpansion() {
  const auto result = exec(localRequest("atlas_probe=42; printf '%s' \"$atlas_probe\""));

  require(result.status == ShellStatus::success, "variable expansion should succeed");
  require(result.stdout == "42", "shell variable should expand");
}

void testFailure() {
  const auto result = exec(localRequest("exit 3"));

  require(result.status == ShellStatus::failed, "non-zero command should have failed status");
  require(result.exit_code == 3, "failed command should keep its exit code");
  require(
      result.error == "process exited with code 3",
      "exit code error should be returned");
}

void testMissingCommand() {
  const auto result = exec(localRequest("atlas-command-that-does-not-exist"));

  require(result.status == ShellStatus::failed, "missing command should fail");
  require(result.exit_code == 127, "missing command should use the exec failure code");
}

void testTimeout() {
  ShellRequest request = localRequest("sleep 2");
  request.timeout = std::chrono::milliseconds(100);

  const auto result = exec(request);

  require(result.status == ShellStatus::timed_out, "timed out command should have timeout status");
  require(result.error == "process timed out", "timeout error should be returned");
  require(result.duration >= std::chrono::milliseconds(50), "timeout should wait for the deadline");
  require(result.duration < std::chrono::seconds(2), "timeout should terminate the command");
}

void testWorkingDirectory(const std::filesystem::path& directory) {
  ShellRequest request = localRequest("pwd");
  request.cwd = directory.string();

  const auto result = exec(request);

  require(result.status == ShellStatus::success, "command with valid cwd should succeed");
  require(result.stdout == directory.string() + "\n", "command should run in the requested cwd");
}

void testEmptyCommand() {
  const auto result = exec(localRequest(""));

  require(result.status == ShellStatus::failed, "empty command should fail");
  require(result.error == "command cannot be empty", "empty command error should be returned");
}

const StructuredValue* objectField(const StructuredValue& value, std::string_view name) {
  const auto* object = std::get_if<StructuredValue::Object>(&value.value);
  if (object == nullptr) {
    return nullptr;
  }
  const auto iterator = object->find(name);
  return iterator == object->end() ? nullptr : &iterator->second;
}

void testDispatch() {
  NativeRequest request;
  request.target = "local";
  request.arguments = StructuredValue::Object{{"command", StructuredValue("printf dispatch-ok")}};

  const auto result = dispatch(request);
  require(result.status == ExecutionStatus::success, "dispatch should execute the command");
  const StructuredValue* stdout = objectField(result.output, "stdout");
  const auto* text = stdout == nullptr ? nullptr : std::get_if<std::string>(&stdout->value);
  require(text != nullptr && *text == "dispatch-ok", "dispatch should expose stdout");

  NativeRequest missing;
  missing.target = "local";
  missing.arguments = StructuredValue::Object{};
  const auto failure = dispatch(missing);
  require(
      failure.status == ExecutionStatus::failed &&
          failure.error.find("'command'") != std::string::npos,
      "dispatch should reject a missing command");
}

void testRegistryAndDiscovery() {
  Registry registry;
  Discovery discovery(registry);
  Loader loader(registry);

  require(
      loader.scan("src/capabilities/tools/shell"),
      "shell group and shell.exec should be loaded from their manifests");
  require(
      !loader.load("src/capabilities/tools/shell/exec/capability.json"),
      "duplicate capability loading should fail");

  const auto group = registry.get("shell");
  require(group.has_value(), "shell group should be registered");
  require(group->type == "group", "shell should be registered as a group");

  const auto registered = registry.get("shell.exec");
  require(registered.has_value(), "registered capability should be returned");
  require(registered->type == "tool", "registered capability should expose its type");
  require(
      registered->summary == "executa um comando usando o shell do sistema",
      "registered capability should expose its summary");
  require(
      registered->description.find("pipes") != std::string::npos,
      "registered capability should expose its description");
  require(registered->parent == "shell", "registered capability should expose its parent");
  require(
      registered->implementation.kind == "executable",
      "manifest should expose an executable implementation");
  require(
      registered->implementation.entrypoint.find("src/capabilities/tools/shell/exec/runtime") !=
          std::string::npos,
      "manifest entrypoint should resolve relative to its capability");
  const StructuredValue* schemaProperties = objectField(registered->schema, "properties");
  require(schemaProperties != nullptr, "registered capability should preserve its schema");
  require(
      objectField(*schemaProperties, "command") != nullptr &&
          objectField(*schemaProperties, "timeout_ms") != nullptr,
      "registered capability should preserve complete schema properties");

  const auto definition = discovery.getDefinition("shell.exec");
  require(
      definition.has_value() && definition->description == registered->description &&
          objectField(definition->schema, "required") != nullptr,
      "complete capability definition should be retrievable from the Registry");

  const auto children = registry.children("shell");
  require(
      children.size() == 1 && children.front().id == "shell.exec",
      "shell group should list shell.exec as its child");

  DiscoveryRequest search;
  search.query = "shell";
  const auto found = discovery.discover(search);
  require(
      found.size() == 1 && found.front().id == "shell.exec",
      "Discovery search should find shell.exec");

  require(loader.unload("shell.exec"), "unload should remove the manifest capability");
  require(!registry.get("shell.exec").has_value(), "unloaded manifest capability should leave Registry");
  require(loader.unload("shell"), "unload should remove the shell group");
  require(!registry.get("shell").has_value(), "unloaded group should leave Registry");
}

}  // namespace

int main() {
  const std::filesystem::path directory =
      std::filesystem::temp_directory_path() /
      ("atlas-shell-exec-" + std::to_string(static_cast<long long>(getpid())));
  std::filesystem::remove_all(directory);
  std::filesystem::create_directories(directory);

  testLiteralSuccess();
  testPipe();
  testRedirect(directory);
  testChaining();
  testGlobbing(directory);
  testVariableExpansion();
  testFailure();
  testMissingCommand();
  testTimeout();
  testWorkingDirectory(directory);
  testEmptyCommand();
  testDispatch();
  testRegistryAndDiscovery();

  std::filesystem::remove_all(directory);
  return EXIT_SUCCESS;
}
