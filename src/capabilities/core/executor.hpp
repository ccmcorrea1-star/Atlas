#pragma once

#include "execution.hpp"
#include "registry.hpp"

#include <string>
#include <string_view>

namespace atlas::capabilities {

struct ExecutionRequest {
  std::string capability_id;
  std::string target;
  StructuredArguments arguments;
};

using ExecutorRequest = ExecutionRequest;

// Executa uma capability somente depois de resolve-la no Registry.
class Executor {
 public:
  explicit Executor(const Registry& registry) : registry_(registry) {}

  // Recebe a requisicao completa e devolve sempre o mesmo contrato de resultado.
  ExecutionResult execute(
      const ExecutionRequest& request,
      const ExecutionOutputCallback& on_output = {}) const;

  // Atalho para callers que ja possuem id, target e argumentos estruturados.
  ExecutionResult execute(
      std::string_view capability_id,
      std::string target,
      StructuredArguments arguments = {},
      const ExecutionOutputCallback& on_output = {}) const;

 private:
  static ExecutionResult failure(std::string target, std::string error);
  static ExecutionResult unavailable(std::string target, std::string kind);

  const Registry& registry_;
};

}  // namespace atlas::capabilities
