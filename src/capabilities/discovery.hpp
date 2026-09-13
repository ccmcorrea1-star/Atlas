#pragma once

#include "registry.hpp"

#include <optional>
#include <string>
#include <string_view>
#include <vector>

namespace atlas::capabilities {

struct DiscoveryRequest {
  std::optional<std::string> path;
  std::optional<std::string> query;
};

// Projecao minima enviada ao Agent, sem detalhes de execucao ou aliases.
struct DiscoveryResult {
  std::string id;
  std::string type;
  std::string summary;
};

// Consulta o Registry a cada descoberta e nunca executa a capability encontrada.
class Discovery {
 public:
  explicit Discovery(const Registry& registry) : registry_(registry) {}

  std::vector<DiscoveryResult> discover(const DiscoveryRequest& request = {}) const;
  std::vector<DiscoveryResult> discover(std::string_view path) const;

 private:
  static std::vector<DiscoveryResult> project(const std::vector<Capability>& capabilities);

  const Registry& registry_;
};

}  // namespace atlas::capabilities
