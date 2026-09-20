#include "../../src/capabilities/core/discovery.hpp"
#include "../../src/capabilities/core/loader.hpp"
#include "../../src/capabilities/core/registry.hpp"
#include "../../src/skills/discovery.hpp"
#include "../../src/skills/loader.hpp"
#include "../../src/skills/registry.hpp"

#include <cerrno>
#include <cstdlib>
#include <filesystem>
#include <fstream>
#include <iostream>
#include <sstream>
#include <string>
#include <string_view>
#include <sys/wait.h>
#include <unistd.h>
#include <vector>

namespace {

using atlas::capabilities::Discovery;
using atlas::capabilities::Loader;
using atlas::capabilities::Registry;
using atlas::capabilities::StructuredValue;
using atlas::skills::SkillDiscovery;
using atlas::skills::SkillLoader;
using atlas::skills::SkillRegistry;
using atlas::skills::SkillSource;

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

std::string groupManifest(std::string_view id, std::string_view summary) {
  return "{\n"
         "  \"id\": \"" + std::string(id) + "\",\n"
         "  \"summary\": \"" + std::string(summary) + "\"\n"
         "}\n";
}

std::string skillDocument(
    std::string_view name,
    std::string_view description,
    std::string_view instructions) {
  return "---\nname: " + std::string(name) + "\ndescription: " + std::string(description) +
      "\n---\n" + std::string(instructions);
}

const StructuredValue* objectField(
    const StructuredValue::Object& object,
    std::string_view name) {
  const auto iterator = object.find(name);
  return iterator == object.end() ? nullptr : &iterator->second;
}

std::string runBridge(
    const std::filesystem::path& executable,
    const std::filesystem::path& directory,
    const std::filesystem::path& home,
    const std::string& input) {
  int inputPipe[2] = {-1, -1};
  int outputPipe[2] = {-1, -1};
  require(pipe(inputPipe) == 0 && pipe(outputPipe) == 0, "bridge pipes should be created");
  const pid_t child = fork();
  require(child != -1, "bridge process should fork");
  if (child == 0) {
    if (dup2(inputPipe[0], STDIN_FILENO) == -1 || dup2(outputPipe[1], STDOUT_FILENO) == -1 ||
        chdir(directory.c_str()) == -1 || setenv("HOME", home.c_str(), 1) == -1) {
      _exit(126);
    }
    close(inputPipe[0]);
    close(inputPipe[1]);
    close(outputPipe[0]);
    close(outputPipe[1]);
    execl(executable.c_str(), executable.c_str(), static_cast<char*>(nullptr));
    _exit(127);
  }

  close(inputPipe[0]);
  close(outputPipe[1]);
  std::size_t written = 0;
  while (written < input.size()) {
    const ssize_t count = write(inputPipe[1], input.data() + written, input.size() - written);
    if (count > 0) {
      written += static_cast<std::size_t>(count);
    } else if (count == -1 && errno == EINTR) {
      continue;
    } else {
      require(false, "bridge request should be written");
    }
  }
  close(inputPipe[1]);

  std::string output;
  char buffer[4096];
  while (true) {
    const ssize_t count = read(outputPipe[0], buffer, sizeof(buffer));
    if (count > 0) {
      output.append(buffer, static_cast<std::size_t>(count));
    } else if (count == 0) {
      break;
    } else if (errno != EINTR) {
      require(false, "bridge output should be readable");
    }
  }
  close(outputPipe[0]);

  int status = 0;
  require(waitpid(child, &status, 0) == child, "bridge process should be reaped");
  require(WIFEXITED(status) && WEXITSTATUS(status) == 0, "bridge process should exit successfully");
  return output;
}

void testBridge(const std::filesystem::path& directory) {
  const std::filesystem::path project = directory / "bridge-project";
  const std::filesystem::path home = directory / "bridge-home";
  std::filesystem::create_directories(project / ".atlas" / "skills" / "local");
  std::filesystem::create_directories(project / ".agents" / "skills" / "agent");
  std::filesystem::create_directories(home / ".config" / "atlas" / "skills" / "global");
  writeManifest(
      project / ".atlas" / "skills" / "local" / "SKILL.md",
      skillDocument("bridge.local", "skill local", "Use the local tools."));
  writeManifest(
      project / ".agents" / "skills" / "agent" / "SKILL.md",
      skillDocument("bridge.agent", "skill agent", "Use the agent tools."));
  writeManifest(
      home / ".config" / "atlas" / "skills" / "global" / "SKILL.md",
      skillDocument("bridge.global", "skill global", "Use the global tools."));

  const std::filesystem::path executable =
      std::filesystem::absolute("src/capabilities/runtime/bridge/runtime");
  require(std::filesystem::exists(executable), "bridge executable should be built");
  const std::string input =
      "{\"operation\":\"get_skill\",\"id\":\"bridge.local\",\"request_id\":\"local\"}\n"
      "{\"operation\":\"get_skill\",\"id\":\"bridge.global\",\"request_id\":\"global\"}\n"
      "{\"operation\":\"get_skill\",\"id\":\"bridge.agent\",\"request_id\":\"agent\"}\n"
      "{\"operation\":\"discover\",\"query\":\"skill local\",\"request_id\":\"discover\"}\n"
      "{\"operation\":\"execute\",\"id\":\"bridge.local\",\"target\":\"local\",\"request_id\":\"execute\"}\n";
  const std::string output = runBridge(executable, project, home, input);

  std::vector<StructuredValue> responses;
  std::istringstream lines(output);
  std::string line;
  while (std::getline(lines, line)) {
    std::string parseError;
    const auto parsed = atlas::capabilities::parseJson(line, parseError);
    require(parsed.has_value(), "bridge response should be valid JSON");
    responses.push_back(parsed.value());
  }
  require(responses.size() == 5, "bridge should return one response per request");

  for (std::size_t index = 0; index < 3; ++index) {
    const auto* response = std::get_if<StructuredValue::Object>(&responses[index].value);
    require(response != nullptr, "skill response should be an object");
    const auto* skill = objectField(*response, "skill");
    const auto* skillObject = skill == nullptr ? nullptr : std::get_if<StructuredValue::Object>(&skill->value);
    require(skillObject != nullptr, "get_skill should return a materialized skill");
    const auto* instructions = objectField(*skillObject, "instructions");
    require(
        instructions != nullptr && std::get_if<std::string>(&instructions->value) != nullptr,
        "materialized skill should include instructions");
  }

  const auto* discoveryResponse = std::get_if<StructuredValue::Object>(&responses[3].value);
  require(discoveryResponse != nullptr, "unified discovery response should be an object");
  require(objectField(*discoveryResponse, "results") != nullptr, "discovery should include Skill results");

  const auto* execution = std::get_if<StructuredValue::Object>(&responses.back().value);
  require(execution != nullptr, "execute response should be an object");
  const auto* error = objectField(*execution, "error");
  require(
      error != nullptr && std::get_if<std::string>(&error->value) != nullptr &&
           std::get_if<std::string>(&error->value)->find("is not registered") != std::string::npos,
       "bridge should not send a Skill to the executor");
}

void testLoader(const std::filesystem::path& directory) {
  Registry registry;
  Loader loader(registry);
  Discovery discovery(registry);
  SkillRegistry skillRegistry;
  SkillLoader skillLoader(skillRegistry);
  SkillDiscovery skillDiscovery(skillRegistry);

  const std::filesystem::path groupPath = directory / "group" / "group.json";
  std::filesystem::create_directories(groupPath.parent_path());
  writeManifest(groupPath, groupManifest("tests", "testes de capabilities"));
  require(loader.load(groupPath), "valid group manifest should load");

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
      "Discovery should expose only usable capabilities");
  require(
      discovery.discover({.query = "repete", .limit = std::nullopt}).size() == 1 &&
          discovery.discover({.query = "repete", .limit = std::nullopt}).front().id == "tests.echo",
      "loaded capability should appear in Discovery search");

