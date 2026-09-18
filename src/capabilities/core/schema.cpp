#include "schema.hpp"

#include <cmath>
#include <cstdint>
#include <string>

namespace atlas::capabilities {
namespace {

const StructuredValue* member(const StructuredValue::Object& object, std::string_view name) {
  const auto iterator = object.find(name);
  return iterator == object.end() ? nullptr : &iterator->second;
}

std::string joinPath(std::string path, std::string_view name) {
  if (path.empty()) {
    return std::string(name);
  }
  return path + "." + std::string(name);
}

std::string displayPath(std::string_view path) {
  return path.empty() ? "arguments" : "arguments." + std::string(path);
}

// Tipo do schema entra em mensagem de erro: "an array" e "an object" exigem
// artigo diferente dos tipos que comecam com consoante.
std::string typePhrase(std::string_view type) {
  const bool vowel = type == "array" || type == "object";
  return std::string(vowel ? "an " : "a ") + std::string(type);
}

bool isNumber(const StructuredValue& value, double& number) {
  if (const auto* integer = std::get_if<std::int64_t>(&value.value); integer != nullptr) {
    number = static_cast<double>(*integer);
    return true;
  }
  if (const auto* decimal = std::get_if<double>(&value.value); decimal != nullptr && std::isfinite(*decimal)) {
    number = *decimal;
    return true;
  }
  return false;
}

bool typeMatches(const StructuredValue& value, std::string_view type) {
  if (type == "object") {
    return std::holds_alternative<StructuredValue::Object>(value.value);
  }
  if (type == "array") {
    return std::holds_alternative<StructuredValue::Array>(value.value);
  }
  if (type == "string") {
    return std::holds_alternative<std::string>(value.value);
  }
  if (type == "boolean") {
    return std::holds_alternative<bool>(value.value);
  }
  if (type == "integer") {
    return std::holds_alternative<std::int64_t>(value.value);
  }
  if (type == "number") {
    double ignored = 0.0;
    return isNumber(value, ignored);
  }
  if (type == "null") {
    return std::holds_alternative<std::nullptr_t>(value.value);
  }
  return false;
}

std::optional<std::string> validateValue(
    const StructuredValue& value,
    const StructuredValue& schema,
    std::string path);

std::optional<std::string> validateObject(
    const StructuredValue::Object& value,
    const StructuredValue::Object& schema,
    const std::string& path) {
  if (const StructuredValue* requiredValue = member(schema, "required"); requiredValue != nullptr) {
    const auto* required = std::get_if<StructuredValue::Array>(&requiredValue->value);
    if (required == nullptr) {
      return "schema required must be an array";
    }
    for (const StructuredValue& item : *required) {
      const auto* name = std::get_if<std::string>(&item.value);
      if (name == nullptr) {
        return "schema required entries must be strings";
      }
      if (value.find(*name) == value.end()) {
        return displayPath(joinPath(path, *name)) + " is required";
      }
    }
  }

  const StructuredValue::Object* properties = nullptr;
  if (const StructuredValue* propertiesValue = member(schema, "properties"); propertiesValue != nullptr) {
    properties = std::get_if<StructuredValue::Object>(&propertiesValue->value);
    if (properties == nullptr) {
      return "schema properties must be an object";
    }
  }

  bool additionalProperties = true;
  if (const StructuredValue* additional = member(schema, "additionalProperties"); additional != nullptr) {
    const auto* boolean = std::get_if<bool>(&additional->value);
    if (boolean == nullptr) {
      return "schema additionalProperties must be a boolean";
    }
    additionalProperties = *boolean;
  }

  for (const auto& [name, item] : value) {
    const StructuredValue* propertySchema = properties == nullptr ? nullptr : member(*properties, name);
    if (propertySchema == nullptr) {
      if (!additionalProperties) {
        return displayPath(joinPath(path, name)) + " is not allowed";
      }
      continue;
    }
    if (const auto error = validateValue(item, *propertySchema, joinPath(path, name)); error.has_value()) {
      return error;
    }
  }
  return std::nullopt;
}

std::optional<std::string> validateArray(
    const StructuredValue::Array& value,
    const StructuredValue::Object& schema,
    const std::string& path) {
  if (const StructuredValue* itemsValue = member(schema, "items"); itemsValue != nullptr) {
    for (std::size_t index = 0; index < value.size(); ++index) {
      if (const auto error = validateValue(
              value[index], *itemsValue, path + "[" + std::to_string(index) + "]");
          error.has_value()) {
        return error;
      }
    }
  }
  return std::nullopt;
}

std::optional<std::string> validateBounds(
    const StructuredValue& value,
    const StructuredValue::Object& schema,
    const std::string& path) {
  double number = 0.0;
  if (!isNumber(value, number)) {
    return std::nullopt;
  }
  for (const std::string_view name : {"minimum", "maximum"}) {
    const StructuredValue* boundValue = member(schema, name);
    if (boundValue == nullptr) {
      continue;
    }
    double bound = 0.0;
    if (!isNumber(*boundValue, bound)) {
      return "schema " + std::string(name) + " must be a number";
    }
    if ((name == "minimum" && number < bound) || (name == "maximum" && number > bound)) {
      return displayPath(path) + " violates " + std::string(name);
    }
  }
  return std::nullopt;
}

std::optional<std::string> validateValue(
    const StructuredValue& value,
    const StructuredValue& schema,
    std::string path) {
  const auto* object = std::get_if<StructuredValue::Object>(&schema.value);
  if (object == nullptr) {
    return "schema at " + displayPath(path) + " must be an object";
  }

  if (const StructuredValue* typeValue = member(*object, "type"); typeValue != nullptr) {
    const auto* type = std::get_if<std::string>(&typeValue->value);
    if (type == nullptr) {
      return "schema type at " + displayPath(path) + " must be a string";
    }
    if (!typeMatches(value, *type)) {
      return displayPath(path) + " must be " + typePhrase(*type);
    }
  }

  if (const auto error = validateBounds(value, *object, path); error.has_value()) {
    return error;
  }
  if (const auto* objectValue = std::get_if<StructuredValue::Object>(&value.value);
      objectValue != nullptr) {
    if (const auto error = validateObject(*objectValue, *object, path); error.has_value()) {
      return error;
    }
  }
  if (const auto* arrayValue = std::get_if<StructuredValue::Array>(&value.value);
      arrayValue != nullptr) {
    if (const auto error = validateArray(*arrayValue, *object, path); error.has_value()) {
      return error;
    }
  }
  return std::nullopt;
}

}  // namespace

std::optional<std::string> validateArguments(
    const StructuredArguments& arguments,
    const StructuredValue& schema) {
  if (std::holds_alternative<std::nullptr_t>(schema.value)) {
    return std::nullopt;
  }
  return validateValue(StructuredValue(arguments), schema, {});
}

}  // namespace atlas::capabilities
