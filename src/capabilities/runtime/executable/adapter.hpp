#pragma once

#include "../../core/execution.hpp"

namespace atlas::capabilities::runtime::executable {

using Dispatch = ExecutionResult (*) (
    const NativeRequest& request,
    const ExecutionOutputCallback& on_output);

// Resolve o dispatch C++ normal fornecido pela capability do executavel.
Dispatch dispatch();

// Executa uma chamada completa usando o dispatch fornecido pela capability.
int run();
int run(Dispatch dispatch);

}  // namespace atlas::capabilities::runtime::executable