  const std::filesystem::path localSkills = directory / ".atlas" / "skills";
  {
    SkillRegistry blockRegistry;
    SkillLoader blockLoader(blockRegistry);
    const auto scanRoot = directory / "block-skills";
    const auto blockPath = scanRoot / "valid" / "SKILL.md";
    std::filesystem::create_directories(scanRoot / "empty");
    std::filesystem::create_directories(blockPath.parent_path());
    writeManifest(blockPath,
        "---\nname: block\ndescription: >\n  Review code\n  and tests.\n---\nInstructions.\n");
    require(blockLoader.scan(scanRoot, SkillSource::project),
        "scan should ignore directories without SKILL.md and load folded descriptions");
    const auto block = blockRegistry.get("block");
    require(block.has_value() && block->summary == "Review code and tests.\n" &&
        block->instructions == "Instructions.\n", "folding must preserve the skill body");

    writeManifest(blockPath,
        "---\r\nname: block\r\ndescription: |-\r\n  First: line\r\n  Second line\r\n"
        "metadata: ignored\r\n---\r\nBody\r\n");
    require(blockLoader.reload("block"), "literal descriptions with CRLF should reload");
    require(blockRegistry.get("block")->summary == "First: line\nSecond line",
        "literal block should retain line breaks and strip the final newline");

    writeManifest(blockPath,
        "---\nname: block\ndescription: >-\n  First paragraph\n  continues\n\n"
        "  Second paragraph\n---\nBody");
    require(blockLoader.reload("block"), "folded paragraphs should reload");
    require(blockRegistry.get("block")->summary == "First paragraph continues\nSecond paragraph",
        "folding should preserve paragraph breaks");

    writeManifest(blockPath,
        "---\nname: block\ndescription: |+\n  First\n\n---\nBody");
    require(blockLoader.reload("block"), "keep chomping should reload");
    require(blockRegistry.get("block")->summary == "First\n\n",
        "keep chomping should preserve trailing newlines");

    writeManifest(blockPath, "---\nname: block\ndescription: >\n---\nBody");
    require(!blockLoader.reload("block"), "empty block descriptions should be rejected");
    writeManifest(blockPath,
        "---\nname: block\ndescription: >\n  First\ndescription: duplicate\n---\nBody");
    require(!blockLoader.reload("block"), "duplicate descriptions after a block should be rejected");
  }
  const std::filesystem::path localSkill = localSkills / "procedure" / "SKILL.md";
  std::filesystem::create_directories(localSkill.parent_path());
  writeManifest(
      localSkill,
      skillDocument("tests.procedure", "combina ferramentas", "Combine the tools in order."));
  require(skillLoader.load(localSkill, SkillSource::project), "valid SKILL.md should load");
  const auto loadedSkill = skillRegistry.get("tests.procedure");
  require(
      loadedSkill.has_value() &&
          loadedSkill->summary == "combina ferramentas" &&
          loadedSkill->instructions == "Combine the tools in order.",
      "skill frontmatter and instructions should be materialized");
  const auto discoveredSkill = skillDiscovery.getSkill("tests.procedure");
  require(
      discoveredSkill.has_value() && discoveredSkill->instructions == "Combine the tools in order.",
      "loaded skill should be available through skill discovery");

