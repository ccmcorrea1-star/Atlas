#include "../../src/capabilities/discovery.hpp"
#include "../../src/capabilities/loader.hpp"
#include "../../src/capabilities/registry.hpp"

#include <cstdlib>
#include <filesystem>
#include <fstream>
#include <iostream>
#include <string>
#include <string_view>
#include <unistd.h>

namespace {

using atlas::capabilities::Discovery;
using atlas::capabilities::Loader;
using atlas::capabilities::Registry;

void require(bool condition, std::string_view message) {
  if (!condition) {
    std::cerr << "capability loader test failed: " << message << '\n';
    std::exit(EXIT_FAILURE);
  }
}

void writeManifest(const std::filesystem::path& path, const std::string& contents) {
  std::ofstream file(path, std::ios::binary | std::ios::trunc);
  require(static_cast<bool>(file), "manifest should be writable");
  file << contents;
  require(static_cast<bool>(file), "manifest should be written");
}

std::string manifest(
    std::string_view id,
    std::string_view summary,
    std::string_view kind = "native",
    std::string_view entrypoint = "atlas/test") {
  return "{\n"
         "  \"id\": \"" + std::string(id) + "\",\n"
         "  \"type\": \"tool\",\n"
         "  \"summary\": \"" + std::string(summary) + "\",\n"
         "  \"parent\": \"tests\",\n"
         "  \"aliases\": [\"test\"],\n"
         "  \"implementation\": {\"kind\": \"" + std::string(kind) +
         "\", \"entrypoint\": \"" + std::string(entrypoint) + "\"}\n"
         "}\n";
}

void testLoader(const std::filesystem::path& directory) {
  Registry registry;
  Loader loader(registry);
  Discovery discovery(registry);

  const std::filesystem::path validPath = directory / "valid" / "capability.json";
  std::filesystem::create_directories(validPath.parent_path());
  writeManifest(validPath, manifest("tests.echo", "repete texto"));

  require(loader.load(validPath), "valid manifest should load");
  const auto loaded = registry.get("tests.echo");
  require(loaded.has_value(), "loaded capability should be in Registry");
  require(loaded->implementation.kind == "native", "implementation kind should be preserved");
  require(loaded->implementation.entrypoint == "atlas/test", "entrypoint should be preserved");
  require(
      discovery.discover().size() == 1 && discovery.discover().front().id == "tests.echo",
      "loaded capability should appear in Discovery");

  const std::filesystem::path invalidPath = directory / "invalid" / "capability.json";
  std::filesystem::create_directories(invalidPath.parent_path());
  writeManifest(
      invalidPath,
      "{\"id\":\"tests.invalid\",\"type\":\"tool\",\"summary\":\"invalida\"}");
  require(!loader.load(invalidPath), "invalid manifest should be rejected");
  require(
      loader.lastError().find("implementation") != std::string::npos,
      "invalid manifest error should identify the invalid field");
  require(!registry.get("tests.invalid").has_value(), "invalid capability must not enter Registry");

  const std::filesystem::path duplicatePath = directory / "duplicate" / "capability.json";
  std::filesystem::create_directories(duplicatePath.parent_path());
  writeManifest(duplicatePath, manifest("tests.echo", "outra definicao"));
  require(!loader.load(duplicatePath), "duplicate capability should be rejected");
  require(
      loader.lastError().find("already registered") != std::string::npos,
      "duplicate error should clearly identify the existing registration");

  writeManifest(validPath, manifest("tests.echo", "texto atualizado", "python", "scripts/echo.py"));
  require(loader.reload("tests.echo"), "reload should update a loaded capability");
  const auto reloaded = registry.get("tests.echo");
  require(
      reloaded.has_value() && reloaded->summary == "texto atualizado" &&
          reloaded->implementation.kind == "python" &&
          reloaded->implementation.entrypoint == "scripts/echo.py",
      "reload should expose the changed manifest immediately");

  const std::filesystem::path scanDirectory = directory / "scan";
  const std::filesystem::path firstScanPath = scanDirectory / "first" / "capability.json";
  const std::filesystem::path secondScanPath = scanDirectory / "nested" / "second" / "capability.json";
  std::filesystem::create_directories(firstScanPath.parent_path());
  std::filesystem::create_directories(secondScanPath.parent_path());
  writeManifest(firstScanPath, manifest("scan.first", "primeira capability", "service", "http://localhost"));
  writeManifest(secondScanPath, manifest("scan.second", "segunda capability", "mcp", "server/tool"));
  require(loader.scan(scanDirectory), "scan should load manifests recursively");
  require(registry.get("scan.first").has_value(), "first scanned capability should be registered");
  require(registry.get("scan.second").has_value(), "nested scanned capability should be registered");

  require(loader.unload("tests.echo"), "unload should remove a loaded capability");
  require(!registry.get("tests.echo").has_value(), "unloaded capability should leave Registry");
  for (const auto& result : discovery.discover()) {
    require(result.id != "tests.echo", "unloaded capability should leave Discovery");
  }
  require(!loader.unload("tests.echo"), "unload should fail for a missing capability");
}

}  // namespace

int main() {
  const std::filesystem::path directory =
      std::filesystem::temp_directory_path() /
      ("atlas-capability-loader-" + std::to_string(static_cast<long long>(getpid())));
  std::filesystem::remove_all(directory);
  std::filesystem::create_directories(directory);

  testLoader(directory);

  std::filesystem::remove_all(directory);
  return EXIT_SUCCESS;
}
