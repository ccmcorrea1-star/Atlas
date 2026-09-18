#include "arguments.hpp"

#include <utility>

namespace atlas::capabilities {

const StructuredValue* ArgumentReader::value(std::string_view name) const {
  const auto iterator = arguments_.find(name);
  return iterator == arguments_.end() ? nullptr : &iterator->second;
}

void ArgumentReader::missing(std::string_view name) {
  throw ArgumentError("field '" + std::string(name) + "' is required");
}

void ArgumentReader::wrongType(std::string_view name, std::string_view expected) {
  throw ArgumentError("field '" + std::string(name) + "' must be " + std::string(expected));
}

std::string ArgumentReader::string(std::string_view name) const {
  const StructuredValue* current = value(name);
  if (current == nullptr) {
    missing(name);
  }
  const auto* result = std::get_if<std::string>(&current->value);
  if (result == nullptr) {
    wrongType(name, "a string");
  }
  return *result;
}

std::optional<std::string> ArgumentReader::optionalString(std::string_view name) const {
  const StructuredValue* current = value(name);
  if (current == nullptr) {
    return std::nullopt;
  }
  const auto* result = std::get_if<std::string>(&current->value);
  if (result == nullptr) {
    wrongType(name, "a string");
  }
  return *result;
}

bool ArgumentReader::boolean(std::string_view name, bool fallback) const {
  const StructuredValue* current = value(name);
  if (current == nullptr) {
    return fallback;
  }
  const auto* result = std::get_if<bool>(&current->value);
  if (result == nullptr) {
    wrongType(name, "a boolean");
  }
  return *result;
}

std::int64_t ArgumentReader::integer(std::string_view name) const {
  const StructuredValue* current = value(name);
  if (current == nullptr) {
    missing(name);
  }
  const auto* result = std::get_if<std::int64_t>(&current->value);
  if (result == nullptr) {
    wrongType(name, "an integer");
  }
  return *result;
}

std::optional<std::int64_t> ArgumentReader::optionalInteger(std::string_view name) const {
  const StructuredValue* current = value(name);
  if (current == nullptr) {
    return std::nullopt;
  }
  const auto* result = std::get_if<std::int64_t>(&current->value);
  if (result == nullptr) {
    wrongType(name, "an integer");
  }
  return *result;
}

std::vector<std::string> ArgumentReader::stringArray(std::string_view name) const {
  const StructuredValue* current = value(name);
  if (current == nullptr) {
    missing(name);
  }
  const auto* array = std::get_if<StructuredValue::Array>(&current->value);
  if (array == nullptr) {
    wrongType(name, "an array of strings");
  }
  std::vector<std::string> result;
  result.reserve(array->size());
  for (const StructuredValue& item : *array) {
    const auto* string = std::get_if<std::string>(&item.value);
    if (string == nullptr) {
      wrongType(name, "an array of strings");
    }
    result.push_back(*string);
  }
  return result;
}

std::optional<std::vector<std::string>> ArgumentReader::optionalStringArray(
    std::string_view name) const {
  if (value(name) == nullptr) {
    return std::nullopt;
  }
  return stringArray(name);
}

}  // namespace atlas::capabilities
