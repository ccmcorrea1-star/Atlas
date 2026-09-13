#pragma once

#include <chrono>
#include <optional>
#include <string>
#include <vector>

namespace atlas::capabilities::tools::process {

// O target permanece no contrato para futura resolucao pelo Device Fabric.
inline constexpr char kLocalTarget[] = "local";

enum class ExecStatus {
  success,
  failed,
  timed_out,
};

struct ExecRequest {
  // Somente o target local e executado nesta primeira implementacao.
  std::string target;

  // O programa e cada argumento seguem para execve sem interpretacao.
  std::string program;
  std::vector<std::string> args;

  // Quando informado, o filho troca para este diretorio antes do execve.
  std::optional<std::string> cwd;

  // Limite de tempo de parede; a ausencia do valor significa sem limite.
  std::optional<std::chrono::milliseconds> timeout;
};

struct ExecResult {
  // Mantem a identidade do target recebida na requisicao.
  std::string target;

  // Saidas capturadas separadamente dos descritores padrao do processo.
  std::string stdout;
  std::string stderr;

  // 127 indica falha antes da execucao; sinais usam 128 + numero do sinal.
  int exit_code{-1};

  // Duracao total da chamada, incluindo inicializacao e captura das saidas.
  std::chrono::milliseconds duration{0};

  // failed tambem representa um processo que terminou com exit_code diferente de zero.
  ExecStatus status{ExecStatus::failed};

  // Fica vazio em sucesso e descreve falhas de lancamento ou de execucao.
  std::string error;
};

// Executa um processo local diretamente, sem shell e sem interpretar os argumentos.
ExecResult exec(const ExecRequest& request);

// Converte o status para o valor estavel usado por adaptadores externos.
const char* statusName(ExecStatus status) noexcept;

}  // namespace atlas::capabilities::tools::process
