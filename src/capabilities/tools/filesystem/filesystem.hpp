#pragma once

#include "../../core/execution.hpp"

namespace atlas::capabilities::tools::filesystem {

// Ler o conteudo completo ou por intervalo de linhas.
atlas::capabilities::ExecutionResult readDispatch(
    const atlas::capabilities::NativeRequest& request);

// Criar ou sobrescrever um arquivo com o conteudo completo.
atlas::capabilities::ExecutionResult writeDispatch(
    const atlas::capabilities::NativeRequest& request);

// Substituir trechos literais de um arquivo, na ordem informada.
atlas::capabilities::ExecutionResult editDispatch(
    const atlas::capabilities::NativeRequest& request);

// Listar as entradas imediatas de um diretorio.
atlas::capabilities::ExecutionResult listDispatch(
    const atlas::capabilities::NativeRequest& request);

// Buscar uma substring em nomes de arquivos e conteudo de forma recursiva.
atlas::capabilities::ExecutionResult searchDispatch(
    const atlas::capabilities::NativeRequest& request);

}  // namespace atlas::capabilities::tools::filesystem
