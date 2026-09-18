#include "../../src/capabilities/tools/filesystem/filesystem.hpp"

#include "../../src/capabilities/core/discovery.hpp"
#include "../../src/capabilities/core/executor.hpp"
#include "../../src/capabilities/core/loader.hpp"
#include "../../src/capabilities/core/registry.hpp"

#include <algorithm>
#include <cstdlib>
#include <filesystem>
#include <fstream>
#include <iostream>
#include <string>
#include <utility>

#include <unistd.h>

namespace {

using atlas::capabilities::Discovery;
using atlas::capabilities::ExecutionResult;
using atlas::capabilities::ExecutionStatus;
using atlas::capabilities::Executor;
using atlas::capabilities::Loader;
using atlas::capabilities::Registry;
using atlas::capabilities::StructuredArguments;
using atlas::capabilities::StructuredValue;
namespace filesystem = atlas::capabilities::tools::filesystem;

void require(bool condition, std::string_view message) {
  if (!condition) {
    std::cerr << "filesystem test failed: " << message << '\n';
    std::exit(EXIT_FAILURE);
  }
}

// Arvore temporaria com arquivos fixos para cada operacao.
struct TempTree {
  std::filesystem::path root;

  static TempTree create() {
    static int counter = 0;
    TempTree tree{
        .root = std::filesystem::temp_directory_path() /
            ("atlas-filesystem-test-" + std::to_string(::getpid()) + "-" + std::to_string(counter++))};
    std::error_code code;
    std::filesystem::remove_all(tree.root, code);
    std::filesystem::create_directories(tree.root / "nested", code);
    writeFile(tree.root / "hello.txt", "first line\nsecond line\nthird line\n");
    writeFile(tree.root / "nested" / "other.txt", "alpha\nshared token here\n");
    return tree;
  }

  static void writeFile(const std::filesystem::path& path, std::string_view content) {
    std::ofstream file(path, std::ios::binary);
    file.write(content.data(), static_cast<std::streamsize>(content.size()));
  }

  std::string readFile(const std::filesystem::path& path) const {
    std::ifstream file(path, std::ios::binary);
    return std::string{
        std::istreambuf_iterator<char>(file),
        std::istreambuf_iterator<char>()};
  }

  std::string file(const std::string_view name) const {
    return (root / name).string();
  }

