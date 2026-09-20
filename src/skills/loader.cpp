#include "loader.hpp"

#include <algorithm>
#include <cctype>
#include <fstream>
#include <iterator>
#include <system_error>
#include <utility>
#include <vector>

namespace atlas::skills {
namespace {

std::string trim(std::string_view value) {
  std::size_t first = 0;
  while (first < value.size() && std::isspace(static_cast<unsigned char>(value[first])) != 0) {
    ++first;
  }
  std::size_t last = value.size();
  while (last > first && std::isspace(static_cast<unsigned char>(value[last - 1])) != 0) {
    --last;
  }
  return std::string(value.substr(first, last - first));
}

std::string frontmatterValue(std::string_view value) {
  std::string result = trim(value);
  if (result.size() >= 2 &&
      ((result.front() == '"' && result.back() == '"') ||
       (result.front() == '\'' && result.back() == '\''))) {
    result = result.substr(1, result.size() - 2);
  }
  return result;
}

}  // namespace

bool SkillLoader::parse(
    const std::filesystem::path& path,
    Skill& skill,
    std::string& error) {
  std::ifstream file(path, std::ios::binary);
  if (!file) {
    error = "cannot open skill";
    return false;
  }
  const std::string contents{
      std::istreambuf_iterator<char>(file),
      std::istreambuf_iterator<char>()};
  if (file.bad()) {
    error = "cannot read skill";
    return false;
  }

  const std::size_t firstLineEnd = contents.find('\n');
  const std::string firstLine = trim(
      std::string_view(contents).substr(0, firstLineEnd == std::string::npos ? contents.size() : firstLineEnd));
  if (firstLine != "---" || firstLineEnd == std::string::npos) {
    error = "skill must start with YAML frontmatter";
    return false;
  }

  bool hasName = false;
  bool hasDescription = false;
  std::size_t position = firstLineEnd + 1;
  std::size_t instructionsStart = std::string::npos;
  while (position <= contents.size()) {
    const std::size_t lineEnd = contents.find('\n', position);
    std::string_view line(
        contents.data() + position,
        lineEnd == std::string::npos ? contents.size() - position : lineEnd - position);
    if (!line.empty() && line.back() == '\r') {
      line.remove_suffix(1);
    }
    if (trim(line) == "---") {
      instructionsStart = lineEnd == std::string::npos ? contents.size() : lineEnd + 1;
      break;
    }

    const std::string metadata = trim(line);
    if (!metadata.empty()) {
      const std::size_t separator = metadata.find(':');
      if (separator == std::string::npos) {
        error = "invalid skill frontmatter entry";
        return false;
      }
      const std::string key = trim(std::string_view(metadata).substr(0, separator));
      const std::string value = frontmatterValue(std::string_view(metadata).substr(separator + 1));
      if (key == "name") {
        if (hasName || value.empty()) {
          error = "frontmatter field 'name' must be present once and non-empty";
          return false;
        }
        skill.id = value;
        hasName = true;
      } else if (key == "description") {
        if (hasDescription || value.empty()) {
          error = "frontmatter field 'description' must be present once and non-empty";
          return false;
        }
        skill.summary = value;
        hasDescription = true;
      }
    }

    if (lineEnd == std::string::npos) {
      break;
    }
    position = lineEnd + 1;
  }

  if (instructionsStart == std::string::npos) {
    error = "skill frontmatter is not closed";
    return false;
  }
  if (!hasName || !hasDescription) {
    error = "skill frontmatter requires 'name' and 'description'";
    return false;
  }
  skill.instructions = contents.substr(instructionsStart);
  skill.source = sourcePath(path);
  return true;
}

std::filesystem::path SkillLoader::sourcePath(const std::filesystem::path& path) {
  std::error_code error;
  const std::filesystem::path absolute = std::filesystem::absolute(path, error);
  return (error ? path : absolute).lexically_normal();
}

bool SkillLoader::fail(std::string message) {
  last_error_ = std::move(message);
  return false;
}

bool SkillLoader::load(const std::filesystem::path& path, SkillSource source) {
  last_error_.clear();
  if (path.filename() != "SKILL.md") {
    return fail("skill path must point to 'SKILL.md'");
  }

  Skill skill;
  std::string error;
  if (!parse(path, skill, error)) {
    return fail("failed to load skill '" + path.string() + "': " + error);
  }

  const std::string id = skill.id;
  const SkillRegistration registration = registry_.registerSkill(std::move(skill), source);
  if (registration == SkillRegistration::ignored) {
    return true;
  }
  if (registration == SkillRegistration::duplicate) {
    return fail("skill has an equal-precedence duplicate");
  }

  sources_[id] = sourcePath(path);
  sourceKinds_[id] = source;
  return true;
}

bool SkillLoader::unload(std::string_view id) {
  last_error_.clear();
  if (id.empty() || !registry_.unregister(id)) {
    return fail("skill is not registered");
  }
  sources_.erase(std::string(id));
  sourceKinds_.erase(std::string(id));
  return true;
}

bool SkillLoader::reload(std::string_view id) {
  last_error_.clear();
  const auto source = sources_.find(id);
  const auto sourceKind = sourceKinds_.find(id);
  if (source == sources_.end() || sourceKind == sourceKinds_.end()) {
    return fail("skill has no loaded SKILL.md");
  }

  Skill skill;
  std::string error;
  if (!parse(source->second, skill, error)) {
    return fail("failed to reload skill: " + error);
  }
  if (skill.id != id || !registry_.update(std::move(skill), sourceKind->second)) {
    return fail("reloaded SKILL.md does not match the registered Skill");
  }
  return true;
}

bool SkillLoader::scan(const std::filesystem::path& directory, SkillSource source) {
  last_error_.clear();
  std::error_code errorCode;
  if (!std::filesystem::is_directory(directory, errorCode)) {
    return fail("cannot scan skills '" + directory.string() + "': directory does not exist");
  }
  if (errorCode) {
    return fail("cannot scan skills '" + directory.string() + "': " + errorCode.message());
  }

  std::vector<std::filesystem::path> skills;
  std::filesystem::directory_iterator iterator(directory, errorCode);
  if (errorCode) {
    return fail("cannot scan skills '" + directory.string() + "': " + errorCode.message());
  }
  const std::filesystem::directory_iterator end;
  while (iterator != end) {
    std::error_code entryError;
    if (iterator->is_directory(entryError)) {
      const std::filesystem::path skill = iterator->path() / "SKILL.md";
      std::error_code skillError;
      if (std::filesystem::is_regular_file(skill, skillError)) {
        skills.push_back(skill);
      } else if (skillError) {
        return fail("cannot inspect skill '" + skill.string() + "': " + skillError.message());
      }
    } else if (entryError) {
      return fail("cannot inspect '" + iterator->path().string() + "': " + entryError.message());
    }
    iterator.increment(errorCode);
    if (errorCode) {
      return fail("cannot scan skills '" + directory.string() + "': " + errorCode.message());
    }
  }
  std::sort(skills.begin(), skills.end());

  for (const std::filesystem::path& skill : skills) {
    if (!load(skill, source)) {
      return false;
    }
  }
  return true;
}

}  // namespace atlas::skills
