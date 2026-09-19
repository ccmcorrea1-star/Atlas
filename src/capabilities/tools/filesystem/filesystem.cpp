#include "filesystem.hpp"

#include "search/ignore.hpp"

#include <algorithm>
#include <cstddef>
#include <filesystem>
#include <fstream>
#include <iterator>
#include <limits>
#include <map>
#include <set>
#include <string>
#include <string_view>
#include <utility>
#include <vector>

namespace atlas::capabilities::tools::filesystem {
namespace {

constexpr std::int64_t kDefaultSearchLimit = 200;

const StructuredValue* argument(
    const atlas::capabilities::NativeRequest& request,
    std::string_view name) {
  const auto iterator = request.arguments.find(name);
  return iterator == request.arguments.end() ? nullptr : &iterator->second;
}

const std::string* stringField(
    const atlas::capabilities::NativeRequest& request,
    std::string_view name) {
  const StructuredValue* value = argument(request, name);
  return value == nullptr ? nullptr : std::get_if<std::string>(&value->value);
}

atlas::capabilities::ExecutionResult failure(
    std::string_view target,
    std::string error) {
  atlas::capabilities::ExecutionResult result;
  result.target = std::string(target);
  result.status = atlas::capabilities::ExecutionStatus::failed;
  result.error = std::move(error);
  return result;
}

// Conteudo e limites de cada linha, em posicoes sobre o texto do arquivo.
struct FileLines {
  std::string content;
  std::vector<std::size_t> line_starts;
  // Inclui o newline da linha quando existir; a ultima linha termina no fim.
  std::vector<std::size_t> line_ends;
};

bool requirePath(
    const atlas::capabilities::NativeRequest& request,
    const std::string** path,
    std::string& error) {
  *path = stringField(request, "path");
  if (*path == nullptr || (*path)->empty()) {
    error = "field 'path' must be a non-empty string";
    return false;
  }
  return true;
}

bool readLines(const std::string& path, FileLines& lines, std::string& error) {
  std::error_code code;
  if (!std::filesystem::exists(path, code)) {
    error = "path does not exist: " + path;
    return false;
  }
  if (!std::filesystem::is_regular_file(path, code)) {
    error = "path is not a regular file: " + path;
    return false;
  }

  std::ifstream file(path, std::ios::binary);
  if (!file) {
    error = "cannot open file: " + path;
    return false;
  }
  lines.content.assign(
      std::istreambuf_iterator<char>(file),
      std::istreambuf_iterator<char>());
  if (file.bad()) {
    error = "cannot read file: " + path;
    return false;
  }

  const std::string_view raw(lines.content);
  std::size_t position = 0;
  while (position < raw.size()) {
    const std::size_t newline = raw.find('\n', position);
    lines.line_starts.push_back(position);
    if (newline == std::string_view::npos) {
      lines.line_ends.push_back(raw.size());
      break;
    }
    lines.line_ends.push_back(newline + 1);
    position = newline + 1;
  }
  return true;
}

// Converte offset (1-based, 0 significa inicio) e limit para o intervalo fechado.
bool lineRange(
    const FileLines& lines,
    std::int64_t offset_argument,
    std::int64_t limit_argument,
    std::size_t& first,
    std::size_t& last,
    std::string& error) {
  const std::int64_t total = static_cast<std::int64_t>(lines.line_starts.size());
  const std::int64_t offset = offset_argument == 0 ? 1 : offset_argument;

  if (offset < 1 || offset > total + 1) {
    error = "line offset " + std::to_string(offset) +
        " is beyond the file range of " + std::to_string(total) + " lines";
    return false;
  }

  const std::int64_t available = total + 1 - offset;
  const std::size_t count = limit_argument == 0 || limit_argument >= available
      ? static_cast<std::size_t>(available)
      : static_cast<std::size_t>(limit_argument);
  first = static_cast<std::size_t>(offset);
  last = first + count - 1;
  return true;
}

// Conteudo do intervalo, exatamente como esta no arquivo, incluindo o newline final.
std::string sliceContent(const FileLines& lines, std::size_t first, std::size_t last) {
  if (last < first) {
    return std::string();
  }
  return lines.content.substr(
      lines.line_starts[first - 1],
      lines.line_ends[last - 1] - lines.line_starts[first - 1]);
}

// Linha sem o newline final, para resultados de busca legiveis.
std::string lineText(const FileLines& lines, std::size_t index) {
  std::string text = lines.content.substr(
      lines.line_starts[index],
      lines.line_ends[index] - lines.line_starts[index]);
  if (!text.empty() && text.back() == '\n') {
    text.pop_back();
  }
  return text;
}

bool writeFile(const std::string& path, const std::string& content, std::string& error) {
  std::ofstream file(path, std::ios::binary | std::ios::trunc);
  if (!file) {
    error = "cannot open file for writing: " + path;
    return false;
  }
  file.write(content.data(), static_cast<std::streamsize>(content.size()));
  file.flush();
  file.close();
  if (!file) {
    error = "cannot write file: " + path;
    return false;
  }
  return true;
}

}  // namespace

atlas::capabilities::ExecutionResult readDispatch(const atlas::capabilities::NativeRequest& request) {
  const std::string* path = nullptr;
  std::string error;
  if (!requirePath(request, &path, error)) {
    return failure(request.target, std::move(error));
  }

  std::int64_t offset = 0;
  if (const StructuredValue* offsetValue = argument(request, "offset"); offsetValue != nullptr) {
    const auto* offsetInteger = std::get_if<std::int64_t>(&offsetValue->value);
    if (offsetInteger == nullptr || *offsetInteger < 1 || *offsetInteger > std::numeric_limits<int>::max()) {
      return failure(request.target, "field 'offset' must be an integer greater than or equal to 1");
    }
    offset = *offsetInteger;
  }

  std::int64_t limit = 0;
  if (const StructuredValue* limitValue = argument(request, "limit"); limitValue != nullptr) {
    const auto* limitInteger = std::get_if<std::int64_t>(&limitValue->value);
    if (limitInteger == nullptr || *limitInteger < 0) {
      return failure(request.target, "field 'limit' must be a non-negative integer");
    }
    limit = *limitInteger;
  }

  FileLines lines;
  if (!readLines(*path, lines, error)) {
    return failure(request.target, std::move(error));
  }

  std::size_t first = 0;
  std::size_t last = 0;
  if (!lineRange(lines, offset, limit, first, last, error)) {
    return failure(request.target, std::move(error));
  }

  atlas::capabilities::ExecutionResult result;
  result.target = request.target;
  result.status = atlas::capabilities::ExecutionStatus::success;
  result.output = StructuredValue::Object{
      {"path", *path},
      {"content", sliceContent(lines, first, last)},
      {"line_start", static_cast<std::int64_t>(first)},
      {"line_end", static_cast<std::int64_t>(last)},
      {"total_lines", static_cast<std::int64_t>(lines.line_starts.size())},
  };
  return result;
}

atlas::capabilities::ExecutionResult writeDispatch(const atlas::capabilities::NativeRequest& request) {
  const std::string* path = nullptr;
  std::string error;
  if (!requirePath(request, &path, error)) {
    return failure(request.target, std::move(error));
  }
  const std::string* content = stringField(request, "content");
  if (content == nullptr) {
    return failure(request.target, "field 'content' must be a string");
  }

  std::error_code code;
  if (std::filesystem::is_directory(*path, code)) {
    return failure(request.target, "path is a directory: " + *path);
  }
  if (!writeFile(*path, *content, error)) {
    return failure(request.target, std::move(error));
  }

  atlas::capabilities::ExecutionResult result;
  result.target = request.target;
  result.status = atlas::capabilities::ExecutionStatus::success;
  result.output = StructuredValue::Object{
      {"path", *path},
      {"bytes", static_cast<std::int64_t>(content->size())},
  };
  return result;
}

atlas::capabilities::ExecutionResult editDispatch(const atlas::capabilities::NativeRequest& request) {
  const std::string* path = nullptr;
  std::string error;
  if (!requirePath(request, &path, error)) {
    return failure(request.target, std::move(error));
  }

  const StructuredValue* replacementsValue = argument(request, "replacements");
  const auto* replacements = replacementsValue == nullptr
      ? nullptr
      : std::get_if<StructuredValue::Array>(&replacementsValue->value);
  if (replacements == nullptr) {
    return failure(request.target, "field 'replacements' must be an array of objects");
  }

  struct Replacement {
    std::string old_string;
    std::string new_string;
  };
  std::vector<Replacement> parsed;
  for (const StructuredValue& item : *replacements) {
    const auto* object = std::get_if<StructuredValue::Object>(&item.value);
    if (object == nullptr) {
      return failure(
          request.target,
          "field 'replacements' must contain objects with 'old_string' and 'new_string'");
    }

    Replacement replacement;
    if (const auto oldIterator = object->find("old_string"); oldIterator != object->end()) {
      if (const auto* value = std::get_if<std::string>(&oldIterator->second.value); value != nullptr) {
        replacement.old_string = *value;
      }
    }
    if (const auto newIterator = object->find("new_string"); newIterator != object->end()) {
      if (const auto* value = std::get_if<std::string>(&newIterator->second.value); value != nullptr) {
        replacement.new_string = *value;
      }
    }
    if (replacement.old_string.empty()) {
      return failure(request.target, "field 'old_string' must be a non-empty string");
    }
    parsed.push_back(std::move(replacement));
  }

  FileLines lines;
  if (!readLines(*path, lines, error)) {
    return failure(request.target, std::move(error));
  }

  std::string edited = std::move(lines.content);
  for (std::size_t index = 0; index < parsed.size(); ++index) {
    const Replacement& item = parsed[index];
    std::size_t occurrences = 0;
    for (std::size_t position = edited.find(item.old_string);
         position != std::string::npos;
         position = edited.find(item.old_string, position + item.old_string.size())) {
      ++occurrences;
    }
    if (occurrences == 0) {
      return failure(
          request.target,
          "replacement " + std::to_string(index + 1) +
              ": old_string not found in " + *path);
    }
    if (occurrences > 1) {
      return failure(
          request.target,
          "replacement " + std::to_string(index + 1) + ": old_string matches " +
              std::to_string(occurrences) + " occurrences; expected exactly one");
    }
    const std::size_t position = edited.find(item.old_string);
    edited.replace(position, item.old_string.size(), item.new_string);
  }

  if (!writeFile(*path, edited, error)) {
    return failure(request.target, std::move(error));
  }

  const std::int64_t applied = static_cast<std::int64_t>(parsed.size());

  atlas::capabilities::ExecutionResult result;
  result.target = request.target;
  result.status = atlas::capabilities::ExecutionStatus::success;
  result.output = StructuredValue::Object{
      {"path", *path},
      {"replacements", applied},
  };
  return result;
}

atlas::capabilities::ExecutionResult listDispatch(const atlas::capabilities::NativeRequest& request) {
  const std::string* path = nullptr;
  std::string error;
  if (!requirePath(request, &path, error)) {
    return failure(request.target, std::move(error));
  }

  std::error_code code;
  if (!std::filesystem::is_directory(*path, code)) {
    return failure(request.target, "path is not a directory: " + *path);
  }

  std::vector<std::filesystem::directory_entry> collected;
  for (const std::filesystem::directory_entry& entry :
       std::filesystem::directory_iterator(*path, std::filesystem::directory_options::none, code)) {
    collected.push_back(entry);
  }
  if (code) {
    return failure(request.target, "cannot list directory '" + *path + "': " + code.message());
  }
  std::sort(collected.begin(), collected.end(), [](const auto& left, const auto& right) {
    return left.path() < right.path();
  });

  StructuredValue::Array entries;
  for (const std::filesystem::directory_entry& entry : collected) {
    std::error_code entryError;
    const bool directory = entry.is_directory(entryError) && !entryError;
    const char* type = directory ? "directory" : entry.is_regular_file(entryError) && !entryError ? "file" : "other";
    const auto size = static_cast<std::int64_t>(std::filesystem::file_size(entry.path(), entryError));
    StructuredValue::Object item{
        {"name", entry.path().filename().string()},
        {"type", type},
        {"size_bytes", entryError ? static_cast<std::int64_t>(0) : size},
    };
    entries.push_back(StructuredValue(std::move(item)));
  }

  atlas::capabilities::ExecutionResult result;
  result.target = request.target;
  result.status = atlas::capabilities::ExecutionStatus::success;
  result.output = StructuredValue::Object{
      {"path", *path},
      {"entries", std::move(entries)},
  };
  return result;
}

namespace {

// Contexto da varredura: mantem o limite, as regras de ignore e o acumulador.
struct SearchState {
  const std::string* query = nullptr;
  std::int64_t limit = 0;
  // A varredura usa a raiz absoluta; a saida preserva o caminho informado.
  std::filesystem::path absoluteRoot;
  std::filesystem::path givenRoot;
  StructuredValue::Array matches;
  bool truncated = false;
  std::int64_t collected = 0;

