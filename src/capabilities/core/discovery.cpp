#include "discovery.hpp"

#include <algorithm>

namespace atlas::capabilities {

std::vector<DiscoveryResult> ToolDiscovery::discover(const DiscoveryRequest& request) const {
  std::vector<Capability> capabilities = registry_.search(request.query.value_or(""));
  capabilities.erase(
      std::remove_if(
          capabilities.begin(),
          capabilities.end(),
           [](const Capability& capability) { return capability.type != "tool"; }),
      capabilities.end());
  if (request.limit.has_value() && capabilities.size() > request.limit.value()) {
    capabilities.resize(request.limit.value());
  }
  return project(capabilities);
}

std::vector<ToolListResult> ToolDiscovery::listTools(std::optional<std::string_view> group) const {
  std::vector<Capability> capabilities = group.has_value()
      ? registry_.children(group.value())
      : registry_.list();
  capabilities.erase(
      std::remove_if(
          capabilities.begin(),
          capabilities.end(),
           [](const Capability& capability) { return capability.type != "tool"; }),
      capabilities.end());
  return projectTools(capabilities);
}

std::optional<Capability> ToolDiscovery::getDefinition(std::string_view id) const {
  const auto capability = registry_.getDefinition(id);
  if (!capability.has_value() || capability->type != "tool") {
    return std::nullopt;
  }
  return capability;
}

std::vector<DiscoveryResult> ToolDiscovery::project(const std::vector<Capability>& capabilities) {
  std::vector<DiscoveryResult> result;
  result.reserve(capabilities.size());
  for (const Capability& capability : capabilities) {
    result.push_back({capability.id, capability.type, capability.summary});
  }
  return result;
}

std::vector<ToolListResult> ToolDiscovery::projectTools(const std::vector<Capability>& capabilities) {
  std::vector<ToolListResult> result;
  result.reserve(capabilities.size());
  for (const Capability& capability : capabilities) {
    result.push_back({capability.id, capability.type, capability.summary, capability.parent});
  }
  return result;
}

}  // namespace atlas::capabilities
