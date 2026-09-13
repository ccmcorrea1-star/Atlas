#pragma once

#include "../../core/execution.hpp"

#include <optional>
#include <string>
#include <string_view>

namespace atlas::capabilities::runtime::executable {

// Representa o contrato plano compartilhado por executaveis de capabilities.
struct RequestParseResult {
  std::optional<NativeRequest> request;
  std::string target;
  std::string error;
};

// Le target e preserva os demais campos como argumentos estruturados.
RequestParseResult parseRequest(std::string_view source);

// Converte o resultado estruturado para o objeto JSON do protocolo atual.
StructuredValue responseValue(const ExecutionResult& result);

}  // namespace atlas::capabilities::runtime::executable
