#pragma once

#include <cstddef>
#include <filesystem>
#include <string>
#include <string_view>
#include <vector>

namespace atlas::capabilities::tools::filesystem {

// Decide quais entradas a busca recursiva deve pular: artefatos conhecidos de
// build/dependencias e padroes dos arquivos .gitignore do projeto.
class SearchIgnores {
 public:
  // Carrega as regras embutidas para artefatos que nunca sao codigo-fonte.
  void loadDefaults(const std::filesystem::path& base);

  // Carrega o .gitignore do diretorio quando o arquivo existir.
  void loadDirectory(const std::filesystem::path& directory);

  // Carrega os .gitignore desde a raiz do repositorio ate o diretorio buscado.
  void loadAncestors(const std::filesystem::path& root);

  // Permite empilhar e desempilhar as regras de um subdiretorio visitado.
  std::size_t mark() const;
  void restore(std::size_t mark);

  // A ultima regra que casa decide; diretorios ignorados nao sao percorridos.
  bool ignores(const std::filesystem::path& path, bool is_directory) const;

 private:
  struct Rule {
    std::string pattern;
    bool negated = false;
    bool directory_only = false;
    bool anchored = false;
  };

  struct Source {
    std::filesystem::path base;
    std::vector<Rule> rules;
  };

  static std::vector<Rule> parse(std::string_view content);
  void addRules(const std::filesystem::path& base, std::vector<Rule> rules);

  std::vector<Source> sources_;
};

}  // namespace atlas::capabilities::tools::filesystem
