#pragma once

#include "execution.hpp"

#include <optional>
#include <string>
#include <string_view>

namespace atlas::capabilities {

// Valida os subconjuntos de schema usados pelo contrato de capabilities.
std::optional<std::string> validateArguments(
    const StructuredArguments& arguments,
    const StructuredValue& schema);

}  // namespace atlas::capabilities
