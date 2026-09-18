#pragma once

namespace atlas::capabilities::tools::git {

// O target permanece no contrato para futura resolucao pelo Device Fabric.
inline constexpr char kLocalTarget[] = "local";

enum class GitStatus {
  success,
  failed,
  timed_out,
  unavailable,
};

}  // namespace atlas::capabilities::tools::git
