#include "discovery.hpp"

#include <algorithm>
#include <array>
#include <fstream>
#include <iterator>

namespace atlas::skills {
namespace {

bool isAuxiliaryPath(const std::filesystem::path& path) {
  static constexpr std::array<std::string_view, 4> roots = {
      "references", "scripts", "templates", "assets"};
  const auto first = path.begin();
  if (first == path.end()) {
    return false;
  }
  return std::find(roots.begin(), roots.end(), first->string()) != roots.end();
}

bool safeRelativePath(std::string_view value, std::filesystem::path& path) {
  if (value.empty()) {
    return false;
  }
  path = std::filesystem::path(value).lexically_normal();
  if (path.is_absolute() || path == "." || path.filename() == "SKILL.md") {
    return false;
  }
  for (const auto& component : path) {
    if (component == "..") {
      return false;
    }
  }
  return isAuxiliaryPath(path);
}

bool isWithin(const std::filesystem::path& root, const std::filesystem::path& candidate) {
  auto rootPart = root.begin();
  auto candidatePart = candidate.begin();
  while (rootPart != root.end() && candidatePart != candidate.end()) {
    if (*rootPart != *candidatePart) {
      return false;
    }
    ++rootPart;
    ++candidatePart;
  }
  return rootPart == root.end();
}

}  // namespace

std::vector<SkillDiscoveryResult> SkillDiscovery::discover(
    const SkillDiscoveryRequest& request) const {
  const std::optional<std::size_t> limit = request.hasLimit
      ? std::optional<std::size_t>(request.limit)
      : std::nullopt;
  return project(registry_.search(request.query, limit));
}

std::optional<Skill> SkillDiscovery::getSkill(
    std::string_view id,
    std::optional<std::string_view> path) const {
  std::optional<Skill> skill = registry_.get(id);
  if (!skill.has_value() || !path.has_value()) {
    return skill;
  }

  std::filesystem::path relative;
  if (!safeRelativePath(path.value(), relative)) {
    return std::nullopt;
  }
  std::error_code error;
  const std::filesystem::path root = std::filesystem::canonical(
      skill->source.parent_path(), error);
  if (error) {
    return std::nullopt;
  }
  const std::filesystem::path filePath = std::filesystem::canonical(root / relative, error);
  if (error || !isWithin(root, filePath) ||
      !std::filesystem::is_regular_file(filePath, error) || error) {
    return std::nullopt;
  }
  std::ifstream file(filePath, std::ios::binary);
  if (!file) {
    return std::nullopt;
  }
  skill->files.push_back({relative.generic_string(), {
      std::istreambuf_iterator<char>(file),
      std::istreambuf_iterator<char>()}});
  return skill;
}

std::vector<SkillDiscoveryResult> SkillDiscovery::project(const std::vector<Skill>& skills) {
  std::vector<SkillDiscoveryResult> result;
  result.reserve(skills.size());
  for (const Skill& skill : skills) {
    result.push_back({skill.id, "skill", skill.summary});
  }
  return result;
}

}  // namespace atlas::skills
