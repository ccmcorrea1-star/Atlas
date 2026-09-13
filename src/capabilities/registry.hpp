#pragma once

#include <functional>
#include <map>
#include <optional>
#include <shared_mutex>
#include <string>
#include <string_view>
#include <vector>

namespace atlas::capabilities {

// Descreve uma capability sem depender da linguagem que a implementa.
struct Capability {
  std::string id;
  std::string type;
  std::string summary;
  std::optional<std::string> parent;
  std::vector<std::string> aliases;
  std::string implementation;
};

// Mantem capabilities mutaveis em runtime e fornece snapshots ordenados.
class Registry {
 public:
  Registry() = default;
  Registry(const Registry&) = delete;
  Registry& operator=(const Registry&) = delete;

  // Retorna false quando a definicao e invalida ou o id ja esta registrado.
  bool registerCapability(Capability capability);

  // Remove somente a capability identificada por id.
  bool unregister(std::string_view id);

  // Atualiza uma capability existente sem permitir troca de identidade.
  bool update(Capability capability);
  bool update(std::string_view id, Capability capability);

  // Busca por id. Aliases continuam sendo usados apenas pela busca textual.
  std::optional<Capability> get(std::string_view id) const;

  // Todas as operacoes de consulta retornam dados independentes do Registry.
  std::vector<Capability> list() const;
  std::vector<Capability> children(std::string_view path) const;
  std::vector<Capability> search(std::string_view query) const;

 private:
  static bool isValid(const Capability& capability);

  mutable std::shared_mutex mutex_;
  std::map<std::string, Capability, std::less<>> capabilities_;
};

using CapabilityRegistry = Registry;

}  // namespace atlas::capabilities
