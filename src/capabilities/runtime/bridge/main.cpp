#include "../../core/discovery.hpp"
#include "../../core/executor.hpp"
#include "../../core/loader.hpp"
#include "../../core/registry.hpp"
#include "../executable/protocol.hpp"
#include "../../../skills/discovery.hpp"
#include "../../../skills/loader.hpp"
#include "../../../skills/registry.hpp"
#include "../../../search.hpp"

#include <algorithm>
#include <cstddef>
#include <cstdint>
#include <cstdlib>
#include <filesystem>
#include <iostream>
#include <limits>
#include <optional>
#include <stdexcept>
#include <string>
#include <string_view>
#include <system_error>
#include <utility>
#include <vector>

namespace {

using atlas::capabilities::Capability;
using atlas::capabilities::DiscoveryRequest;
using atlas::capabilities::ToolDiscovery;
using atlas::capabilities::Executor;
using atlas::capabilities::Loader;
using atlas::capabilities::Registry;
using atlas::capabilities::StructuredValue;
using atlas::capabilities::ToolListResult;
using atlas::skills::Skill;
using atlas::skills::SkillDiscovery;
using atlas::skills::SkillLoader;
using atlas::skills::SkillRegistry;
using atlas::skills::SkillSource;

class RuntimeDiscovery {
 public:
  RuntimeDiscovery(const Registry& registry, const SkillRegistry& skills)
      : tools_(registry), skills_(skills) {}

  std::vector<atlas::capabilities::DiscoveryResult> discover(
      const DiscoveryRequest& request) const {
    const atlas::search::Query query(request.query.value_or(""));
    struct RankedResult {
      atlas::capabilities::DiscoveryResult result;
      atlas::search::Match match;
    };
    std::vector<RankedResult> ranked;
    for (const auto& tool : tools_.discover()) {
      const auto definition = tools_.getDefinition(tool.id);
      if (!definition.has_value()) {
        continue;
      }
      const auto match = query.match(
          {tool.id, tool.summary, definition->aliases, definition->description});
      if (match.score >= 0) {
        ranked.push_back({tool, match});
      }
    }
    for (const auto& skill : skills_.discover()) {
      const auto match = query.match({skill.id, skill.summary});
      if (match.score >= 0) {
        ranked.push_back({{skill.id, skill.type, skill.summary}, match});
      }
    }
    // A reserva parcial só vale quando nenhum dos dois catálogos cobre a consulta.
    if (std::any_of(ranked.begin(), ranked.end(), [](const RankedResult& entry) {
          return entry.match.full;
        })) {
      std::erase_if(ranked, [](const RankedResult& entry) { return !entry.match.full; });
    }
    std::sort(ranked.begin(), ranked.end(), [](const RankedResult& left, const RankedResult& right) {
      if (left.match.score != right.match.score) {
        return left.match.score > right.match.score;
      }
      if (left.result.id != right.result.id) {
        return left.result.id < right.result.id;
      }
      return left.result.type < right.result.type;
    });
    std::vector<atlas::capabilities::DiscoveryResult> result;
    for (auto& entry : ranked) {
      result.push_back(std::move(entry.result));
    }
    if (request.limit.has_value() && result.size() > request.limit.value()) {
      result.resize(request.limit.value());
    }
    return result;
  }

  std::vector<ToolListResult> listTools(std::optional<std::string_view> group) const {
    return tools_.listTools(group);
  }

  std::optional<Capability> getDefinition(std::string_view id) const {
    return tools_.getDefinition(id);
  }

  std::optional<Skill> getSkill(
      std::string_view id,
      std::optional<std::string_view> path = std::nullopt) const {
    return skills_.getSkill(id, path);
  }

