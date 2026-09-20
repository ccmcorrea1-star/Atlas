#include "registry.hpp"
#include "../../search.hpp"

#include <algorithm>
#include <mutex>
#include <utility>

namespace atlas::capabilities {

bool Registry::isValid(const Capability& capability) {
  if (capability.id.empty() || capability.type.empty() || capability.summary.empty() ||
      capability.id.find('\0') != std::string::npos ||
      capability.type.find('\0') != std::string::npos || capability.summary.find('\0') != std::string::npos ||
      capability.description.find('\0') != std::string::npos ||
      capability.implementation.kind.find('\0') != std::string::npos ||
      capability.implementation.entrypoint.find('\0') != std::string::npos) {
    return false;
  }
  if (capability.parent.has_value() &&
      (capability.parent->empty() || capability.parent->find('\0') != std::string::npos)) {
    return false;
  }
  if (capability.type != "tool" && capability.type != "group") {
    return false;
  }

  std::vector<std::string> aliases;
  aliases.reserve(capability.aliases.size());
  for (const std::string& alias : capability.aliases) {
    if (alias.empty() || alias.find('\0') != std::string::npos || alias == capability.id) {
      return false;
    }
    if (std::find(aliases.begin(), aliases.end(), alias) != aliases.end()) {
      return false;
    }
    aliases.push_back(alias);
  }
  if (capability.type == "group") {
    return capability.implementation.kind.empty() && capability.implementation.entrypoint.empty();
  }
  return !capability.implementation.empty();
}

bool Registry::registerCapability(Capability capability) {
  if (!isValid(capability)) {
    return false;
  }

  std::string id = capability.id;
  std::unique_lock lock(mutex_);
  const auto [iterator, inserted] = capabilities_.try_emplace(std::move(id), std::move(capability));
  return inserted;
}

bool Registry::unregister(std::string_view id) {
  std::unique_lock lock(mutex_);
  const auto iterator = capabilities_.find(id);
  if (iterator == capabilities_.end()) {
    return false;
  }
  capabilities_.erase(iterator);
  return true;
}

bool Registry::update(Capability capability) {
  if (!isValid(capability)) {
    return false;
  }

  std::unique_lock lock(mutex_);
  const auto iterator = capabilities_.find(capability.id);
  if (iterator == capabilities_.end()) {
    return false;
  }
  iterator->second = std::move(capability);
  return true;
}

bool Registry::update(std::string_view id, Capability capability) {
  if (capability.id != id) {
    return false;
  }
  return update(std::move(capability));
}

std::optional<Capability> Registry::get(std::string_view id) const {
  return getDefinition(id);
}

std::optional<Capability> Registry::getDefinition(std::string_view id) const {
  std::shared_lock lock(mutex_);
  const auto iterator = capabilities_.find(id);
  if (iterator == capabilities_.end()) {
    return std::nullopt;
  }
  return iterator->second;
}

bool Registry::registerNativeEntrypoint(std::string entrypoint, NativeEntrypoint function) {
  if (entrypoint.empty() || entrypoint.find('\0') != std::string::npos || !function) {
    return false;
  }

  std::unique_lock lock(mutex_);
  return native_entrypoints_.try_emplace(std::move(entrypoint), std::move(function)).second;
}

// Copia o callback sob lock para permitir execucao sem reter o Registry bloqueado.
std::optional<NativeEntrypoint> Registry::resolveNativeEntrypoint(std::string_view entrypoint) const {
  std::shared_lock lock(mutex_);
  const auto iterator = native_entrypoints_.find(entrypoint);
  if (iterator == native_entrypoints_.end()) {
    return std::nullopt;
  }
  return iterator->second;
}

bool Registry::unregisterNativeEntrypoint(std::string_view entrypoint) {
  std::unique_lock lock(mutex_);
  const auto iterator = native_entrypoints_.find(entrypoint);
  if (iterator == native_entrypoints_.end()) {
    return false;
  }
  native_entrypoints_.erase(iterator);
  return true;
}

std::vector<Capability> Registry::list() const {
  std::shared_lock lock(mutex_);
  std::vector<Capability> result;
  result.reserve(capabilities_.size());
  for (const auto& entry : capabilities_) {
    result.push_back(entry.second);
  }
  return result;
}

std::vector<Capability> Registry::rootGroups() const {
  std::shared_lock lock(mutex_);
  std::vector<Capability> result;
  for (const auto& entry : capabilities_) {
    const Capability& capability = entry.second;
    if (capability.type == "group" && !capability.parent.has_value()) {
      result.push_back(capability);
    }
  }
  return result;
}

std::vector<Capability> Registry::children(std::string_view path) const {
  std::shared_lock lock(mutex_);
  std::vector<Capability> result;
  for (const auto& entry : capabilities_) {
    const Capability& capability = entry.second;
    const bool isChild = capability.parent.has_value()
        ? capability.parent.value() == path
        : path.empty();
    if (isChild) {
      result.push_back(capability);
    }
  }
  return result;
}

std::vector<Capability> Registry::search(
    std::string_view query,
    std::optional<std::size_t> limit) const {
  const search::Query searchQuery(query);
  std::shared_lock lock(mutex_);
  struct ScoredCapability {
    Capability capability;
    int score;
    bool full;
  };
  std::vector<ScoredCapability> scored;
  for (const auto& entry : capabilities_) {
    const Capability& capability = entry.second;
    const auto [score, full] = searchQuery.match(
        {capability.id, capability.summary, capability.aliases, capability.description});
    if (score >= 0) {
      scored.push_back({capability, score, full});
    }
  }
  // Reserva (todos-menos-um) so vale quando a intencao estrita nao casa nada:
  // havendo correspondencia total, parciais nao poluem o resultado.
  const bool hasStrict = std::any_of(scored.begin(), scored.end(), [](const ScoredCapability& entry) {
        return entry.full;
      });
  if (hasStrict) {
    scored.erase(
        std::remove_if(
            scored.begin(),
            scored.end(),
            [](const ScoredCapability& entry) { return !entry.full; }),
        scored.end());
  }

  std::sort(
      scored.begin(),
      scored.end(),
      [](const ScoredCapability& left, const ScoredCapability& right) {
        if (left.score != right.score) {
          return left.score > right.score;
        }
        return left.capability.id < right.capability.id;
      });

  std::vector<Capability> result;
  const std::size_t resultSize = limit.has_value() ? std::min(*limit, scored.size()) : scored.size();
  result.reserve(resultSize);
  for (std::size_t index = 0; index < resultSize; ++index) {
    result.push_back(std::move(scored[index].capability));
  }
  return result;
}

}  // namespace atlas::capabilities
