#include "ignore.hpp"

#include <algorithm>
#include <fstream>
#include <iterator>
#include <sstream>

namespace atlas::capabilities::tools::filesystem {
namespace {

// Artefatos que nunca sao codigo-fonte: dependencias, builds e caches.
constexpr std::string_view kBuiltInPatterns[] = {
    ".git/",
    "node_modules/",
    "target/",
    "dist/",
    "build/",
    "coverage/",
    ".native-cmake/",
    ".native-test/",
    ".venv/",
    "venv/",
    "__pycache__/",
    ".pytest_cache/",
    ".mypy_cache/",
    ".gradle/",
    ".next/",
    ".turbo/",
    ".cache/",
};

std::vector<std::string_view> splitPath(std::string_view path) {
  std::vector<std::string_view> segments;
  std::size_t start = 0;
  while (start <= path.size()) {
    const std::size_t slash = path.find('/', start);
    const std::size_t end = slash == std::string_view::npos ? path.size() : slash;
    if (end > start) {
      segments.push_back(path.substr(start, end - start));
    }
    if (slash == std::string_view::npos) {
      break;
    }
    start = slash + 1;
  }
  return segments;
}

// Casa um segmento de caminho com um padrao de um unico segmento.
bool globSegment(std::string_view pattern, std::string_view text) {
  std::size_t patternIndex = 0;
  std::size_t textIndex = 0;
  std::size_t starIndex = std::string_view::npos;
  std::size_t starText = 0;

  while (textIndex < text.size()) {
    if (patternIndex < pattern.size() && pattern[patternIndex] == '*') {
      starIndex = patternIndex++;
      starText = textIndex;
      continue;
    }
    if (patternIndex < pattern.size() && pattern[patternIndex] == '?') {
      ++patternIndex;
      ++textIndex;
      continue;
    }
    if (patternIndex < pattern.size() && pattern[patternIndex] == '[') {
      const std::size_t close = pattern.find(']', patternIndex + 1);
      if (close != std::string_view::npos && close > patternIndex + 1) {
        std::string_view body = pattern.substr(patternIndex + 1, close - patternIndex - 1);
        bool negated = false;
        if (!body.empty() && (body.front() == '!' || body.front() == '^')) {
          negated = true;
          body.remove_prefix(1);
        }
        bool matched = false;
        for (std::size_t index = 0; index < body.size(); ++index) {
          if (index + 2 < body.size() && body[index + 1] == '-') {
            if (text[textIndex] >= body[index] && text[textIndex] <= body[index + 2]) {
              matched = true;
            }
            index += 2;
            continue;
          }
          if (body[index] == text[textIndex]) {
            matched = true;
          }
        }
        if (matched != negated) {
          patternIndex = close + 1;
          ++textIndex;
          continue;
        }
      }
    } else if (patternIndex < pattern.size() && pattern[patternIndex] == text[textIndex]) {
      ++patternIndex;
      ++textIndex;
      continue;
    }

    if (starIndex != std::string_view::npos) {
      patternIndex = starIndex + 1;
      textIndex = ++starText;
      continue;
    }
    return false;
  }

  while (patternIndex < pattern.size() && pattern[patternIndex] == '*') {
    ++patternIndex;
  }
  return patternIndex == pattern.size();
}

// `**` casa zero ou mais segmentos; os demais segmentos casam um a um.
bool matchSegments(
    const std::vector<std::string_view>& pattern,
    std::size_t patternIndex,
    const std::vector<std::string_view>& path,
    std::size_t pathIndex) {
  if (patternIndex == pattern.size()) {
    return pathIndex == path.size();
  }
  if (pattern[patternIndex] == "**") {
    for (std::size_t skip = pathIndex; skip <= path.size(); ++skip) {
      if (matchSegments(pattern, patternIndex + 1, path, skip)) {
        return true;
      }
    }
    return false;
  }
  if (pathIndex == path.size() || !globSegment(pattern[patternIndex], path[pathIndex])) {
    return false;
  }
  return matchSegments(pattern, patternIndex + 1, path, pathIndex + 1);
}

std::string trimLine(std::string_view line) {
  while (!line.empty() && (line.back() == '\r' || line.back() == ' ' || line.back() == '\t')) {
    line.remove_suffix(1);
  }
  std::size_t start = 0;
  while (start < line.size() && (line[start] == ' ' || line[start] == '\t')) {
    ++start;
  }
  return std::string(line.substr(start));
}

// Sobe do diretorio buscado ate o .git mais proximo para achar a raiz das regras.
std::filesystem::path repositoryRoot(const std::filesystem::path& root) {
  std::error_code code;
  std::filesystem::path current = root;
  while (!current.empty() && current != current.root_path()) {
    if (std::filesystem::exists(current / ".git", code)) {
      return current;
    }
    current = current.parent_path();
  }
  return root;
}

}  // namespace

bool globMatch(std::string_view pattern, std::string_view path) {
  return matchSegments(splitPath(pattern), 0, splitPath(path), 0);
}

std::vector<SearchIgnores::Rule> SearchIgnores::parse(std::string_view content) {
  std::vector<Rule> rules;
  std::istringstream stream{std::string(content)};
  std::string raw;
  while (std::getline(stream, raw)) {
    std::string line = trimLine(raw);
    if (line.empty() || line.front() == '#') {
      continue;
    }

    Rule rule;
    if (line.front() == '!') {
      rule.negated = true;
      line.erase(0, 1);
    }
    if (line.size() > 1 && line.back() == '/') {
      rule.directory_only = true;
      line.pop_back();
    }
    if (line.empty()) {
      continue;
    }
    if (line.front() == '/') {
      rule.anchored = true;
      line.erase(0, 1);
    }
    if (line.find('/') != std::string::npos) {
      rule.anchored = true;
    }
    rule.pattern = std::move(line);
    rules.push_back(std::move(rule));
  }
  return rules;
}

void SearchIgnores::addRules(const std::filesystem::path& base, std::vector<Rule> rules) {
  if (rules.empty()) {
    return;
  }
  sources_.push_back(Source{.base = base, .rules = std::move(rules)});
}

void SearchIgnores::loadDefaults(const std::filesystem::path& base) {
  std::vector<Rule> rules;
  for (const std::string_view pattern : kBuiltInPatterns) {
    std::vector<Rule> parsed = parse(pattern);
    if (!parsed.empty()) {
      rules.push_back(parsed.front());
    }
  }
  addRules(base, std::move(rules));
}

void SearchIgnores::loadDirectory(const std::filesystem::path& directory) {
  std::ifstream file(directory / ".gitignore", std::ios::binary);
  if (!file) {
    return;
  }
  const std::string content{
      std::istreambuf_iterator<char>(file),
      std::istreambuf_iterator<char>()};
  addRules(directory, parse(content));
}

void SearchIgnores::loadAncestors(const std::filesystem::path& root) {
  // O diretorio buscado carrega o proprio .gitignore durante a varredura.
  const std::filesystem::path repository = repositoryRoot(root);
  std::vector<std::filesystem::path> chain;
  for (std::filesystem::path current = root.parent_path();
       !current.empty() && current != current.root_path();
       current = current.parent_path()) {
    chain.push_back(current);
    if (current == repository) {
      break;
    }
  }
  std::reverse(chain.begin(), chain.end());
  for (const std::filesystem::path& directory : chain) {
    loadDirectory(directory);
  }
}

std::size_t SearchIgnores::mark() const {
  return sources_.size();
}

void SearchIgnores::restore(std::size_t mark) {
  if (sources_.size() > mark) {
    sources_.resize(mark);
  }
}

bool SearchIgnores::ignores(const std::filesystem::path& path, bool is_directory) const {
  const std::string name = path.filename().string();
  bool ignored = false;

  for (const Source& source : sources_) {
    const std::filesystem::path relative = path.lexically_relative(source.base);
    if (relative.empty()) {
      continue;
    }
    const std::string relativeText = relative.generic_string();
    if (relativeText == "." || relativeText.starts_with("..")) {
      continue;
    }

    const std::vector<std::string_view> pathSegments = splitPath(relativeText);
    for (const Rule& rule : source.rules) {
      if (rule.directory_only && !is_directory) {
        continue;
      }
      const bool matched = rule.anchored
          ? matchSegments(splitPath(rule.pattern), 0, pathSegments, 0)
          : globSegment(rule.pattern, name);
      if (matched) {
        // A ultima regra que casa decide, como no gitignore.
        ignored = !rule.negated;
      }
    }
  }
  return ignored;
}

}  // namespace atlas::capabilities::tools::filesystem