  ~TempTree() {
    std::error_code code;
    std::filesystem::remove_all(root, code);
  }
};

StructuredArguments arguments(std::initializer_list<std::pair<const std::string, StructuredValue>> fields) {
  return StructuredArguments(fields);
}

const StructuredValue* outputField(const ExecutionResult& result, std::string_view name) {
  const auto* output = std::get_if<StructuredValue::Object>(&result.output.value);
  if (output == nullptr) {
    return nullptr;
  }
  const auto iterator = output->find(name);
  return iterator == output->end() ? nullptr : &iterator->second;
}

const std::string* stringOutput(const ExecutionResult& result, std::string_view name) {
  const StructuredValue* field = outputField(result, name);
  return field == nullptr ? nullptr : std::get_if<std::string>(&field->value);
}

const std::int64_t* integerOutput(const ExecutionResult& result, std::string_view name) {
  const StructuredValue* field = outputField(result, name);
  return field == nullptr ? nullptr : std::get_if<std::int64_t>(&field->value);
}

const StructuredValue::Array* arrayOutput(const ExecutionResult& result, std::string_view name) {
  const StructuredValue* field = outputField(result, name);
  return field == nullptr ? nullptr : std::get_if<StructuredValue::Array>(&field->value);
}

const StructuredValue* matchField(const StructuredValue& value, std::string_view name) {
  const auto* object = std::get_if<StructuredValue::Object>(&value.value);
  if (object == nullptr) {
    return nullptr;
  }
  const auto iterator = object->find(name);
  return iterator == object->end() ? nullptr : &iterator->second;
}

void testLoadingAndDiscovery() {
  Registry registry;
  Loader loader(registry);
  Discovery discovery(registry);

  require(
      loader.scan("src/capabilities/tools/filesystem"),
      "filesystem group and tools should load from their manifests");
  require(registry.get("filesystem").has_value(), "filesystem group should be registered");

  for (const std::string_view id :
       {"filesystem.read", "filesystem.write", "filesystem.edit", "filesystem.list", "filesystem.search"}) {
    const auto registered = registry.get(id);
    require(registered.has_value(), std::string(id).append(" should be registered"));
    require(registered->parent == "filesystem", std::string(id).append(" should belong to the filesystem group"));
    require(
        registered->implementation.kind == "executable",
        std::string(id).append(" should have an executable implementation"));
    require(
        registered->implementation.entrypoint.find("runtime") != std::string::npos,
        std::string(id).append(" should resolve its runtime relative to the manifest"));
  }

  const auto discoverable = discovery.discover();
  for (const std::string_view id :
       {"filesystem.read", "filesystem.write", "filesystem.edit", "filesystem.list", "filesystem.search"}) {
    const bool found = std::any_of(
        discoverable.begin(),
        discoverable.end(),
        [id](const auto& item) { return item.id == id; });
    require(found, std::string(id).append(" should be discoverable"));
  }
}

void testReadThroughExecutor(const TempTree& tree) {
  Registry registry;
  Loader loader(registry);
  require(
      loader.load("src/capabilities/tools/filesystem/read/capability.json"),
      "filesystem.read manifest should load in isolation");

  Executor executor(registry);
  const ExecutionResult result = executor.execute(
      "filesystem.read",
      "local",
      arguments({{"path", StructuredValue(tree.file("hello.txt"))}}));
  require(result.status == ExecutionStatus::success, "filesystem.read should execute through the Executor");
  require(result.target == "local", "Executor should preserve the target");
  require(result.error.empty(), "successful execution should not have an error");
  const auto* content = stringOutput(result, "content");
  require(content != nullptr && !content->empty(), "filesystem.read should expose content");
  const auto* totalLines = integerOutput(result, "total_lines");
  require(totalLines != nullptr && *totalLines == 3, "filesystem.read should expose total_lines");
}

void testRead(const TempTree& tree) {
  const std::string path = tree.file("hello.txt");
  const ExecutionResult full = filesystem::readDispatch(
      {"local", arguments({{"path", StructuredValue(path)}})});
  require(full.status == ExecutionStatus::success, "filesystem.read should succeed");
  const auto* content = stringOutput(full, "content");
  require(
      content != nullptr && *content == "first line\nsecond line\nthird line\n",
      "filesystem.read should return the exact file content");

  const ExecutionResult range = filesystem::readDispatch(
      {"local", arguments({{"path", StructuredValue(path)}, {"offset", std::int64_t{2}}, {"limit", std::int64_t{1}}})});
  require(range.status == ExecutionStatus::success, "filesystem.read range should succeed");
  const auto* slice = stringOutput(range, "content");
  require(slice != nullptr && *slice == "second line\n", "filesystem.read should return the requested lines");
  const auto* lineEnd = integerOutput(range, "line_end");
  require(lineEnd != nullptr && *lineEnd == 2, "filesystem.read should report the last line read");

  const std::string missing = tree.file("missing.txt");
  const ExecutionResult absent = filesystem::readDispatch(
      {"local", arguments({{"path", StructuredValue(missing)}})});
  require(absent.status == ExecutionStatus::failed, "reading a missing file should fail");
  require(absent.error == "path does not exist: " + missing, "missing file error should be explicit");

  const ExecutionResult directory = filesystem::readDispatch(
      {"local", arguments({{"path", StructuredValue(tree.root.string())}})});
  require(directory.status == ExecutionStatus::failed, "reading a directory should fail");
  require(
      directory.error == "path is not a regular file: " + tree.root.string(),
      "directory read error should be explicit");

  const ExecutionResult beyond = filesystem::readDispatch(
      {"local", arguments({{"path", StructuredValue(path)}, {"offset", std::int64_t{9}}})});
  require(beyond.status == ExecutionStatus::failed, "offset beyond the end should fail");
  require(!beyond.error.empty(), "offset error should be explicit");

  const ExecutionResult noField = filesystem::readDispatch({"local", {}});
  require(noField.status == ExecutionStatus::failed, "missing path field should fail");
  require(noField.error == "field 'path' must be a non-empty string", "path field error should be explicit");

  const std::filesystem::path emptyPath = tree.root / "empty.txt";
  TempTree::writeFile(emptyPath, "");
  const ExecutionResult empty = filesystem::readDispatch(
      {"local", arguments({{"path", StructuredValue(emptyPath.string())}})});
  require(empty.status == ExecutionStatus::success, "empty file should read successfully");
  const auto* emptyContent = stringOutput(empty, "content");
  require(emptyContent != nullptr && emptyContent->empty(), "empty file should yield empty content");
  const auto* emptyTotal = integerOutput(empty, "total_lines");
  require(emptyTotal != nullptr && *emptyTotal == 0, "empty file should have zero lines");
}

void testWriteAndList(const TempTree& tree) {
  const std::filesystem::path created = tree.root / "created.txt";
  const ExecutionResult write = filesystem::writeDispatch(
      {"local", arguments({{"path", StructuredValue(created.string())}, {"content", std::string("alpha\nbeta")}})});
  require(write.status == ExecutionStatus::success, "filesystem.write should succeed");
  const auto* bytes = integerOutput(write, "bytes");
  require(bytes != nullptr && *bytes == 10, "filesystem.write should report the byte count");
  require(tree.readFile(created) == "alpha\nbeta", "filesystem.write should create the file");

  const ExecutionResult overwrite = filesystem::writeDispatch(
      {"local", arguments({{"path", StructuredValue(created.string())}, {"content", std::string()}})});
  require(overwrite.status == ExecutionStatus::success, "overwriting should succeed");
  require(tree.readFile(created).empty(), "overwriting should replace the content");

  const ExecutionResult writeDirectory = filesystem::writeDispatch(
      {"local", arguments({{"path", StructuredValue((tree.root / "nested").string())}, {"content", std::string()}})});
  require(writeDirectory.status == ExecutionStatus::failed, "writing over a directory should fail");
  require(
      writeDirectory.error == "path is a directory: " + (tree.root / "nested").string(),
      "directory write error should be explicit");

  const ExecutionResult list = filesystem::listDispatch(
      {"local", arguments({{"path", StructuredValue(tree.root.string())}})});
  require(list.status == ExecutionStatus::success, "filesystem.list should succeed");
  const StructuredValue::Array* entries = arrayOutput(list, "entries");
  require(entries != nullptr && entries->size() == 4, "filesystem.list should return four entries");

  const auto entryName = [](const StructuredValue& value) {
    const auto* object = std::get_if<StructuredValue::Object>(&value.value);
    if (object == nullptr) {
      return std::string();
    }
    const auto iterator = object->find("name");
    return iterator == object->end() ? std::string() : std::get<std::string>(iterator->second.value);
  };
  require(entryName(entries->front()) == "created.txt", "filesystem.list should sort entries by name");
  require(entryName((*entries)[1]) == "empty.txt", "filesystem.list should sort entries by name");
  require(entryName((*entries)[3]) == "nested", "filesystem.list should include directories");

  const std::string notADirectory = tree.file("hello.txt");
  const ExecutionResult listFile = filesystem::listDispatch(
      {"local", arguments({{"path", StructuredValue(notADirectory)}})});
  require(listFile.status == ExecutionStatus::failed, "listing a file should fail");
  require(
      listFile.error == "path is not a directory: " + notADirectory,
      "list error should be explicit");
}

void testEdit(const TempTree& tree) {
  const std::filesystem::path path = tree.root / "editable.txt";
  TempTree::writeFile(path, "one\nshared\nshared end\n");

  const auto replacement = [](std::string oldString, std::string newString) {
    return StructuredValue(StructuredValue::Object{
        {"old_string", std::move(oldString)},
        {"new_string", std::move(newString)},
    });
  };

  const ExecutionResult single = filesystem::editDispatch(
      {"local", arguments({{"path", StructuredValue(path.string())},
                           {"replacements", StructuredValue(StructuredValue::Array{replacement("one", "uno")})}})});
  require(single.status == ExecutionStatus::success, "filesystem.edit should replace an unique occurrence");
  require(tree.readFile(path) == "uno\nshared\nshared end\n", "filesystem.edit should write the edited content");

  const ExecutionResult ordered = filesystem::editDispatch(
      {"local", arguments({{"path", StructuredValue(path.string())},
                           {"replacements", StructuredValue(StructuredValue::Array{
                                replacement("shared\nshared end", "done")})}})});
  require(ordered.status == ExecutionStatus::success, "filesystem.edit should support multiline replacements");
  require(tree.readFile(path) == "uno\ndone\n", "filesystem.edit should preserve the rest of the file");

  const ExecutionResult ambiguous = filesystem::editDispatch(
      {"local", arguments({{"path", StructuredValue(path.string())},
                           {"replacements", StructuredValue(StructuredValue::Array{replacement("o", "x")})}})});
  require(ambiguous.status == ExecutionStatus::failed, "ambiguous replacement should fail");
  require(
      ambiguous.error == "replacement 1: old_string matches 2 occurrences; expected exactly one",
      "ambiguous replacement error should be explicit");

  const ExecutionResult absent = filesystem::editDispatch(
      {"local", arguments({{"path", StructuredValue(path.string())},
                           {"replacements", StructuredValue(StructuredValue::Array{replacement("nothing here", "x")})}})});
  require(absent.status == ExecutionStatus::failed, "replacement of absent text should fail");
  require(
      absent.error == "replacement 1: old_string not found in " + path.string(),
      "absent replacement error should be explicit");

  const ExecutionResult emptyOld = filesystem::editDispatch(
      {"local", arguments({{"path", StructuredValue(path.string())},
                           {"replacements", StructuredValue(StructuredValue::Array{replacement("", "x")})}})});
  require(emptyOld.status == ExecutionStatus::failed, "empty old_string should fail");
}

void testSearch(const TempTree& tree) {
  const ExecutionResult search = filesystem::searchDispatch(
      {"local", arguments({{"path", StructuredValue(tree.root.string())}, {"query", std::string("shared")}})});
  require(search.status == ExecutionStatus::success, "filesystem.search should succeed");
  const StructuredValue::Array* matches = arrayOutput(search, "matches");
  require(matches != nullptr && matches->size() == 1, "filesystem.search should find the line in nested files");
  const auto* matchPath = matchField(matches->front(), "path");
  require(
      matchPath != nullptr && std::get<std::string>(matchPath->value) == (tree.root / "nested/other.txt").string(),
      "filesystem.search should report the file path");
  const auto* matchKind = matchField(matches->front(), "kind");
  require(matchKind != nullptr && std::get<std::string>(matchKind->value) == "line", "line matches kind should be line");

  const ExecutionResult fileSearch = filesystem::searchDispatch(
      {"local", arguments({{"path", StructuredValue(tree.root.string())}, {"query", std::string("other")}})});
  require(fileSearch.status == ExecutionStatus::success, "filesystem.search on file names should succeed");
  const StructuredValue::Array* fileMatches = arrayOutput(fileSearch, "matches");
  require(fileMatches != nullptr && !fileMatches->empty(), "filesystem.search should match file names");
  require(
      std::get<std::string>(matchField(fileMatches->front(), "kind")->value) == "file",
      "file name matches should be reported as kind file");

  const std::string absentRoot = tree.file("not-here");
  const ExecutionResult absent = filesystem::searchDispatch(
      {"local", arguments({{"path", StructuredValue(absentRoot)}, {"query", std::string("x")}})});
  require(absent.status == ExecutionStatus::failed, "search on a missing directory should fail");
  require(absent.error == "path is not a directory: " + absentRoot, "search error should be explicit");

  const ExecutionResult limited = filesystem::searchDispatch(
      {"local", arguments({{"path", StructuredValue(tree.root.string())}, {"query", std::string("e")}, {"max_results", std::int64_t{1}}})});
  require(limited.status == ExecutionStatus::success, "limited search should succeed");
  const auto* truncated = std::get_if<bool>(&outputField(limited, "truncated")->value);
  require(truncated != nullptr && *truncated, "limited search should report truncation");
}

}  // namespace

int main() {
  const TempTree tree = TempTree::create();
  testLoadingAndDiscovery();
  testReadThroughExecutor(tree);
  testRead(tree);
  testWriteAndList(tree);
  testEdit(tree);
  testSearch(tree);
  return EXIT_SUCCESS;
}
