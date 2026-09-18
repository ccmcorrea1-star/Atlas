// Testes das tools git.* contra um repositorio temporario real.
#include "../../src/capabilities/tools/git/status/status.hpp"
#include "../../src/capabilities/tools/git/diff/diff.hpp"

#include <cstdio>
#include <cstdlib>
#include <filesystem>
#include <iostream>
#include <string>

namespace {

using atlas::capabilities::ExecutionStatus;
using atlas::capabilities::NativeRequest;
using atlas::capabilities::StructuredValue;
using atlas::capabilities::tools::git::dispatch;
using atlas::capabilities::tools::git::dispatchDiff;

void require(bool condition, const std::string& message) {
  if (!condition) {
    std::cerr << "git test failed: " << message << '\n';
    std::exit(EXIT_FAILURE);
  }
}

NativeRequest request(std::string path, bool staged) {
  NativeRequest native;
  native.target = "local";
  native.arguments["path"] = path;
  if (staged) {
    native.arguments["staged"] = true;
  }
  return native;
}

const StructuredValue::Object& outputOf(
    const atlas::capabilities::ExecutionResult& result,
    const std::string& context) {
  require(result.status == ExecutionStatus::success, context + ": status " + std::to_string(static_cast<int>(result.status)));
  const auto* output = std::get_if<StructuredValue::Object>(&result.output.value);
  require(output != nullptr, context + ": output nao e objeto");
  return *output;
}

bool cleanOf(const StructuredValue::Object& output) {
  const auto iterator = output.find("clean");
  require(iterator != output.end(), "sem campo clean");
  const auto* clean = std::get_if<bool>(&iterator->second.value);
  require(clean != nullptr, "clean nao e booleano");
  return *clean;
}

std::size_t arraySize(const StructuredValue::Object& output, std::string_view key) {
  const auto iterator = output.find(std::string(key));
  require(iterator != output.end(), std::string(key) + " ausente");
  const auto* array = std::get_if<StructuredValue::Array>(&iterator->second.value);
  require(array != nullptr, std::string(key) + " nao e array");
  return array->size();
}

std::string shell(const std::string& command) {
  require(std::system(command.c_str()) == 0, "comando falhou: " + command);
  return command;
}

}  // namespace

int main() {
  char templateDir[] = "/tmp/atlas-git-test-XXXXXX";
  require(mkdtemp(templateDir) != nullptr, "mkdtemp falhou");
  const std::string dir(templateDir);
  shell("git -C " + dir + " init -q");
  shell("git -C " + dir + " config user.email t@t.t && git -C " + dir + " config user.name t");

  // Repositorio vazio: branch existe, limpo.
  const auto empty = outputOf(dispatch(request(dir, false), {}), "status vazio");
  require(cleanOf(empty), "repo vazio deveria estar limpo");

  // Arquivo novo: untracked.
  {
    FILE* file = std::fopen((dir + "/a.txt").c_str(), "w");
    require(file != nullptr, "criar a.txt");
    std::fputs("um\n", file);
    std::fclose(file);
  }
  const auto untracked = outputOf(dispatch(request(dir, false), {}), "status untracked");
  require(!cleanOf(untracked), "com a.txt deveria estar sujo");
  require(arraySize(untracked, "untracked") == 1, "um untracked");
  require(arraySize(untracked, "staged") == 0, "nada no staging");

  // git add: vai para staged; diff do staged mostra o arquivo.
  shell("git -C " + dir + " add a.txt");
  const auto staged = outputOf(dispatch(request(dir, false), {}), "status staged");
  require(arraySize(staged, "staged") == 1, "um staged");
  require(arraySize(staged, "untracked") == 0, "sem untracked");
  const auto stagedDiff = outputOf(dispatchDiff(request(dir, true), {}), "diff staged");
  {
    const auto iterator = stagedDiff.find("diff");
    require(iterator != stagedDiff.end(), "sem campo diff");
    const auto* text = std::get_if<std::string>(&iterator->second.value);
    require(text != nullptr && text->find("a.txt") != std::string::npos, "diff staged sem a.txt");
  }

  // Modificacao no working tree: unstaged + diff nao staged a mostra.
  {
    FILE* file = std::fopen((dir + "/a.txt").c_str(), "a");
    require(file != nullptr, "abrir a.txt");
    std::fputs("dois\n", file);
    std::fclose(file);
  }
  const auto dirty = outputOf(dispatch(request(dir, false), {}), "status dirty");
  require(arraySize(dirty, "unstaged") == 1, "um unstaged");
  const auto workDiff = outputOf(dispatchDiff(request(dir, false), {}), "diff worktree");
  {
    const auto* text = std::get_if<std::string>(&workDiff.find("diff")->second.value);
    require(text != nullptr && !text->empty(), "diff worktree vazio");
  }

  // Fora de repo: falha estruturada.
  {
    const auto result = dispatch(request("/tmp", false), {});
    require(result.status == ExecutionStatus::failed, "fora de repo deveria falhar");
    require(!result.error.empty(), "falha sem mensagem");
  }

  // Path obrigatorio.
  {
    atlas::capabilities::NativeRequest bad;
    bad.target = "local";
    const auto result = dispatch(bad, {});
    require(result.status == ExecutionStatus::failed, "sem path deveria falhar");
  }

  std::filesystem::remove_all(dir);
  std::cout << "git: status + diff ok\n";
  return EXIT_SUCCESS;
}