  const std::filesystem::path globalSkills = directory / "home" / ".config" / "atlas" / "skills";
  const std::filesystem::path agentSkills = directory / ".agents" / "skills";
  const std::filesystem::path globalSkill = globalSkills / "global" / "SKILL.md";
  const std::filesystem::path agentSkill = agentSkills / "agent" / "SKILL.md";
  const std::filesystem::path globalOverride = globalSkills / "procedure" / "SKILL.md";
  const std::filesystem::path agentOverride = agentSkills / "procedure" / "SKILL.md";
  std::filesystem::create_directories(globalSkill.parent_path());
  std::filesystem::create_directories(agentSkill.parent_path());
  std::filesystem::create_directories(globalOverride.parent_path());
  std::filesystem::create_directories(agentOverride.parent_path());
  writeManifest(globalSkill, skillDocument("tests.global", "skill global", "Global instructions."));
  writeManifest(agentSkill, skillDocument("tests.agent", "skill agent", "Agent instructions."));
  writeManifest(
      globalOverride,
      skillDocument("tests.procedure", "global override", "Global instructions."));
  writeManifest(
      agentOverride,
      skillDocument("tests.procedure", "agent override", "Agent instructions."));
  require(skillLoader.scan(globalSkills, SkillSource::global), "global skills should scan");
  require(skillLoader.scan(agentSkills, SkillSource::agents), "agent skills should scan");
  require(skillRegistry.get("tests.global").has_value(), "global skill should be registered");
  require(skillRegistry.get("tests.agent").has_value(), "agent skill should be registered");
  const auto precedence = skillRegistry.get("tests.procedure");
  require(
      precedence.has_value() && precedence->summary == "combina ferramentas" &&
          skillRegistry.sourceOf("tests.procedure") == SkillSource::project,
      "project Skills should have precedence over global and agent Skills");

  const std::filesystem::path auxiliary = localSkills / "procedure" / "references" / "guide.md";
  std::filesystem::create_directories(auxiliary.parent_path());
  writeManifest(auxiliary, "reference content");
  const auto materialized = skillDiscovery.getSkill("tests.procedure", "references/guide.md");
  require(
      materialized.has_value() && materialized->files.size() == 1 &&
          materialized->files.front().content == "reference content",
      "skill discovery should materialize an auxiliary file on demand");

  const auto catalog = discovery.listTools();
  require(
      catalog.size() == 1 && catalog[0].id == "tests.echo" && catalog[0].type == "tool",
      "listTools should return only tools from the Registry");
  const auto groupTools = discovery.listTools("tests");
  require(
      groupTools.size() == 1 && groupTools.front().id == "tests.echo" &&
          groupTools.front().group == "tests",
      "listTools should filter tools by group");

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
  require(loader.unload("tests"), "group unload should remove a loaded group");

  testBridge(directory);
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