  bool accept() {
    if (collected >= limit) {
      truncated = true;
      return false;
    }
    ++collected;
    return true;
  }

  // Reporta o mesmo caminho que o chamador informou como raiz.
  std::string reportedPath(const std::filesystem::path& entry) const {
    return (givenRoot / entry.lexically_relative(absoluteRoot)).lexically_normal().string();
  }
};

// Percorre um diretorio em ordem determinista, podando o que for ignorado.
void searchDirectory(const std::filesystem::path& directory, SearchIgnores& ignores, SearchState& state) {
  const std::size_t mark = ignores.mark();
  // Um .gitignore local vale apenas para a subarvore visitada.
  ignores.loadDirectory(directory);

  std::error_code code;
  std::vector<std::filesystem::directory_entry> entries;
  for (const std::filesystem::directory_entry& entry :
       std::filesystem::directory_iterator(
           directory, std::filesystem::directory_options::skip_permission_denied, code)) {
    if (code) {
      code.clear();
      continue;
    }
    entries.push_back(entry);
  }
  std::sort(entries.begin(), entries.end(), [](const auto& left, const auto& right) {
    return left.path() < right.path();
  });

  for (const std::filesystem::directory_entry& entry : entries) {
    if (state.truncated) {
      break;
    }

    std::error_code entryError;
    const bool isDirectory = entry.is_directory(entryError) && !entryError;
    const bool isFile = entry.is_regular_file(entryError) && !entryError;
    if (ignores.ignores(entry.path(), isDirectory)) {
      continue;
    }
    if (isDirectory) {
      searchDirectory(entry.path(), ignores, state);
      continue;
    }
    if (!isFile) {
      continue;
    }

    const std::string entryPath = state.reportedPath(entry.path());
    if (entry.path().filename().string().find(*state.query) != std::string::npos &&
        state.accept()) {
      state.matches.push_back(StructuredValue::Object{
          {"path", entryPath},
          {"kind", "file"},
      });
      if (state.truncated) {
        break;
      }
    }

    FileLines lines;
    std::string readError;
    // Arquivos ilegiveis sao ignorados; a busca continua deterministica.
    if (!readLines(entry.path().string(), lines, readError)) {
      continue;
    }
    for (std::size_t index = 0; index < lines.line_starts.size(); ++index) {
      const std::string text = lineText(lines, index);
      if (text.find(*state.query) == std::string::npos) {
        continue;
      }
      if (!state.accept()) {
        break;
      }
      state.matches.push_back(StructuredValue::Object{
          {"path", entryPath},
          {"kind", "line"},
          {"line_number", static_cast<std::int64_t>(index + 1)},
          {"line", text},
      });
      if (state.truncated) {
        break;
      }
    }
  }

  ignores.restore(mark);
}

}  // namespace

atlas::capabilities::ExecutionResult searchDispatch(const atlas::capabilities::NativeRequest& request) {
  const std::string* path = nullptr;
  std::string error;
  if (!requirePath(request, &path, error)) {
    return failure(request.target, std::move(error));
  }
  const std::string* query = stringField(request, "query");
  if (query == nullptr) {
    return failure(request.target, "field 'query' must be a non-empty string");
  }

  std::int64_t limit = kDefaultSearchLimit;
  if (const StructuredValue* limitValue = argument(request, "max_results"); limitValue != nullptr) {
    const auto* limitInteger = std::get_if<std::int64_t>(&limitValue->value);
    if (limitInteger == nullptr || *limitInteger < 1) {
      return failure(request.target, "field 'max_results' must be an integer greater than or equal to 1");
    }
    limit = *limitInteger;
  }

  std::error_code code;
  if (!std::filesystem::is_directory(*path, code)) {
    return failure(request.target, "path is not a directory: " + *path);
  }

  const std::filesystem::path root(*path);
  const std::filesystem::path absoluteRoot = std::filesystem::absolute(root).lexically_normal();
  SearchIgnores ignores;
  ignores.loadDefaults(absoluteRoot);
  ignores.loadAncestors(absoluteRoot);

  SearchState state;
  state.query = query;
  state.limit = limit;
  state.givenRoot = root;
  state.absoluteRoot = absoluteRoot;
  searchDirectory(state.absoluteRoot, ignores, state);

  atlas::capabilities::ExecutionResult result;
  result.target = request.target;
  result.status = atlas::capabilities::ExecutionStatus::success;
  result.output = StructuredValue::Object{
      {"path", *path},
      {"query", *query},
      {"matches", std::move(state.matches)},
      {"total_matches", state.collected},
      {"truncated", state.truncated},
  };
  return result;
}

namespace {

// Contexto da busca por glob: padrao, limite e caminhos acumulados.
struct GlobState {
  const std::string* pattern = nullptr;
  // Um padrao com '/' casa o caminho relativo; sem '/', casa o nome do arquivo.
  bool matchRelativePath = false;
  std::filesystem::path absoluteRoot;
  std::filesystem::path givenRoot;
  std::vector<std::string> matches;
  std::int64_t limit = 0;
  std::int64_t collected = 0;
  bool truncated = false;
};

// Percorre um diretorio em ordem determinista, podando o que for ignorado.
void globDirectory(
    const std::filesystem::path& directory,
    SearchIgnores& ignores,
    GlobState& state) {
  const std::size_t mark = ignores.mark();
  ignores.loadDirectory(directory);

  std::error_code code;
  std::vector<std::filesystem::directory_entry> entries;
  for (const std::filesystem::directory_entry& entry :
       std::filesystem::directory_iterator(
           directory, std::filesystem::directory_options::skip_permission_denied, code)) {
    if (code) {
      code.clear();
      continue;
    }
    entries.push_back(entry);
  }
  std::sort(entries.begin(), entries.end(), [](const auto& left, const auto& right) {
    return left.path() < right.path();
  });

  for (const std::filesystem::directory_entry& entry : entries) {
    if (state.truncated) {
      break;
    }

    std::error_code entryError;
    const bool isDirectory = entry.is_directory(entryError) && !entryError;
    const bool isFile = entry.is_regular_file(entryError) && !entryError;
    if (ignores.ignores(entry.path(), isDirectory)) {
      continue;
    }
    if (isDirectory) {
      globDirectory(entry.path(), ignores, state);
      continue;
    }
    if (!isFile) {
      continue;
    }

    const std::filesystem::path relative = entry.path().lexically_relative(state.absoluteRoot);
    const std::string candidate =
        state.matchRelativePath ? relative.generic_string() : entry.path().filename().string();
    if (!globMatch(*state.pattern, candidate)) {
      continue;
    }
    if (state.collected >= state.limit) {
      state.truncated = true;
      break;
    }
    ++state.collected;
    state.matches.push_back(
        (state.givenRoot / relative).lexically_normal().string());
  }

  ignores.restore(mark);
}

}  // namespace

atlas::capabilities::ExecutionResult globDispatch(
    const atlas::capabilities::NativeRequest& request) {
  const std::string* path = nullptr;
  std::string error;
  if (!requirePath(request, &path, error)) {
    return failure(request.target, std::move(error));
  }
  const std::string* pattern = stringField(request, "pattern");
  if (pattern == nullptr || pattern->empty()) {
    return failure(request.target, "field 'pattern' must be a non-empty string");
  }

  std::int64_t limit = kDefaultSearchLimit;
  if (const StructuredValue* limitValue = argument(request, "max_results"); limitValue != nullptr) {
    const auto* limitInteger = std::get_if<std::int64_t>(&limitValue->value);
    if (limitInteger == nullptr || *limitInteger < 1) {
      return failure(request.target, "field 'max_results' must be an integer greater than or equal to 1");
    }
    limit = *limitInteger;
  }

  std::error_code code;
  if (!std::filesystem::is_directory(*path, code)) {
    return failure(request.target, "path is not a directory: " + *path);
  }

  const std::filesystem::path root(*path);
  const std::filesystem::path absoluteRoot = std::filesystem::absolute(root).lexically_normal();
  SearchIgnores ignores;
  ignores.loadDefaults(absoluteRoot);
  ignores.loadAncestors(absoluteRoot);

  GlobState state;
  state.pattern = pattern;
  state.matchRelativePath = pattern->find('/') != std::string::npos;
  state.absoluteRoot = absoluteRoot;
  state.givenRoot = root;
  state.limit = limit;
  globDirectory(absoluteRoot, ignores, state);

  std::sort(state.matches.begin(), state.matches.end());

  StructuredValue::Array matches;
  matches.reserve(state.matches.size());
  for (const std::string& match : state.matches) {
    matches.emplace_back(match);
  }

  atlas::capabilities::ExecutionResult result;
  result.target = request.target;
  result.status = atlas::capabilities::ExecutionStatus::success;
  result.output = StructuredValue::Object{
      {"path", *path},
      {"pattern", *pattern},
      {"matches", std::move(matches)},
      {"total_matches", state.collected},
      {"truncated", state.truncated},
  };
  return result;
}

namespace {

// Linha de um hunk de patch, classificada por contexto/remocao/adicao.
struct PatchHunkLine {
  char type = ' ';
  std::string text;
};

struct PatchHunk {
  std::vector<PatchHunkLine> lines;
};

enum class PatchAction {
  add,
  update,
  remove,
  move,
};

struct PatchOperation {
  PatchAction action = PatchAction::add;
  std::string path;
  std::string move_to;
  std::vector<std::string> lines;
  std::vector<PatchHunk> hunks;
};

bool startsWith(std::string_view value, std::string_view prefix) {
  return value.size() >= prefix.size() && value.substr(0, prefix.size()) == prefix;
}

std::string trimSpaces(std::string_view value) {
  std::size_t start = 0;
  std::size_t end = value.size();
  while (start < end && (value[start] == ' ' || value[start] == '\t')) {
    ++start;
  }
  while (end > start && (value[end - 1] == ' ' || value[end - 1] == '\t')) {
    --end;
  }
  return std::string(value.substr(start, end - start));
}

// Divide o texto em linhas, tolerando CRLF e sem criar linha vazia final.
std::vector<std::string> splitPatchLines(std::string_view text) {
  std::vector<std::string> lines;
  std::size_t position = 0;
  while (position <= text.size()) {
    const std::size_t newline = text.find('\n', position);
    if (newline == std::string_view::npos) {
      if (position < text.size()) {
        std::string line(text.substr(position));
        if (!line.empty() && line.back() == '\r') {
          line.pop_back();
        }
        lines.push_back(std::move(line));
      }
      break;
    }
    std::string line(text.substr(position, newline - position));
    if (!line.empty() && line.back() == '\r') {
      line.pop_back();
    }
    lines.push_back(std::move(line));
    position = newline + 1;
  }
  return lines;
}

bool sectionValue(const std::string& line, std::string_view prefix, std::string& value) {
  if (!startsWith(line, prefix)) {
    return false;
  }
  value = trimSpaces(std::string_view(line).substr(prefix.size()));
  return true;
}

bool parsePatch(
    const std::string& text,
    std::vector<PatchOperation>& operations,
    std::string& error) {
  const std::vector<std::string> lines = splitPatchLines(text);
  if (lines.empty() || lines.front() != "*** Begin Patch") {
    error = "patch must start with '*** Begin Patch'";
    return false;
  }
  if (lines.back() != "*** End Patch") {
    error = "patch must end with '*** End Patch'";
    return false;
  }

  const std::size_t bodyEnd = lines.size() - 1;
  std::size_t index = 1;
  while (index < bodyEnd) {
    const std::string& line = lines[index];
    std::string path;

    if (sectionValue(line, "*** Add File: ", path)) {
      if (path.empty()) {
        error = "add section requires a path";
        return false;
      }
      PatchOperation operation;
      operation.action = PatchAction::add;
      operation.path = path;
      ++index;
      while (index < bodyEnd && !startsWith(lines[index], "*** ")) {
        const std::string& content = lines[index];
        if (content.empty() || content.front() != '+') {
          error = "add lines must start with '+' in section '" + path + "'";
          return false;
        }
        operation.lines.push_back(content.substr(1));
        ++index;
      }
      operations.push_back(std::move(operation));
      continue;
    }

    if (sectionValue(line, "*** Update File: ", path)) {
      if (path.empty()) {
        error = "update section requires a path";
        return false;
      }
      PatchOperation operation;
      operation.action = PatchAction::update;
      operation.path = path;
      ++index;
      std::string moveTo;
      if (index < bodyEnd && sectionValue(lines[index], "*** Move to: ", moveTo)) {
        if (moveTo.empty()) {
          error = "move section requires a path";
          return false;
        }
        operation.move_to = moveTo;
        operation.action = PatchAction::move;
        ++index;
      }
      while (index < bodyEnd && !startsWith(lines[index], "*** ")) {
        const std::string& content = lines[index];
        if (content.empty()) {
          if (operation.hunks.empty()) {
            error = "expected '@@' before hunk lines in section '" + path + "'";
            return false;
          }
          operation.hunks.back().lines.push_back(PatchHunkLine{' ', std::string()});
          ++index;
          continue;
        }
        if (startsWith(content, "@@")) {
          operation.hunks.push_back(PatchHunk{});
          ++index;
          continue;
        }
        if (content.front() != ' ' && content.front() != '+' && content.front() != '-') {
          error = "hunk lines must start with ' ', '+' or '-' in section '" + path + "'";
          return false;
        }
        if (operation.hunks.empty()) {
          error = "expected '@@' before hunk lines in section '" + path + "'";
          return false;
        }
        operation.hunks.back().lines.push_back(PatchHunkLine{content.front(), content.substr(1)});
        ++index;
      }
      if (operation.hunks.empty() && operation.move_to.empty()) {
        error = "update section '" + path + "' must contain a hunk or a move";
        return false;
      }
      operations.push_back(std::move(operation));
      continue;
    }

    if (sectionValue(line, "*** Delete File: ", path)) {
      if (path.empty()) {
        error = "delete section requires a path";
        return false;
      }
      PatchOperation operation;
      operation.action = PatchAction::remove;
      operation.path = path;
      ++index;
      operations.push_back(std::move(operation));
      continue;
    }

    error = "unknown patch section: " + trimSpaces(line);
    return false;
  }

  if (operations.empty()) {
    error = "patch must contain at least one operation";
    return false;
  }
  return true;
}

// Texto em linhas, preservando se o arquivo termina com newline.
struct PatchText {
  std::vector<std::string> lines;
  bool trailingNewline = false;
};

PatchText splitText(const std::string& content) {
  PatchText text;
  std::size_t position = 0;
  while (position < content.size()) {
    const std::size_t newline = content.find('\n', position);
    if (newline == std::string::npos) {
      text.lines.push_back(content.substr(position));
      break;
    }
    text.lines.push_back(content.substr(position, newline - position));
    position = newline + 1;
  }
  text.trailingNewline = !content.empty() && content.back() == '\n';
  return text;
}

std::string joinText(const PatchText& text) {
  std::string result;
  for (std::size_t index = 0; index < text.lines.size(); ++index) {
    if (index != 0) {
      result.push_back('\n');
    }
    result += text.lines[index];
  }
  if (text.trailingNewline && !text.lines.empty()) {
    result.push_back('\n');
  }
  return result;
}

bool applyHunks(
    const std::string& content,
    const std::vector<PatchHunk>& hunks,
    std::string& output,
    std::string& error) {
  PatchText text = splitText(content);
  std::size_t cursor = 0;

  for (std::size_t hunkIndex = 0; hunkIndex < hunks.size(); ++hunkIndex) {
    const PatchHunk& hunk = hunks[hunkIndex];
    std::vector<std::string> oldLines;
    std::vector<std::string> newLines;
    for (const PatchHunkLine& line : hunk.lines) {
      if (line.type != '+') {
        oldLines.push_back(line.text);
      }
      if (line.type != '-') {
        newLines.push_back(line.text);
      }
    }

    std::size_t position = cursor;
    bool found = oldLines.empty();
    if (!found && oldLines.size() <= text.lines.size()) {
      for (std::size_t start = cursor; start + oldLines.size() <= text.lines.size(); ++start) {
        const auto begin = text.lines.begin() + static_cast<std::ptrdiff_t>(start);
        if (std::equal(oldLines.begin(), oldLines.end(), begin)) {
          position = start;
          found = true;
          break;
        }
      }
    }
    if (!found) {
      error = "hunk " + std::to_string(hunkIndex + 1) + " context not found";
      return false;
    }

    const auto positionIterator = text.lines.begin() + static_cast<std::ptrdiff_t>(position);
    text.lines.erase(
        positionIterator,
        positionIterator + static_cast<std::ptrdiff_t>(oldLines.size()));
    text.lines.insert(
        text.lines.begin() + static_cast<std::ptrdiff_t>(position),
        newLines.begin(),
        newLines.end());
    cursor = position + newLines.size();
  }

  output = joinText(text);
  return true;
}

// Modela os arquivos alterados antes de tocar no disco.
struct PatchWorkspace {
  std::map<std::string, std::string, std::less<>> contents;
  std::set<std::string, std::less<>> removed;
};

std::string normalizePath(const std::string& path) {
  std::error_code code;
  const std::filesystem::path absolute = std::filesystem::absolute(path, code);
  return (code ? std::filesystem::path(path) : absolute).lexically_normal().string();
}

// Resolve o conteudo atual considerando adicoes e remocoes ja simuladas.
bool readWorkspace(
    const PatchWorkspace& workspace,
    const std::string& path,
    bool& exists,
    std::string& content,
    std::string& error) {
  if (const auto iterator = workspace.contents.find(path); iterator != workspace.contents.end()) {
    exists = true;
    content = iterator->second;
    return true;
  }
  if (workspace.removed.contains(path)) {
    exists = false;
    return true;
  }

  std::error_code code;
  const std::filesystem::path file(path);
  if (!std::filesystem::exists(file, code)) {
    exists = false;
    return true;
  }
  if (!std::filesystem::is_regular_file(file, code)) {
    error = "path is not a regular file: " + path;
    return false;
  }

  std::ifstream stream(file, std::ios::binary);
  if (!stream) {
    error = "cannot open file: " + path;
    return false;
  }
  content.assign(std::istreambuf_iterator<char>(stream), std::istreambuf_iterator<char>());
  if (stream.bad()) {
    error = "cannot read file: " + path;
    return false;
  }
  exists = true;
  return true;
}

bool commitWorkspace(const PatchWorkspace& workspace, std::string& error) {
  for (const auto& [path, content] : workspace.contents) {
    std::error_code code;
    const std::filesystem::path parent = std::filesystem::path(path).parent_path();
    if (!parent.empty()) {
      std::filesystem::create_directories(parent, code);
      if (code) {
        error = "cannot create directory '" + parent.string() + "': " + code.message();
        return false;
      }
    }
    if (!writeFile(path, content, error)) {
      return false;
    }
  }
  for (const std::string& path : workspace.removed) {
    std::error_code code;
    const std::filesystem::path file(path);
    if (!std::filesystem::exists(file, code)) {
      continue;
    }
    std::filesystem::remove(file, code);
    if (code) {
      error = "cannot remove file '" + path + "': " + code.message();
      return false;
    }
  }
  return true;
}

StructuredValue patchChange(std::string path, std::string action) {
  return StructuredValue(StructuredValue::Object{
      {"path", std::move(path)},
      {"action", std::move(action)},
  });
}

}  // namespace

atlas::capabilities::ExecutionResult patchDispatch(
    const atlas::capabilities::NativeRequest& request) {
  const std::string* patch = stringField(request, "patch");
  if (patch == nullptr) {
    return failure(request.target, "field 'patch' must be a string");
  }

  std::vector<PatchOperation> operations;
  std::string error;
  if (!parsePatch(*patch, operations, error)) {
    return failure(request.target, "invalid patch: " + error);
  }

  PatchWorkspace workspace;
  StructuredValue::Array changes;
  std::int64_t added = 0;
  std::int64_t updated = 0;
  std::int64_t deleted = 0;
  std::int64_t moved = 0;

  for (const PatchOperation& operation : operations) {
    const std::string path = normalizePath(operation.path);
    bool exists = false;
    std::string content;
    if (!readWorkspace(workspace, path, exists, content, error)) {
      return failure(request.target, std::move(error));
    }

    if (operation.action == PatchAction::add) {
      if (exists) {
        return failure(request.target, "cannot add file that already exists: " + operation.path);
      }
      std::string created;
      for (std::size_t index = 0; index < operation.lines.size(); ++index) {
        if (index != 0) {
          created.push_back('\n');
        }
        created += operation.lines[index];
      }
      if (!operation.lines.empty()) {
        created.push_back('\n');
      }
      workspace.removed.erase(path);
      workspace.contents[path] = std::move(created);
      changes.push_back(patchChange(operation.path, "add"));
      ++added;
      continue;
    }

    if (operation.action == PatchAction::remove) {
      if (!exists) {
        return failure(request.target, "cannot delete file that does not exist: " + operation.path);
      }
      workspace.contents.erase(path);
      workspace.removed.insert(path);
      changes.push_back(patchChange(operation.path, "delete"));
      ++deleted;
      continue;
    }

    if (!exists) {
      return failure(request.target, "cannot update file that does not exist: " + operation.path);
    }
    std::string edited;
    if (!applyHunks(content, operation.hunks, edited, error)) {
      return failure(request.target, "update '" + operation.path + "': " + std::move(error));
    }

    if (operation.move_to.empty()) {
      workspace.contents[path] = std::move(edited);
      changes.push_back(patchChange(operation.path, "update"));
      ++updated;
      continue;
    }

    const std::string target = normalizePath(operation.move_to);
    if (target == path) {
      return failure(request.target, "move target equals source: " + operation.path);
    }
    bool targetExists = false;
    std::string targetContent;
    if (!readWorkspace(workspace, target, targetExists, targetContent, error)) {
      return failure(request.target, std::move(error));
    }
    if (targetExists) {
      return failure(request.target, "cannot move onto existing file: " + operation.move_to);
    }
    workspace.contents.erase(path);
    workspace.removed.insert(path);
    workspace.removed.erase(target);
    workspace.contents[target] = std::move(edited);
    changes.push_back(StructuredValue(StructuredValue::Object{
        {"path", operation.path},
        {"action", "move"},
        {"moved_to", operation.move_to},
    }));
    ++moved;
  }

  if (!commitWorkspace(workspace, error)) {
    return failure(request.target, std::move(error));
  }

  atlas::capabilities::ExecutionResult result;
  result.target = request.target;
  result.status = atlas::capabilities::ExecutionStatus::success;
  result.output = StructuredValue::Object{
      {"changes", std::move(changes)},
      {"added", added},
      {"updated", updated},
      {"deleted", deleted},
      {"moved", moved},
  };
  return result;
}

}  // namespace atlas::capabilities::tools::filesystem
