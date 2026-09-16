#pragma once

#include "../../core/execution.hpp"

namespace atlas::capabilities {

// Cada executavel fornece este unico ponto de entrada para a tool compilada.
extern "C" ExecutionResult atlas_executable_dispatch(
    const NativeRequest& request,
    const ExecutionOutputCallback& on_output);

}  // namespace atlas::capabilities

namespace atlas::capabilities::runtime::executable {

using Dispatch = ExecutionResult (*)(
    const NativeRequest& request,
    const ExecutionOutputCallback& on_output);

// Executa uma chamada completa usando o dispatch especifico do executavel.
int run(Dispatch dispatch);

}  // namespace atlas::capabilities::runtime::executable
