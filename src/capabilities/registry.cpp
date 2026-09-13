#include "registry.hpp"

#include <algorithm>
#include <mutex>
#include <sstream>
#include <utility>

namespace atlas::capabilities {
namespace {

std::string lowerAscii(std::string_view value) {
  std::string lowered;
  lowered.reserve(value.size());
  for (const char character : value) {
    const bool isUppercase = character >= 'A' && character <= 'Z';
    lowered.push_back(isUppercase ? static_cast<char>(character - 'A' + 'a') : character);
  }
  return lowered;
}

std::vector<std::string> queryTokens(std::string_view query) {
  std::istringstream stream(lowerAscii(query));
  std::vector<std::string> tokens;
  std::string token;
  while (stream >> token) {
    tokens.push_back(std::move(token));
  }
  return tokens;
}

bool containsAllTokens(const Capability& capability, const std::vector<std::string>& tokens) {
  std::string searchable = lowerAscii(capability.id);
  searchable += ' ';
  searchable += lowerAscii(capability.type);
  searchable += ' ';
  searchable += lowerAscii(capability.summary);
  if (capability.parent.has_value()) {
    searchable += ' ';
    searchable += lowerAscii(capability.parent.value());
  }
  for (const std::string& alias : capability.aliases) {
    searchable += ' ';
    searchable += lowerAscii(alias);
  }

  for (const std::string& token : tokens) {
    if (searchable.find(token) == std::string::npos) {
      return false;
    }
  }
  return true;
}

}  // namespace

bool Registry::isValid(const Capability& capability) {
  if (capability.id.empty() || capability.type.empty() || capability.summary.empty() ||
      capability.implementation.empty() || capability.id.find('\0') != std::string::npos ||
      capability.type.find('\0') != std::string::npos || capability.summary.find('\0') != std::string::npos ||
      capability.implementation.find('\0') != std::string::npos) {
    return false;
  }
  if (capability.parent.has_value() &&
      (capability.parent->empty() || capability.parent->find('\0') != std::string::npos)) {
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
  return true;
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
  std::shared_lock lock(mutex_);
  const auto iterator = capabilities_.find(id);
  if (iterator == capabilities_.end()) {
    return std::nullopt;
  }
  return iterator->second;
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

std::vector<Capability> Registry::search(std::string_view query) const {
  const std::vector<std::string> tokens = queryTokens(query);
  std::shared_lock lock(mutex_);
  std::vector<Capability> result;
  for (const auto& entry : capabilities_) {
    const Capability& capability = entry.second;
    if (tokens.empty() || containsAllTokens(capability, tokens)) {
      result.push_back(capability);
    }
  }
  return result;
}

}  // namespace atlas::capabilities
