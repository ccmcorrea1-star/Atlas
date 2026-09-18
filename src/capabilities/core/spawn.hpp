#pragma once

#include "execution.hpp"

#include <chrono>
#include <optional>
#include <string>
#include <vector>

namespace atlas::capabilities {

// Resultado do lancamento e acompanhamento de um processo filho com saidas capturadas.
enum class SpawnStatus {
  // O processo iniciou e terminou com codigo zero.
  success,

  // Falha de lancamento ou termino com codigo diferente de zero.
  failed,

  // O limite foi atingido e o processo foi encerrado.
  timed_out,
};

enum class SpawnErrorKind {
  none,
  other,
  executable_not_found,
};

struct SpawnRequest {
  // Programa executado com busca no PATH quando nao contem barra.
  std::string program;
  std::vector<std::string> args;

  // Quando informado, o filho troca para este diretorio antes do exec.
  std::optional<std::string> cwd;

  // Limite de tempo de parede; a ausencia do valor significa sem limite.
  std::optional<std::chrono::milliseconds> timeout;
};

struct SpawnResult {
  // Saidas capturadas separadamente dos descritores padrao do processo.
  std::string stdout;
  std::string stderr;

  // O codigo de saida e preservado; executavel ausente e indicado por error_kind.
  int exit_code{-1};

  // Duracao total da chamada, incluindo inicializacao e captura das saidas.
  std::chrono::milliseconds duration{0};

  SpawnStatus status{SpawnStatus::failed};
  SpawnErrorKind error_kind{SpawnErrorKind::none};

  // Fica vazio em sucesso e descreve falhas de lancamento ou de execucao.
  std::string error;
};

// Lanca o processo, captura as saidas e encerra ao atingir o timeout quando informado.
SpawnResult spawn(
    const SpawnRequest& request,
    const ExecutionOutputCallback& on_output = {});

}  // namespace atlas::capabilities
