#pragma once

#include "registry.hpp"

#include <optional>
#include <string_view>
#include <vector>

namespace atlas::skills {

class SkillDiscovery {
 public:
  explicit SkillDiscovery(const SkillRegistry& registry) : registry_(registry) {}

  std::vector<SkillDiscoveryResult> discover(
      const SkillDiscoveryRequest& request = {}) const;
  std::optional<Skill> getSkill(
      std::string_view id,
      std::optional<std::string_view> path = std::nullopt) const;

 private:
  static std::vector<SkillDiscoveryResult> project(const std::vector<Skill>& skills);

  const SkillRegistry& registry_;
};

}  // namespace atlas::skills
