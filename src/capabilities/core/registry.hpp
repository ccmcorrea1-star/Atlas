#pragma once

#include "execution.hpp"

#include <cstddef>
#include <functional>
#include <map>
#include <optional>
#include <shared_mutex>
#include <string>
#include <string_view>
#include <utility>
#include <vector>

namespace atlas::capabilities {

// Identifica a fronteira de execucao sem acoplar o Registry a uma linguagem.
struct CapabilityImplementation {
  std::string kind;
  std::string entrypoint;

  CapabilityImplementation() = default;

  // Mantem a inicializacao simples das capabilities nativas existentes.
  CapabilityImplementation(const char* legacyEntrypoint)
      : kind("native"), entrypoint(legacyEntrypoint == nullptr ? "" : legacyEntrypoint) {}

  // Interpreta referencias antigas como entrypoints nativos.
  CapabilityImplementation(std::string legacyEntrypoint)
      : kind("native"), entrypoint(std::move(legacyEntrypoint)) {}

  // Permite construir diretamente a referencia lida de um manifesto.
  CapabilityImplementation(std::string implementationKind, std::string implementationEntrypoint)
      : kind(std::move(implementationKind)), entrypoint(std::move(implementationEntrypoint)) {}

  bool empty() const noexcept { return kind.empty() || entrypoint.empty(); }
};

// Descreve uma capability sem depender da linguagem que a implementa.
struct Capability {
  std::string id;
  std::string type;
  std::string summary;
  std::optional<std::string> parent;
  std::vector<std::string> aliases;
  CapabilityImplementation implementation;

  // Mantem os detalhes necessarios para materializar a capability depois da descoberta.
  std::string description;
  StructuredValue schema;
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

  // Retorna a definicao completa, incluindo descricao, schema e implementation.
  std::optional<Capability> getDefinition(std::string_view id) const;

  // Registra a funcao que resolve um entrypoint de implementation.kind native.
  bool registerNativeEntrypoint(std::string entrypoint, NativeEntrypoint function);

  // Retorna uma copia segura do entrypoint nativo registrado.
  std::optional<NativeEntrypoint> resolveNativeEntrypoint(std::string_view entrypoint) const;

  // Remove um entrypoint nativo registrado explicitamente.
  bool unregisterNativeEntrypoint(std::string_view entrypoint);

  // Todas as operacoes de consulta retornam dados independentes do Registry.
  std::vector<Capability> list() const;
  std::vector<Capability> rootGroups() const;
  std::vector<Capability> children(std::string_view path) const;
  // Ordena por especificidade do campo e aplica o limite quando informado.
  std::vector<Capability> search(
      std::string_view query,
      std::optional<std::size_t> limit = std::nullopt) const;

 private:
  static bool isValid(const Capability& capability);

  mutable std::shared_mutex mutex_;
  std::map<std::string, Capability, std::less<>> capabilities_;
  std::map<std::string, NativeEntrypoint, std::less<>> native_entrypoints_;
};

using CapabilityRegistry = Registry;

}  // namespace atlas::capabilities
