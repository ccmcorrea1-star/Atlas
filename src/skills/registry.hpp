#pragma once

#include "types.hpp"

#include <map>
#include <optional>
#include <shared_mutex>
#include <string_view>
#include <vector>

namespace atlas::skills {

enum class SkillRegistration {
  inserted,
  replaced,
  ignored,
  duplicate,
};

class SkillRegistry {
 public:
  SkillRegistry() = default;
  SkillRegistry(const SkillRegistry&) = delete;
  SkillRegistry& operator=(const SkillRegistry&) = delete;

  SkillRegistration registerSkill(Skill skill, SkillSource source);
  bool unregister(std::string_view id);
  bool update(Skill skill, SkillSource source);
  std::optional<Skill> get(std::string_view id) const;
  std::optional<SkillSource> sourceOf(std::string_view id) const;
  std::vector<Skill> list() const;
  std::vector<Skill> search(
      std::string_view query,
      std::optional<std::size_t> limit = std::nullopt) const;

 private:
  struct Entry {
    Skill skill;
    SkillSource source;
  };

  static bool isValid(const Skill& skill);
  static int priority(SkillSource source) noexcept;

  mutable std::shared_mutex mutex_;
  std::map<std::string, Entry, std::less<>> skills_;
};

}  // namespace atlas::skills
