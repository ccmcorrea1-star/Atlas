#include "../../core/discovery.hpp"
#include "../../core/executor.hpp"
#include "../../core/loader.hpp"
#include "../../core/registry.hpp"
#include "../executable/protocol.hpp"

#include <cstdlib>
#include <filesystem>
#include <iostream>
#include <iterator>
#include <optional>
#include <stdexcept>
#include <string>
#include <string_view>

namespace {

using atlas::capabilities::Capability;
using atlas::capabilities::Discovery;
using atlas::capabilities::DiscoveryRequest;
using atlas::capabilities::Executor;
using atlas::capabilities::Loader;
using atlas::capabilities::Registry;
using atlas::capabilities::StructuredValue;

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

void loadRegistry(const char* executable, Registry& registry) {
  Loader loader(registry);
  const std::filesystem::path directory = capabilitiesDirectory(executable) / "tools";
  if (!loader.scan(directory)) {
    throw std::runtime_error(loader.lastError());
  }
}

StructuredValue discoveryValue(const Discovery& discovery, const StructuredValue::Object& request) {
  DiscoveryRequest discoveryRequest;
  discoveryRequest.path = optionalString(request, "path");
  discoveryRequest.query = optionalString(request, "query");

  StructuredValue::Array results;
  for (const auto& result : discovery.discover(discoveryRequest)) {
    results.emplace_back(StructuredValue::Object{
        {"id", result.id},
        {"type", result.type},
        {"summary", result.summary},
    });
  }
  return StructuredValue(StructuredValue::Object{{"results", std::move(results)}});
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

StructuredValue getDefinitionValue(
    const Registry& registry,
    const StructuredValue::Object& request) {
  const std::string id = requiredString(request, "id");
  const auto definition = registry.getDefinition(id);
  return StructuredValue(StructuredValue::Object{
      {"definition", definition.has_value() ? definitionValue(definition.value()) : StructuredValue(nullptr)},
  });
}

StructuredValue executeValue(
    const Registry& registry,
    const StructuredValue::Object& request) {
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
      ? [](std::string_view channel, std::string_view delta) {
          std::cout << atlas::capabilities::serializeJson(StructuredValue(StructuredValue::Object{
              {"event", "execution.output.delta"},
              {"channel", std::string(channel)},
              {"delta", std::string(delta)},
          })) << '\n' << std::flush;
        }
      : atlas::capabilities::ExecutionOutputCallback{};
  return atlas::capabilities::runtime::executable::responseValue(
      Executor(registry).execute(id, target, std::move(arguments), on_output));
}

int failure(std::string message) {
  std::cout << atlas::capabilities::serializeJson(
                   StructuredValue(StructuredValue::Object{{"error", std::move(message)}}))
            << '\n';
  return 1;
}

}  // namespace

int main(int argc, char* argv[]) {
  if (argc == 0 || argv[0] == nullptr) {
    return failure("capability bridge executable path is unavailable");
  }

  try {
    const std::string input{
        std::istreambuf_iterator<char>(std::cin),
        std::istreambuf_iterator<char>()};
    std::string parseError;
    const auto parsed = atlas::capabilities::parseJson(input, parseError);
    if (!parsed.has_value()) {
      return failure("invalid JSON request: " + parseError);
    }
    const auto* request = std::get_if<StructuredValue::Object>(&parsed->value);
    if (request == nullptr) {
      return failure("request must be a JSON object");
    }

    Registry registry;
    loadRegistry(argv[0], registry);
    const std::string operation = requiredString(*request, "operation");
    StructuredValue response;
    if (operation == "discover") {
      response = discoveryValue(Discovery(registry), *request);
    } else if (operation == "get_definition") {
      response = getDefinitionValue(registry, *request);
    } else if (operation == "execute") {
      response = executeValue(registry, *request);
    } else {
      return failure("unknown capability bridge operation '" + operation + "'");
    }

    std::cout << atlas::capabilities::serializeJson(response) << '\n';
    return 0;
  } catch (const std::exception& exception) {
    return failure(exception.what());
  }
}
