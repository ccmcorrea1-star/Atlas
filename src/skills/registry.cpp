#include "registry.hpp"

#include <algorithm>
#include <cctype>
#include <mutex>
#include <string>
#include <utility>

namespace atlas::skills {
namespace {

std::string normalize(std::string_view value) {
  std::string result;
  result.reserve(value.size());
  for (std::size_t index = 0; index < value.size(); ++index) {
    const unsigned char character = static_cast<unsigned char>(value[index]);
    if (character < 0x80) {
      if (std::isalnum(character) != 0) {
        result.push_back(static_cast<char>(std::tolower(character)));
      } else {
        result.push_back(' ');
      }
      continue;
    }
    if (character == 0xc3 && index + 1 < value.size()) {
      const unsigned char second = static_cast<unsigned char>(value[++index]);
      switch (second) {
        case 0xa0: case 0xa1: case 0xa2: case 0xa3: case 0xa4: case 0xa5:
          result.push_back('a');
          break;
        case 0xa7:
          result.push_back('c');
          break;
        case 0xa8: case 0xa9: case 0xaa: case 0xab:
          result.push_back('e');
          break;
        case 0xac: case 0xad: case 0xae: case 0xaf:
          result.push_back('i');
          break;
        case 0xb1:
          result.push_back('n');
          break;
        case 0xb2: case 0xb3: case 0xb4: case 0xb5: case 0xb6:
          result.push_back('o');
          break;
        case 0xb9: case 0xba: case 0xbb: case 0xbc:
          result.push_back('u');
          break;
        default:
          result.push_back(' ');
          break;
      }
      continue;
    }
    result.push_back(' ');
  }
  return result;
}

std::vector<std::string> queryWords(std::string_view query) {
  std::vector<std::string> words;
  std::string current;
  for (const char character : normalize(query)) {
    if (character != ' ') {
      current.push_back(character);
    } else if (!current.empty()) {
      words.push_back(std::move(current));
      current.clear();
    }
  }
  if (!current.empty()) {
    words.push_back(std::move(current));
  }
  return words;
}

int score(const Skill& skill, const std::vector<std::string>& words, bool& full) {
  if (words.empty()) {
    full = true;
    return 0;
  }
  const std::string id = normalize(skill.id);
  const std::string summary = normalize(skill.summary);
  int matched = 0;
  for (const std::string& word : words) {
    if (id.find(word) != std::string::npos) {
      ++matched;
      continue;
    }
    if (summary.find(word) != std::string::npos) {
      ++matched;
    }
  }
  full = matched == static_cast<int>(words.size());
  if (!full) {
    return -1;
  }
  return matched * 1000000 + (id.find(words.front()) != std::string::npos ? 100 : 0);
}

}  // namespace

int SkillRegistry::priority(SkillSource source) noexcept {
  switch (source) {
    case SkillSource::agents:
      return 1;
    case SkillSource::global:
      return 2;
    case SkillSource::project:
      return 3;
  }
  return 0;
}

bool SkillRegistry::isValid(const Skill& skill) {
  return !skill.id.empty() && !skill.summary.empty() &&
      !skill.source.empty() && skill.id.find('\0') == std::string::npos &&
      skill.summary.find('\0') == std::string::npos && skill.instructions.find('\0') == std::string::npos;
}

SkillRegistration SkillRegistry::registerSkill(Skill skill, SkillSource source) {
  if (!isValid(skill)) {
    return SkillRegistration::duplicate;
  }

  std::unique_lock lock(mutex_);
  const auto iterator = skills_.find(skill.id);
  if (iterator == skills_.end()) {
    const std::string id = skill.id;
    skills_.emplace(id, Entry{std::move(skill), source});
    return SkillRegistration::inserted;
  }
  if (priority(source) < priority(iterator->second.source)) {
    return SkillRegistration::ignored;
  }
  if (priority(source) == priority(iterator->second.source)) {
    return SkillRegistration::duplicate;
  }
  iterator->second = Entry{std::move(skill), source};
  return SkillRegistration::replaced;
}

bool SkillRegistry::unregister(std::string_view id) {
  std::unique_lock lock(mutex_);
  const auto iterator = skills_.find(id);
  if (iterator == skills_.end()) {
    return false;
  }
  skills_.erase(iterator);
  return true;
}

bool SkillRegistry::update(Skill skill, SkillSource source) {
  if (!isValid(skill)) {
    return false;
  }
  std::unique_lock lock(mutex_);
  const auto iterator = skills_.find(skill.id);
  if (iterator == skills_.end() || priority(source) < priority(iterator->second.source)) {
    return false;
  }
  iterator->second = Entry{std::move(skill), source};
  return true;
}

std::optional<Skill> SkillRegistry::get(std::string_view id) const {
  std::shared_lock lock(mutex_);
  const auto iterator = skills_.find(id);
  return iterator == skills_.end() ? std::nullopt : std::optional<Skill>(iterator->second.skill);
}

std::optional<SkillSource> SkillRegistry::sourceOf(std::string_view id) const {
  std::shared_lock lock(mutex_);
  const auto iterator = skills_.find(id);
  return iterator == skills_.end() ? std::nullopt : std::optional<SkillSource>(iterator->second.source);
}

std::vector<Skill> SkillRegistry::list() const {
  std::shared_lock lock(mutex_);
  std::vector<Skill> result;
  result.reserve(skills_.size());
  for (const auto& [id, entry] : skills_) {
    result.push_back(entry.skill);
  }
  return result;
}

std::vector<Skill> SkillRegistry::search(
    std::string_view query,
    std::optional<std::size_t> limit) const {
  const std::vector<std::string> words = queryWords(query);
  struct Scored {
    Skill skill;
    int value;
  };
  std::vector<Scored> scored;
  std::shared_lock lock(mutex_);
  for (const auto& [id, entry] : skills_) {
    bool full = false;
    const int value = score(entry.skill, words, full);
    if (value >= 0) {
      scored.push_back({entry.skill, value});
    }
  }
  std::sort(scored.begin(), scored.end(), [](const Scored& left, const Scored& right) {
    if (left.value != right.value) {
      return left.value > right.value;
    }
    return left.skill.id < right.skill.id;
  });
  const std::size_t resultSize = limit.has_value() ? std::min(*limit, scored.size()) : scored.size();
  std::vector<Skill> result;
  result.reserve(resultSize);
  for (std::size_t index = 0; index < resultSize; ++index) {
    result.push_back(std::move(scored[index].skill));
  }
  return result;
}

}  // namespace atlas::skills
