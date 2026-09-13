#include "exec.hpp"

#include "../../../core/execution.hpp"

#include <chrono>
#include <cstdint>
#include <iostream>
#include <iterator>
#include <string_view>
#include <utility>

namespace atlas::capabilities::tools::process {
namespace {

using atlas::capabilities::StructuredValue;

const StructuredValue* field(const StructuredValue::Object& object, std::string_view name) {
  const auto iterator = object.find(name);
  return iterator == object.end() ? nullptr : &iterator->second;
}

ExecResult requestFailure(std::string target, std::string error) {
  ExecResult result;
  result.target = std::move(target);
  result.error = std::move(error);
  return result;
}

ExecResult readRequest(std::string_view input) {
  // O manifesto define um contrato plano para manter target e opcoes explicitos.
  std::string parseError;
  const auto parsed = atlas::capabilities::parseJson(input, parseError);
  if (!parsed.has_value()) {
    return requestFailure({}, "invalid JSON request: " + parseError);
  }

  const auto* object = std::get_if<StructuredValue::Object>(&parsed->value);
  if (object == nullptr) {
    return requestFailure({}, "request must be a JSON object");
  }

  const StructuredValue* targetValue = field(*object, "target");
  const auto* target = targetValue == nullptr
      ? nullptr
      : std::get_if<std::string>(&targetValue->value);
  if (target == nullptr) {
    return requestFailure({}, "field 'target' must be a string");
  }

  const StructuredValue* programValue = field(*object, "program");
  const auto* program = programValue == nullptr
      ? nullptr
      : std::get_if<std::string>(&programValue->value);
  if (program == nullptr || program->empty()) {
    return requestFailure(*target, "field 'program' must be a non-empty string");
  }

  ExecRequest request;
  request.target = *target;
  request.program = *program;

  if (const StructuredValue* argsValue = field(*object, "args"); argsValue != nullptr) {
    const auto* args = std::get_if<StructuredValue::Array>(&argsValue->value);
    if (args == nullptr) {
      return requestFailure(*target, "field 'args' must be an array of strings");
    }
    request.args.reserve(args->size());
    for (const StructuredValue& value : *args) {
      const auto* argument = std::get_if<std::string>(&value.value);
      if (argument == nullptr) {
        return requestFailure(*target, "field 'args' must be an array of strings");
      }
      request.args.push_back(*argument);
    }
  }

  if (const StructuredValue* cwdValue = field(*object, "cwd"); cwdValue != nullptr) {
    const auto* cwd = std::get_if<std::string>(&cwdValue->value);
    if (cwd == nullptr) {
      return requestFailure(*target, "field 'cwd' must be a string");
    }
    request.cwd = *cwd;
  }

  const StructuredValue* timeoutValue = field(*object, "timeout_ms");
  if (timeoutValue == nullptr) {
    timeoutValue = field(*object, "timeout");
  }
  if (timeoutValue != nullptr) {
    const auto* timeout = std::get_if<std::int64_t>(&timeoutValue->value);
    if (timeout == nullptr || *timeout < 0) {
      return requestFailure(*target, "field 'timeout_ms' must be a non-negative integer");
    }
    request.timeout = std::chrono::milliseconds(*timeout);
  }

  return exec(request);
}

StructuredValue resultValue(const ExecResult& result) {
  return StructuredValue::Object{
      {"target", result.target},
      {"stdout", result.stdout},
      {"stderr", result.stderr},
      {"exit_code", result.exit_code},
      {"duration_ms", static_cast<std::int64_t>(result.duration.count())},
      {"status", statusName(result.status)},
      {"error", result.error},
  };
}

}  // namespace

int implementationMain() {
  // A implementacao troca uma requisicao e uma resposta JSON por chamada.
  const std::string input{
      std::istreambuf_iterator<char>(std::cin),
      std::istreambuf_iterator<char>()};
  const ExecResult result = readRequest(input);
  std::cout << atlas::capabilities::serializeJson(resultValue(result)) << '\n';
  return 0;
}

}  // namespace atlas::capabilities::tools::process

int main() {
  return atlas::capabilities::tools::process::implementationMain();
}
