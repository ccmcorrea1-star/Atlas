#pragma once

#include "execution.hpp"

#include <cstdint>
#include <optional>
#include <stdexcept>
#include <string>
#include <string_view>
#include <vector>

namespace atlas::capabilities {

class ArgumentError : public std::runtime_error {
 public:
  explicit ArgumentError(const std::string& message) : std::runtime_error(message) {}
};

// Faz somente a conversao dos valores ja validados pelo Executor.
class ArgumentReader {
 public:
  explicit ArgumentReader(const StructuredArguments& arguments) : arguments_(arguments) {}

  std::string string(std::string_view name) const;
  std::optional<std::string> optionalString(std::string_view name) const;
  bool boolean(std::string_view name, bool fallback = false) const;
  std::int64_t integer(std::string_view name) const;
  std::optional<std::int64_t> optionalInteger(std::string_view name) const;
  std::vector<std::string> stringArray(std::string_view name) const;
  std::optional<std::vector<std::string>> optionalStringArray(std::string_view name) const;

 private:
  const StructuredValue* value(std::string_view name) const;
  [[noreturn]] static void missing(std::string_view name);
  [[noreturn]] static void wrongType(std::string_view name, std::string_view expected);

  const StructuredArguments& arguments_;
};

}  // namespace atlas::capabilities
