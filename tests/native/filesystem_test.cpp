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
#include <vector>

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
        {"filesystem.read", "filesystem.list", "filesystem.search", "filesystem.glob", "filesystem.patch"}) {
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
        {"filesystem.read", "filesystem.list", "filesystem.search", "filesystem.glob", "filesystem.patch"}) {
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

void testList(const TempTree& tree) {
  const ExecutionResult list = filesystem::listDispatch(
      {"local", arguments({{"path", StructuredValue(tree.root.string())}})});
  require(list.status == ExecutionStatus::success, "filesystem.list should succeed");
  const StructuredValue::Array* entries = arrayOutput(list, "entries");
  require(entries != nullptr && entries->size() == 3, "filesystem.list should return three entries");

  const auto entryName = [](const StructuredValue& value) {
    const auto* object = std::get_if<StructuredValue::Object>(&value.value);
    if (object == nullptr) {
      return std::string();
    }
    const auto iterator = object->find("name");
    return iterator == object->end() ? std::string() : std::get<std::string>(iterator->second.value);
  };
  require(entryName(entries->front()) == "empty.txt", "filesystem.list should sort entries by name");
  require(entryName((*entries)[1]) == "hello.txt", "filesystem.list should sort entries by name");
  require(entryName((*entries)[2]) == "nested", "filesystem.list should include directories");

  const std::string notADirectory = tree.file("hello.txt");
  const ExecutionResult listFile = filesystem::listDispatch(
      {"local", arguments({{"path", StructuredValue(notADirectory)}})});
  require(listFile.status == ExecutionStatus::failed, "listing a file should fail");
  require(
      listFile.error == "path is not a directory: " + notADirectory,
      "list error should be explicit");
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

// Regressao do benchmark: artefatos de build e .gitignore nao entram na busca.
void testSearchIgnores() {
  TempTree tree = TempTree::create();
  const std::filesystem::path root = tree.root;
  std::error_code code;
  std::filesystem::create_directories(root / "node_modules" / "pkg", code);
  std::filesystem::create_directories(root / "target" / "debug", code);
  std::filesystem::create_directories(root / "src", code);
  TempTree::writeFile(root / "node_modules" / "pkg" / "dep.txt", "needle here\n");
  TempTree::writeFile(root / "target" / "debug" / "artifact.txt", "needle here\n");
  TempTree::writeFile(root / "src" / "kept.txt", "needle here\n");
  TempTree::writeFile(root / "ignored.txt", "needle here\n");
  TempTree::writeFile(root / ".gitignore", "ignored.txt\n# comentario\n/build-only/\n");

  const ExecutionResult search = filesystem::searchDispatch(
      {"local", arguments({{"path", StructuredValue(root.string())}, {"query", std::string("needle")}})});
  require(search.status == ExecutionStatus::success, "search with ignores should succeed");
  const StructuredValue::Array* matches = arrayOutput(search, "matches");
  require(matches != nullptr, "search with ignores should return matches");
  require(matches->size() == 1, "search should keep only the non-ignored source file");
  const auto* keptPath = matchField(matches->front(), "path");
  require(
      keptPath != nullptr && std::get<std::string>(keptPath->value) == (root / "src/kept.txt").string(),
      "search should ignore node_modules, target and .gitignore entries");

  // Uma negacao explicita no .gitignore devolve o arquivo a busca.
  TempTree::writeFile(root / ".gitignore", "ignored.txt\n!kept-again.txt\n");
  TempTree::writeFile(root / "kept-again.txt", "needle here\n");
  const ExecutionResult negated = filesystem::searchDispatch(
      {"local", arguments({{"path", StructuredValue(root.string())}, {"query", std::string("needle")}})});
  const StructuredValue::Array* negatedMatches = arrayOutput(negated, "matches");
  require(
      negatedMatches != nullptr && negatedMatches->size() == 2,
      "a negated gitignore rule should keep the file searchable");

  // Busca por nome de arquivo continua funcionando fora dos diretorios ignorados.
  const ExecutionResult byName = filesystem::searchDispatch(
      {"local", arguments({{"path", StructuredValue(root.string())}, {"query", std::string("artifact")}})});
  const StructuredValue::Array* nameMatches = arrayOutput(byName, "matches");
  require(
      nameMatches != nullptr && nameMatches->empty(),
      "file name search should not report ignored build artifacts");
}

// Extrai os caminhos retornados por um resultado de glob.
std::vector<std::string> globPaths(const ExecutionResult& result) {
  std::vector<std::string> paths;
  const StructuredValue::Array* matches = arrayOutput(result, "matches");
  if (matches == nullptr) {
    return paths;
  }
  for (const StructuredValue& match : *matches) {
    if (const auto* path = std::get_if<std::string>(&match.value); path != nullptr) {
      paths.push_back(*path);
    }
  }
  return paths;
}

void testGlob() {
  TempTree tree = TempTree::create();
  const std::filesystem::path root = tree.root;
  std::error_code code;
  std::filesystem::create_directories(root / "node_modules" / "pkg", code);
  std::filesystem::create_directories(root / "src" / "deep", code);
  TempTree::writeFile(root / "node_modules" / "pkg" / "dep.txt", "ignored\n");
  TempTree::writeFile(root / "src" / "main.cpp", "int main() {}\n");
  TempTree::writeFile(root / "src" / "deep" / "util.cpp", "void util() {}\n");

  // Padrao sem '/' casa o nome do arquivo em qualquer nivel.
  const ExecutionResult byName = filesystem::globDispatch(
      {"local", arguments({{"path", StructuredValue(root.string())}, {"pattern", std::string("*.cpp")}})});
  require(byName.status == ExecutionStatus::success, "filesystem.glob by name should succeed");
  const std::vector<std::string> byNamePaths = globPaths(byName);
  require(byNamePaths.size() == 2, "filesystem.glob should find both cpp files");
  require(
      byNamePaths[0] == (root / "src/deep/util.cpp").string(),
      "filesystem.glob should sort paths deterministically");
  require(
      byNamePaths[1] == (root / "src/main.cpp").string(),
      "filesystem.glob should sort paths deterministically");
  require(
      std::get<std::int64_t>(outputField(byName, "total_matches")->value) == 2,
      "filesystem.glob should report the total");

  // Padrao com '/' casa o caminho relativo a raiz.
  const ExecutionResult byPath = filesystem::globDispatch(
      {"local", arguments({{"path", StructuredValue(root.string())}, {"pattern", std::string("src/**/*.cpp")}})});
  require(byPath.status == ExecutionStatus::success, "filesystem.glob by relative path should succeed");
  const std::vector<std::string> byPathPaths = globPaths(byPath);
  require(byPathPaths.size() == 2, "filesystem.glob should match nested relative paths");

  // Artefatos de build ficam de fora, como em filesystem.search.
  const ExecutionResult ignored = filesystem::globDispatch(
      {"local", arguments({{"path", StructuredValue(root.string())}, {"pattern", std::string("**/*.txt")}})});
  const std::vector<std::string> ignoredPaths = globPaths(ignored);
  require(ignoredPaths.size() == 2, "filesystem.glob should find the two source txt files");
  for (const std::string& path : ignoredPaths) {
    require(
        path.find("node_modules") == std::string::npos,
        "filesystem.glob should ignore node_modules");
  }

  // Limitacao explicita.
  const ExecutionResult limited = filesystem::globDispatch(
      {"local", arguments({{"path", StructuredValue(root.string())},
                           {"pattern", std::string("**/*.cpp")},
                           {"max_results", std::int64_t{1}}})});
  require(limited.status == ExecutionStatus::success, "limited glob should succeed");
  require(globPaths(limited).size() == 1, "limited glob should honor max_results");
  require(
      std::get<bool>(outputField(limited, "truncated")->value),
      "limited glob should report truncation");

  // Erros.
  const ExecutionResult missingPath = filesystem::globDispatch(
      {"local", arguments({{"pattern", StructuredValue(std::string("*.cpp"))}})});
  require(missingPath.status == ExecutionStatus::failed, "glob without path should fail");
  require(
      missingPath.error == "field 'path' must be a non-empty string",
      "glob path error should be explicit");

  const ExecutionResult missingPattern = filesystem::globDispatch(
      {"local", arguments({{"path", StructuredValue(root.string())}})});
  require(missingPattern.status == ExecutionStatus::failed, "glob without pattern should fail");
  require(
      missingPattern.error == "field 'pattern' must be a non-empty string",
      "glob pattern error should be explicit");

  const std::string notDirectory = tree.file("hello.txt");
  const ExecutionResult notDir = filesystem::globDispatch(
      {"local", arguments({{"path", StructuredValue(notDirectory)}, {"pattern", std::string("*.cpp")}})});
  require(notDir.status == ExecutionStatus::failed, "glob on a file should fail");
  require(notDir.error == "path is not a directory: " + notDirectory, "glob error should be explicit");
}

void testPatch() {
  TempTree tree = TempTree::create();
  const std::filesystem::path added = tree.root / "patch-added.txt";
  const std::filesystem::path source = tree.root / "patch-source.txt";
  const std::filesystem::path target = tree.root / "patch-target.txt";
  TempTree::writeFile(source, "hello\nworld\n");

  // Add File.
  const std::string addPatch =
      "*** Begin Patch\n"
      "*** Add File: " + added.string() + "\n"
      "+first\n"
      "+second\n"
      "*** End Patch\n";
  const ExecutionResult add = filesystem::patchDispatch(
      {"local", arguments({{"patch", StructuredValue(addPatch)}})});
  require(add.status == ExecutionStatus::success, "filesystem.patch add should succeed");
  require(tree.readFile(added) == "first\nsecond\n", "patch add should write the content");
  require(
      std::get<std::int64_t>(outputField(add, "added")->value) == 1,
      "patch should count added files");
  const StructuredValue::Array* addChanges = arrayOutput(add, "changes");
  require(addChanges != nullptr && addChanges->size() == 1, "patch add should report one change");
  require(
      std::get<std::string>(matchField(addChanges->front(), "action")->value) == "create",
      "patch should report the create action");
  require(
      std::get<std::string>(matchField(addChanges->front(), "diff")->value) ==
          "@@ -0,0 +1,2 @@\n+first\n+second\n",
      "patch should report the create diff");

  // Update File.
  const std::string updatePatch =
      "*** Begin Patch\n"
      "*** Update File: " + added.string() + "\n"
      "@@\n"
      " first\n"
      "-second\n"
      "+second-updated\n"
      "+third\n"
      "*** End Patch\n";
  const ExecutionResult update = filesystem::patchDispatch(
      {"local", arguments({{"patch", StructuredValue(updatePatch)}})});
  require(update.status == ExecutionStatus::success, "filesystem.patch update should succeed");
  require(
      tree.readFile(added) == "first\nsecond-updated\nthird\n",
      "patch update should apply the hunk");
  require(
      std::get<std::int64_t>(outputField(update, "updated")->value) == 1,
      "patch should count updated files");
  const StructuredValue::Array* updateChanges = arrayOutput(update, "changes");
  require(updateChanges != nullptr && updateChanges->size() == 1, "patch update should report one change");
  require(
      std::get<std::string>(matchField(updateChanges->front(), "action")->value) == "edit",
      "patch should report the edit action");
  require(
      std::get<std::string>(matchField(updateChanges->front(), "diff")->value) ==
          "@@ -1,2 +1,3 @@\n first\n-second\n+second-updated\n+third\n",
      "patch should report the edit diff");

  // Move to com edicao.
  const std::string movePatch =
      "*** Begin Patch\n"
      "*** Update File: " + source.string() + "\n"
      "*** Move to: " + target.string() + "\n"
      "@@\n"
      "-hello\n"
      "+hi\n"
      " world\n"
      "*** End Patch\n";
  const ExecutionResult move = filesystem::patchDispatch(
      {"local", arguments({{"patch", StructuredValue(movePatch)}})});
  require(move.status == ExecutionStatus::success, "filesystem.patch move should succeed");
  require(!std::filesystem::exists(source), "patch move should remove the source");
  require(tree.readFile(target) == "hi\nworld\n", "patch move should keep the edited content");
  require(
      std::get<std::int64_t>(outputField(move, "moved")->value) == 1,
      "patch should count moves");
  const StructuredValue::Array* moveChanges = arrayOutput(move, "changes");
  require(moveChanges != nullptr && moveChanges->size() == 1, "patch move should report one change");
  require(
      std::get<std::string>(matchField(moveChanges->front(), "action")->value) == "move",
      "patch should report the move action");
  require(
      std::get<std::string>(matchField(moveChanges->front(), "path")->value).front() != '/',
      "patch should report a relative source path");
  require(
      std::get<std::string>(matchField(moveChanges->front(), "moved_to")->value).front() != '/',
      "patch should report a relative destination path");
  require(
      std::get<std::string>(matchField(moveChanges->front(), "diff")->value) ==
      "@@ -1,2 +1,2 @@\n-hello\n+hi\n world\n",
      "patch should report the edit made during a move");

  // Delete File.
  const std::string deletePatch =
      "*** Begin Patch\n"
      "*** Delete File: " + added.string() + "\n"
      "*** End Patch\n";
  const ExecutionResult remove = filesystem::patchDispatch(
      {"local", arguments({{"patch", StructuredValue(deletePatch)}})});
  require(remove.status == ExecutionStatus::success, "filesystem.patch delete should succeed");
  require(!std::filesystem::exists(added), "patch delete should remove the file");
  require(
      std::get<std::int64_t>(outputField(remove, "deleted")->value) == 1,
      "patch should count deleted files");
  const StructuredValue::Array* removeChanges = arrayOutput(remove, "changes");
  require(removeChanges != nullptr && removeChanges->size() == 1, "patch delete should report one change");
  require(
      std::get<std::string>(matchField(removeChanges->front(), "action")->value) == "delete",
      "patch should report the delete action");
  require(
      std::get<std::string>(matchField(removeChanges->front(), "diff")->value) ==
      "@@ -1,3 +0,0 @@\n-first\n-second-updated\n-third\n",
      "patch should report the delete diff");

  // Parsing invalido.
  const ExecutionResult noBegin = filesystem::patchDispatch(
      {"local", arguments({{"patch", StructuredValue(std::string("*** Add File: x\n+hi\n*** End Patch\n"))}})});
  require(noBegin.status == ExecutionStatus::failed, "patch without begin should fail");
  require(
      noBegin.error == "invalid patch: patch must start with '*** Begin Patch'",
      "patch begin error should be explicit");

  const ExecutionResult noEnd = filesystem::patchDispatch(
      {"local", arguments({{"patch", StructuredValue(std::string("*** Begin Patch\n*** Delete File: x\n"))}})});
  require(noEnd.status == ExecutionStatus::failed, "patch without end should fail");
  require(
      noEnd.error == "invalid patch: patch must end with '*** End Patch'",
      "patch end error should be explicit");

  const ExecutionResult badLine = filesystem::patchDispatch(
      {"local", arguments({{"patch", StructuredValue(
          "*** Begin Patch\n*** Add File: /tmp/atlas-patch-x\nhello\n*** End Patch\n")}})});
  require(badLine.status == ExecutionStatus::failed, "add without '+' should fail");
  require(
      badLine.error == "invalid patch: add lines must start with '+' in section '/tmp/atlas-patch-x'",
      "patch add line error should be explicit");

  const ExecutionResult empty = filesystem::patchDispatch(
      {"local", arguments({{"patch", StructuredValue(std::string("*** Begin Patch\n*** End Patch\n"))}})});
  require(empty.status == ExecutionStatus::failed, "empty patch should fail");
  require(
      empty.error == "invalid patch: patch must contain at least one operation",
      "empty patch error should be explicit");

  const ExecutionResult missingPatch = filesystem::patchDispatch({"local", {}});
  require(missingPatch.status == ExecutionStatus::failed, "patch without field should fail");
  require(
      missingPatch.error == "field 'patch' must be a string",
      "patch field error should be explicit");

  // Erros de execucao.
  const ExecutionResult addExisting = filesystem::patchDispatch(
      {"local", arguments({{"patch", StructuredValue(
          "*** Begin Patch\n*** Add File: " + tree.file("hello.txt") + "\n+hi\n*** End Patch\n")}})});
  require(addExisting.status == ExecutionStatus::failed, "adding an existing file should fail");
  require(
      addExisting.error == "cannot add file that already exists: " + tree.file("hello.txt"),
      "add existing error should be explicit");

  const ExecutionResult deleteMissing = filesystem::patchDispatch(
      {"local", arguments({{"patch", StructuredValue(
          "*** Begin Patch\n*** Delete File: " + tree.file("missing.txt") + "\n*** End Patch\n")}})});
  require(deleteMissing.status == ExecutionStatus::failed, "deleting a missing file should fail");
  require(
      deleteMissing.error == "cannot delete file that does not exist: " + tree.file("missing.txt"),
      "delete missing error should be explicit");

  const ExecutionResult updateMissing = filesystem::patchDispatch(
      {"local", arguments({{"patch", StructuredValue(
          "*** Begin Patch\n*** Update File: " + tree.file("missing.txt") + "\n@@\n-x\n+y\n*** End Patch\n")}})});
  require(updateMissing.status == ExecutionStatus::failed, "updating a missing file should fail");
  require(
      updateMissing.error == "cannot update file that does not exist: " + tree.file("missing.txt"),
      "update missing error should be explicit");

  const ExecutionResult noContext = filesystem::patchDispatch(
      {"local", arguments({{"patch", StructuredValue(
          "*** Begin Patch\n*** Update File: " + tree.file("hello.txt") + "\n@@\n-not-here\n+x\n*** End Patch\n")}})});
  require(noContext.status == ExecutionStatus::failed, "update without matching context should fail");
  require(
      noContext.error == "update '" + tree.file("hello.txt") + "': hunk 1 context not found",
      "hunk context error should be explicit");

  const ExecutionResult moveExisting = filesystem::patchDispatch(
      {"local", arguments({{"patch", StructuredValue(
          "*** Begin Patch\n*** Update File: " + tree.file("hello.txt") + "\n*** Move to: " +
          (tree.root / "nested/other.txt").string() + "\n*** End Patch\n")}})});
  require(moveExisting.status == ExecutionStatus::failed, "moving onto an existing file should fail");
  require(
      moveExisting.error == "cannot move onto existing file: " + (tree.root / "nested/other.txt").string(),
      "move existing error should be explicit");

  // Validacao antes de aplicar: um patch parcialmente invalido nao altera nada.
  const std::filesystem::path untouched = tree.root / "patch-untouched.txt";
  const ExecutionResult atomic = filesystem::patchDispatch(
      {"local", arguments({{"patch", StructuredValue(
          "*** Begin Patch\n*** Add File: " + untouched.string() + "\n+content\n*** Update File: " +
          tree.file("hello.txt") + "\n@@\n-not-here\n+x\n*** End Patch\n")}})});
  require(atomic.status == ExecutionStatus::failed, "invalid patch should fail");
  require(!std::filesystem::exists(untouched), "failed patch should not create files");
}

}  // namespace

int main() {
  const TempTree tree = TempTree::create();
  testLoadingAndDiscovery();
  testReadThroughExecutor(tree);
  testRead(tree);
  testList(tree);
  testSearch(tree);
  testSearchIgnores();
  testGlob();
  testPatch();
  return EXIT_SUCCESS;
}
