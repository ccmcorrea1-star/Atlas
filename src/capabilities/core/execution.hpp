#pragma once

#include <cstdint>
#include <functional>
#include <map>
#include <optional>
#include <string>
#include <string_view>
#include <utility>
#include <variant>
#include <vector>

namespace atlas::capabilities {

// Representa argumentos e saidas sem acoplar o executor a um formato externo.
struct StructuredValue {
  using Array = std::vector<StructuredValue>;
  using Object = std::map<std::string, StructuredValue, std::less<>>;

  StructuredValue() : value(nullptr) {}
  StructuredValue(std::nullptr_t) : value(nullptr) {}
  StructuredValue(bool value) : value(value) {}
  StructuredValue(int value) : value(static_cast<std::int64_t>(value)) {}
  StructuredValue(std::int64_t value) : value(value) {}
  StructuredValue(double value) : value(value) {}
  StructuredValue(const char* value) : value(std::string(value == nullptr ? "" : value)) {}
  StructuredValue(std::string value) : value(std::move(value)) {}
  StructuredValue(std::string_view value) : value(std::string(value)) {}
  StructuredValue(Array value) : value(std::move(value)) {}
  StructuredValue(Object value) : value(std::move(value)) {}

  std::variant<std::nullptr_t, bool, std::int64_t, double, std::string, Array, Object> value;
};

using StructuredArguments = StructuredValue::Object;

// Mantem o target explicito para toda implementacao nativa registrada.
struct NativeRequest {
  std::string target;
  StructuredArguments arguments;
};

enum class ExecutionStatus {
  success,
  failed,
  timed_out,
  unavailable,
};

struct ExecutionResult {
  std::string target;
  ExecutionStatus status{ExecutionStatus::failed};
  StructuredValue output;
  std::string error;
};

using NativeEntrypoint = std::function<ExecutionResult(const NativeRequest&)>;

const char* executionStatusName(ExecutionStatus status) noexcept;

// Codifica e decodifica o contrato JSON usado por implementacoes executaveis.
std::string serializeJson(const StructuredValue& value);
std::optional<StructuredValue> parseJson(std::string_view source, std::string& error);

}  // namespace atlas::capabilities