 private:
  ToolDiscovery tools_;
  SkillDiscovery skills_;
};

const StructuredValue* field(
    const StructuredValue::Object& object,
    std::string_view name) {
  const auto iterator = object.find(name);
  return iterator == object.end() ? nullptr : &iterator->second;
}

std::string requiredString(
    const StructuredValue::Object& object,
    std::string_view name) {
  const StructuredValue* value = field(object, name);
  const auto* string = value == nullptr ? nullptr : std::get_if<std::string>(&value->value);
  if (string == nullptr || string->empty()) {
    throw std::runtime_error("field '" + std::string(name) + "' must be a non-empty string");
  }
  return *string;
}

std::optional<std::string> optionalString(
    const StructuredValue::Object& object,
    std::string_view name) {
  const StructuredValue* value = field(object, name);
  if (value == nullptr) {
    return std::nullopt;
  }
  const auto* string = std::get_if<std::string>(&value->value);
  if (string == nullptr) {
    throw std::runtime_error("field '" + std::string(name) + "' must be a string");
  }
  return *string;
}

bool optionalBoolean(
    const StructuredValue::Object& object,
    std::string_view name,
    bool fallback) {
  const StructuredValue* value = field(object, name);
  if (value == nullptr) {
    return fallback;
  }
  const auto* boolean = std::get_if<bool>(&value->value);
  if (boolean == nullptr) {
    throw std::runtime_error("field '" + std::string(name) + "' must be a boolean");
  }
  return *boolean;
}

std::optional<std::size_t> optionalLimit(
    const StructuredValue::Object& object,
    std::string_view name) {
  const StructuredValue* value = field(object, name);
  if (value == nullptr) {
    return std::nullopt;
  }
  const auto* integer = std::get_if<std::int64_t>(&value->value);
  if (integer == nullptr || *integer < 0 ||
      static_cast<std::uint64_t>(*integer) > std::numeric_limits<std::size_t>::max()) {
    throw std::runtime_error("field '" + std::string(name) + "' must be a non-negative integer");
  }
  return static_cast<std::size_t>(*integer);
}

std::filesystem::path capabilitiesDirectory(const char* executable) {
  if (const char* configured = std::getenv("ATLAS_CAPABILITIES_DIR"); configured != nullptr && *configured != '\0') {
    return std::filesystem::path(configured);
  }

  std::error_code error;
  std::filesystem::path executablePath = std::filesystem::absolute(executable, error);
  if (error) {
    throw std::runtime_error("cannot resolve capability bridge executable path");
  }
  executablePath = std::filesystem::weakly_canonical(executablePath, error);
  if (error) {
    throw std::runtime_error("cannot canonicalize capability bridge executable path");
  }
  return executablePath.parent_path().parent_path().parent_path();
}

void loadRegistries(const char* executable, Registry& registry, SkillRegistry& skills) {
  Loader loader(registry);
  const std::filesystem::path directory = capabilitiesDirectory(executable) / "tools";
  if (!loader.scan(directory)) {
    throw std::runtime_error(loader.lastError());
  }

  const std::filesystem::path project = std::filesystem::current_path();
  SkillLoader skillLoader(skills);
  const auto scanSkills = [&skillLoader](
                              const std::filesystem::path& root,
                              SkillSource source) {
    std::error_code error;
    if (!std::filesystem::is_directory(root, error)) {
      if (error && error != std::errc::no_such_file_or_directory) {
        throw std::runtime_error("cannot inspect skills directory '" + root.string() + "': " + error.message());
      }
      return;
    }
    if (!skillLoader.scan(root, source)) {
      throw std::runtime_error(skillLoader.lastError());
    }
  };

  scanSkills(project / ".atlas" / "skills", SkillSource::project);
  if (const char* home = std::getenv("HOME"); home != nullptr && *home != '\0') {
    scanSkills(std::filesystem::path(home) / ".config" / "atlas" / "skills", SkillSource::global);
  }
  scanSkills(project / ".agents" / "skills", SkillSource::agents);
}

void writeResponse(const StructuredValue::Object& response) {
  std::cout << atlas::capabilities::serializeJson(StructuredValue(response)) << '\n' << std::flush;
}

void writeError(const std::optional<std::string>& requestId, std::string message) {
  StructuredValue::Object response{{"error", std::move(message)}};
  if (requestId.has_value()) {
    response["request_id"] = requestId.value();
  }
  writeResponse(response);
}

int failure(std::string message) {
  writeError(std::nullopt, std::move(message));
  return 1;
}

StructuredValue::Object discoveryValue(
    const RuntimeDiscovery& discovery,
    const StructuredValue::Object& request) {
  DiscoveryRequest discoveryRequest;
  discoveryRequest.query = optionalString(request, "query");
  discoveryRequest.limit = optionalLimit(request, "limit");

  StructuredValue::Array results;
  for (const auto& result : discovery.discover(discoveryRequest)) {
    results.emplace_back(StructuredValue::Object{
        {"id", result.id},
        {"type", result.type},
        {"summary", result.summary},
    });
  }
  return StructuredValue::Object{{"results", std::move(results)}};
}

StructuredValue::Object listToolsValue(
    const RuntimeDiscovery& discovery,
    const StructuredValue::Object& request) {
  const std::optional<std::string> group = optionalString(request, "group");
  if (group.has_value() && group->empty()) {
    throw std::runtime_error("field 'group' must be a non-empty string");
  }

  StructuredValue::Array tools;
  for (const ToolListResult& result : discovery.listTools(
           group.has_value() ? std::optional<std::string_view>(*group) : std::nullopt)) {
    StructuredValue::Object tool{
        {"id", result.id},
        {"type", result.type},
        {"summary", result.summary},
    };
    if (result.group.has_value()) {
      tool["group"] = result.group.value();
    }
    tools.emplace_back(std::move(tool));
  }
  return StructuredValue::Object{{"tools", std::move(tools)}};
}

StructuredValue definitionValue(const Capability& capability) {
  StructuredValue::Object definition{
      {"id", capability.id},
      {"type", capability.type},
      {"summary", capability.summary},
      {"description", capability.description},
      {"schema", capability.schema},
  };
  if (capability.parent.has_value()) {
    definition["parent"] = capability.parent.value();
  }
  return StructuredValue(std::move(definition));
}

StructuredValue::Object getDefinitionValue(
    const RuntimeDiscovery& discovery,
    const StructuredValue::Object& request) {
  const std::string id = requiredString(request, "id");
  const auto definition = discovery.getDefinition(id);
  return StructuredValue::Object{
      {"definition", definition.has_value() ? definitionValue(definition.value()) : StructuredValue(nullptr)},
  };
}

StructuredValue::Object getSkillValue(
    const RuntimeDiscovery& discovery,
    const StructuredValue::Object& request) {
  const std::string id = requiredString(request, "id");
  const auto path = optionalString(request, "path");
  const auto skill = discovery.getSkill(
      id,
      path.has_value() ? std::optional<std::string_view>(*path) : std::nullopt);
  if (!skill.has_value()) {
    return StructuredValue::Object{{"skill", StructuredValue(nullptr)}};
  }
  return StructuredValue::Object{
      {"skill", StructuredValue::Object{
           {"id", skill->id},
           {"type", "skill"},
           {"summary", skill->summary},
           {"instructions", skill->instructions},
           {"source", skill->source.string()},
           {"files", [&skill] {
             StructuredValue::Array files;
             for (const auto& file : skill->files) {
               files.emplace_back(StructuredValue::Object{
                   {"path", file.path},
                   {"content", file.content},
               });
             }
             return StructuredValue(std::move(files));
           }()},
       }},
  };
}

StructuredValue::Object executeValue(
    const Registry& registry,
    const Executor& executor,
    const StructuredValue::Object& request,
    const std::optional<std::string>& requestId) {
  const std::string id = requiredString(request, "id");
  const std::string target = requiredString(request, "target");
  StructuredValue::Object arguments;
  if (const StructuredValue* value = field(request, "arguments"); value != nullptr) {
    const auto* object = std::get_if<StructuredValue::Object>(&value->value);
    if (object == nullptr) {
      throw std::runtime_error("field 'arguments' must be an object");
    }
    arguments = *object;
  }

  const auto definition = registry.getDefinition(id);
  if (!definition.has_value()) {
    throw std::runtime_error("capability '" + id + "' is not registered");
  }
  const atlas::capabilities::ExecutionOutputCallback on_output =
      optionalBoolean(request, "stream", false)
      ? [&requestId](std::string_view channel, std::string_view delta) {
          StructuredValue::Object event{
              {"event", "execution.output.delta"},
              {"channel", std::string(channel)},
              {"delta", std::string(delta)},
          };
          if (requestId.has_value()) {
            event["request_id"] = requestId.value();
          }
          std::cout << atlas::capabilities::serializeJson(StructuredValue(std::move(event)))
                    << '\n' << std::flush;
        }
      : atlas::capabilities::ExecutionOutputCallback{};
  StructuredValue result = atlas::capabilities::runtime::executable::responseValue(
      executor.execute(id, target, std::move(arguments), on_output));
  return std::get<StructuredValue::Object>(std::move(result.value));
}

void handleLine(
    const Registry& registry,
    const Executor& executor,
    const RuntimeDiscovery& discovery,
    const std::string& line) {
  std::optional<std::string> requestId;
  try {
    std::string parseError;
    const auto parsed = atlas::capabilities::parseJson(line, parseError);
    if (!parsed.has_value()) {
      writeError(std::nullopt, "invalid JSON request: " + parseError);
      return;
    }
    const auto* request = std::get_if<StructuredValue::Object>(&parsed->value);
    if (request == nullptr) {
      writeError(std::nullopt, "request must be a JSON object");
      return;
    }

    requestId = optionalString(*request, "request_id");
    const std::string operation = requiredString(*request, "operation");
    StructuredValue::Object response;
    if (operation == "list_tools") {
      response = listToolsValue(discovery, *request);
    } else if (operation == "discover") {
      response = discoveryValue(discovery, *request);
    } else if (operation == "get_definition") {
      response = getDefinitionValue(discovery, *request);
    } else if (operation == "get_skill") {
      response = getSkillValue(discovery, *request);
    } else if (operation == "execute") {
      response = executeValue(registry, executor, *request, requestId);
    } else {
      throw std::runtime_error("unknown capability bridge operation '" + operation + "'");
    }

    if (requestId.has_value()) {
      response["request_id"] = requestId.value();
    }
    writeResponse(response);
  } catch (const std::exception& exception) {
    writeError(requestId, exception.what());
  }
}

}  // namespace

int main(int argc, char* argv[]) {
  if (argc == 0 || argv[0] == nullptr) {
    return failure("capability bridge executable path is unavailable");
  }

  Registry registry;
  SkillRegistry skills;
  try {
    loadRegistries(argv[0], registry, skills);
  } catch (const std::exception& exception) {
    return failure(exception.what());
  }

  const Executor executor(registry);
  const RuntimeDiscovery discovery(registry, skills);

  std::string line;
  while (std::getline(std::cin, line)) {
    if (!line.empty() && line.back() == '\r') {
      line.pop_back();
    }
    if (line.empty()) {
      continue;
    }
    handleLine(registry, executor, discovery, line);
  }

  return 0;
}
