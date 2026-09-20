#pragma once

#include "registry.hpp"

#include <filesystem>
#include <map>
#include <string>
#include <string_view>

namespace atlas::skills {

class SkillLoader {
 public:
  explicit SkillLoader(SkillRegistry& registry) : registry_(registry) {}
  SkillLoader(const SkillLoader&) = delete;
  SkillLoader& operator=(const SkillLoader&) = delete;

  bool load(
      const std::filesystem::path& path,
      SkillSource source = SkillSource::project);
  bool unload(std::string_view id);
  bool reload(std::string_view id);
  bool scan(const std::filesystem::path& directory, SkillSource source);

  const std::string& lastError() const noexcept { return last_error_; }

 private:
  static bool parse(
      const std::filesystem::path& path,
      Skill& skill,
      std::string& error);
  static std::filesystem::path sourcePath(const std::filesystem::path& path);
  bool fail(std::string message);

  SkillRegistry& registry_;
  std::map<std::string, std::filesystem::path, std::less<>> sources_;
  std::map<std::string, SkillSource, std::less<>> sourceKinds_;
  std::string last_error_;
};

}  // namespace atlas::skills
