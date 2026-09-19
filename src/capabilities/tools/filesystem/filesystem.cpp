#include "filesystem.hpp"

#include "search/ignore.hpp"

#include <algorithm>
#include <filesystem>
#include <fstream>
#include <iterator>
#include <limits>
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

}  // namespace atlas::capabilities::tools::filesystem
