#pragma once

#include "../../../core/execution.hpp"

#include <chrono>
#include <optional>
#include <string>

namespace atlas::capabilities::tools::shell {

// O shell e resolvido no PATH em tempo de execucao; o contrato nao nomeia nenhum shell.
inline constexpr char kShellProgram[] = "sh";

// O target permanece no contrato para futura resolucao pelo Device Fabric.
inline constexpr char kLocalTarget[] = "local";

enum class ShellStatus {
  // O comando iniciou e terminou com codigo zero.
  success,

  // Houve falha de lancamento ou o comando terminou com codigo nao zero.
  failed,

  // O limite foi atingido e o comando foi encerrado pelo Atlas.
  timed_out,
};

struct ShellRequest {
  // Somente o target local e executado nesta primeira implementacao.
  std::string target;

  // Comando interpretado pelo shell, com pipes, redirecionamentos e expansoes.
  std::string command;

  // Quando informado, o shell troca para este diretorio antes de executar.
  std::optional<std::string> cwd;

  // Limite de tempo de parede; a ausencia do valor significa sem limite.
  std::optional<std::chrono::milliseconds> timeout;
};

struct ShellResult {
  // Mantem a identidade do target recebida na requisicao.
  std::string target;

  // Saidas capturadas separadamente dos descritores padrao do comando.
  std::string stdout;
  std::string stderr;

  // 127 indica falha antes da execucao; sinais usam 128 + numero do sinal.
  int exit_code{-1};

  // Duracao total da chamada, incluindo inicializacao e captura das saidas.
  std::chrono::milliseconds duration{0};

  // failed tambem representa um comando que terminou com exit_code diferente de zero.
  ShellStatus status{ShellStatus::failed};

  // Fica vazio em sucesso e descreve falhas de lancamento ou de execucao.
  std::string error;
};

// Executa um comando com semantica de shell, sem substituir process.exec.
ShellResult exec(
    const ShellRequest& request,
    const atlas::capabilities::ExecutionOutputCallback& on_output = {});

// Adapta os argumentos estruturados do runtime para a execucao deste comando.
atlas::capabilities::ExecutionResult dispatch(
    const atlas::capabilities::NativeRequest& request,
    const atlas::capabilities::ExecutionOutputCallback& on_output = {});

// Converte o status para o valor estavel usado por adaptadores externos.
const char* statusName(ShellStatus status) noexcept;

}  // namespace atlas::capabilities::tools::shell
