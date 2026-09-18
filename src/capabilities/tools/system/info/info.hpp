#pragma once

#include "../../../core/execution.hpp"

#include <optional>
#include <string>

namespace atlas::capabilities::tools::system {

struct SystemInfo {
  // Nome do kernel em minusculo, por exemplo "linux".
  std::string platform;

  // Nome do sistema operacional, preferindo /etc/os-release ao nome do kernel.
  std::string os_name;

  // Versao do sistema operacional, preferindo /etc/os-release a versao do kernel.
  std::string os_version;

  // Versao do kernel reportada por uname.
  std::string kernel_version;

  // Arquitetura do hardware reportada por uname.
  std::string architecture;

  // Nome da maquina reportado por uname.
  std::string hostname;

  // Nome do usuario efetivo do processo.
  std::string username;

  // Shell padrao do usuario efetivo.
  std::string shell;

  // Fuso horario: TZ, alvo de /etc/localtime ou UTC como ultimo recurso.
  std::string timezone;
};

// Coleta as informacoes do sistema local sem executar comandos externos.
SystemInfo systemInfo();

// Adapta os argumentos estruturados do runtime para a coleta das informacoes.
atlas::capabilities::ExecutionResult dispatch(
    const atlas::capabilities::NativeRequest& request,
    const atlas::capabilities::ExecutionOutputCallback& /* unused */);

}  // namespace atlas::capabilities::tools::system
