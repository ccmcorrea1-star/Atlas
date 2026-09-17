#pragma once

#include "registry.hpp"

#include <cstddef>
#include <optional>
#include <string>
#include <string_view>
#include <vector>

namespace atlas::capabilities {

struct DiscoveryRequest {
  std::optional<std::string> query;
  std::optional<std::size_t> limit;
};

// Projecao minima enviada ao Agent, sem detalhes de execucao ou aliases.
struct DiscoveryResult {
  std::string id;
  std::string type;
  std::string summary;
};

struct ToolListResult {
  std::string id;
  std::string type;
  std::string summary;
  std::optional<std::string> group;
};

// Consulta o Registry sem executar a capability encontrada.
class Discovery {
 public:
  explicit Discovery(const Registry& registry) : registry_(registry) {}

  std::vector<DiscoveryResult> discover(const DiscoveryRequest& request = {}) const;
  std::vector<ToolListResult> listTools(
      std::optional<std::string_view> group = std::nullopt) const;
  // Carrega detalhes somente quando o Agent escolhe uma capability utilizavel.
  std::optional<Capability> getDefinition(std::string_view id) const;

 private:
  static std::vector<DiscoveryResult> project(const std::vector<Capability>& capabilities);
  static std::vector<ToolListResult> projectTools(const std::vector<Capability>& capabilities);

  const Registry& registry_;
};

}  // namespace atlas::capabilities
