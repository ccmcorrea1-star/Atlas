#include "discovery.hpp"

#include <algorithm>

namespace atlas::capabilities {

std::vector<DiscoveryResult> Discovery::discover(const DiscoveryRequest& request) const {
  std::vector<Capability> capabilities;
  if (request.query.has_value()) {
    capabilities = registry_.search(
        request.query.value(),
        request.path.has_value() ? std::nullopt : request.limit);
    if (request.path.has_value()) {
      const std::string_view path = request.path.value();
      capabilities.erase(
          std::remove_if(
              capabilities.begin(),
              capabilities.end(),
              [path](const Capability& capability) {
                return capability.parent.has_value() ? capability.parent.value() != path : !path.empty();
              }),
          capabilities.end());
      if (request.limit.has_value() && capabilities.size() > request.limit.value()) {
        capabilities.resize(request.limit.value());
      }
    }
  } else if (request.path.has_value()) {
    capabilities = request.path->empty() ? registry_.rootGroups() : registry_.children(request.path.value());
  } else {
    // O nivel inicial revela somente grupos raiz, nunca tools individuais.
    capabilities = registry_.rootGroups();
  }
  return project(capabilities);
}

std::vector<DiscoveryResult> Discovery::discover(std::string_view path) const {
  DiscoveryRequest request;
  request.path = std::string(path);
  return discover(request);
}

std::optional<Capability> Discovery::getDefinition(std::string_view id) const {
  return registry_.getDefinition(id);
}

std::vector<DiscoveryResult> Discovery::project(const std::vector<Capability>& capabilities) {
  std::vector<DiscoveryResult> result;
  result.reserve(capabilities.size());
  for (const Capability& capability : capabilities) {
    result.push_back({capability.id, capability.type, capability.summary});
  }
  return result;
}

}  // namespace atlas::capabilities
