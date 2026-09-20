#pragma once

#include <filesystem>
#include <string>
#include <vector>

namespace atlas::skills {

enum class SkillSource {
  agents,
  global,
  project,
};

// Arquivo auxiliar carregado somente quando a Skill o solicita.
struct SkillFile {
  std::string path;
  std::string content;
};

struct Skill {
  std::string id;
  std::string summary;
  std::string instructions;
  std::filesystem::path source;
  std::vector<SkillFile> files;
};

struct SkillDiscoveryRequest {
  std::string query;
  std::size_t limit{0};
  bool hasLimit{false};
};

struct SkillDiscoveryResult {
  std::string id;
  std::string type{"skill"};
  std::string summary;
};

}  // namespace atlas::skills
