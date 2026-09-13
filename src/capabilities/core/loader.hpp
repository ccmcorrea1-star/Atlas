#pragma once

#include "registry.hpp"

#include <filesystem>
#include <map>
#include <string>
#include <string_view>

namespace atlas::capabilities {

// Carrega manifestos e mantem a origem necessaria para recarregamento explicito.
class Loader {
 public:
  explicit Loader(Registry& registry) : registry_(registry) {}
  Loader(const Loader&) = delete;
  Loader& operator=(const Loader&) = delete;

  // Valida e registra o manifesto capability.json ou group.json indicado.
  bool load(const std::filesystem::path& path);

  // Remove do Registry o recurso identificado, sem executar a implementacao.
  bool unload(std::string_view id);

  // Rele o manifesto usado no load e atualiza a capability de forma atomica.
  bool reload(std::string_view id);

  // Procura capability.json recursivamente e carrega cada manifesto encontrado.
  bool scan(const std::filesystem::path& directory);

  // Retorna a causa da ultima operacao que falhou.
  const std::string& lastError() const noexcept { return last_error_; }

 private:
  static bool parseManifest(
      const std::filesystem::path& path,
      Capability& capability,
      std::string& error);
  static std::filesystem::path sourcePath(const std::filesystem::path& path);
  bool fail(std::string message);

  Registry& registry_;
  std::map<std::string, std::filesystem::path, std::less<>> sources_;
  std::string last_error_;
};

using CapabilityLoader = Loader;

}  // namespace atlas::capabilities
