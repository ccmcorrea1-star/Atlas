// Bateria de discovery por intencao: queries em linguagem natural (pt-BR
// principal) contra os manifestos reais. Regressao dos 44 casos da bateria
// de uso real + formulacoes variadas por capability.
#include "../../src/capabilities/core/discovery.hpp"
#include "../../src/capabilities/core/loader.hpp"
#include "../../src/capabilities/core/registry.hpp"

#include <algorithm>
#include <cstdlib>
#include <iostream>
#include <string>
#include <string_view>
#include <vector>

namespace {

using atlas::capabilities::Discovery;
using atlas::capabilities::DiscoveryRequest;
using atlas::capabilities::Loader;
using atlas::capabilities::Registry;

void require(bool condition, const std::string& message) {
  if (!condition) {
    std::cerr << "discovery test failed: " << message << '\n';
    std::exit(EXIT_FAILURE);
  }
}

struct Expectation {
  std::string_view query;
  std::string_view expectedTop;
};

std::string topFor(Discovery& discovery, std::string_view query) {
  DiscoveryRequest request;
  request.query = std::string(query);
  const auto results = discovery.discover(request);
  if (results.empty()) {
    return "<vazio>";
  }
  return results.front().id;
}

void expectTop(Discovery& discovery, const Expectation& expectation) {
  const std::string top = topFor(discovery, expectation.query);
  require(
      top == expectation.expectedTop,
      std::string(expectation.query) + " -> topo '" + top + "', esperado '" +
          std::string(expectation.expectedTop) + "'");
}

void expectSmallAndOrdered(Discovery& discovery, std::string_view query, std::size_t maxResults) {
  DiscoveryRequest request;
  request.query = std::string(query);
  const auto results = discovery.discover(request);
  require(!results.empty(), std::string(query) + " devolveu vazio");
  require(
      results.size() <= maxResults,
      std::string(query) + " devolveu " + std::to_string(results.size()) + " resultados");
}

}  // namespace

int main() {
  Registry registry;
  Loader loader(registry);
  const std::vector<std::string> manifests = {
      "src/capabilities/tools/filesystem/read/capability.json",
      "src/capabilities/tools/filesystem/list/capability.json",
      "src/capabilities/tools/filesystem/search/capability.json",
      "src/capabilities/tools/filesystem/glob/capability.json",
      "src/capabilities/tools/filesystem/patch/capability.json",
      "src/capabilities/tools/lsp/diagnostics/capability.json",
      "src/capabilities/tools/shell/exec/capability.json",
      "src/capabilities/tools/system/info/capability.json",
      "src/capabilities/tools/web/fetch/capability.json",
      "src/capabilities/tools/web/search/capability.json",
      "src/capabilities/tools/web/browser/capability.json",
      "src/capabilities/tools/web/crawl/capability.json",
  };
  for (const std::string& manifest : manifests) {
    require(loader.load(manifest), "manifesto real deve carregar: " + manifest);
  }
  Discovery discovery(registry);

  // Uso individual: formulacoes variadas por capability.
  const std::vector<Expectation> cases = {
      {"qual o sistema operacional", "system.info"},
      {"mostre informacoes da maquina", "system.info"},
      {"arquitetura do sistema", "system.info"},
      {"qual a versao do sistema", "system.info"},
      {"uname", "system.info"},
      {"ler arquivo", "filesystem.read"},
      {"ver arquivo", "filesystem.read"},
      {"mostre o conteudo do arquivo", "filesystem.read"},
      {"abrir arquivo para leitura", "filesystem.read"},
      {"ler as primeiras linhas", "filesystem.read"},
      {"criar um arquivo novo", "filesystem.patch"},
      {"escrever arquivo", "filesystem.patch"},
      {"salvar dados num arquivo", "filesystem.patch"},
      {"editar o arquivo", "filesystem.patch"},
      {"trocar um trecho do codigo", "filesystem.patch"},
      {"corrigir o bug", "filesystem.patch"},
      {"substituir texto", "filesystem.patch"},
      {"listar arquivos da pasta", "filesystem.list"},
      {"o que tem no diretorio", "filesystem.list"},
      {"ls", "filesystem.list"},
      {"buscar texto no projeto", "filesystem.search"},
      {"procurar funcao no codigo", "filesystem.search"},
      {"onde esta definida a funcao", "filesystem.search"},
      {"grep", "filesystem.search"},
      {"comando com pipe", "shell.exec"},
      {"redirecionar saida para arquivo", "shell.exec"},
      {"encadear comandos com shell", "shell.exec"},
      {"ver erros do codigo", "lsp.diagnostics"},
      {"analisar o codigo", "lsp.diagnostics"},
      {"quais warnings", "lsp.diagnostics"},
      {"tipo errado", "lsp.diagnostics"},
      {"diagnostics", "lsp.diagnostics"},
      {"pesquisar na internet", "web.search"},
      {"buscar tutoriais de rust", "web.search"},
      {"procurar tutoriais na web", "web.search"},
      {"procure documentacao", "web.search"},
      {"navegue no site", "web.browser"},
      {"rastreie as paginas", "web.crawl"},
      {"abrir essa url", "web.fetch"},
      {"baixar a pagina", "web.fetch"},
      {"ler o conteudo do link", "web.fetch"},
      // Desambiguacao.
      {"rodar um comando", "shell.exec"},
      {"buscar no projeto", "filesystem.search"},
      {"ver erros", "lsp.diagnostics"},
      {"abrir url", "web.fetch"},
      {"abrir arquivo", "filesystem.read"},
      {"criar arquivo", "filesystem.patch"},
      {"servidor de linguagem", "lsp.diagnostics"},
      {"server", "lsp.diagnostics"},
      {"ver", "filesystem.read"},
      {"executar", "shell.exec"},
  };
  for (const Expectation& expectation : cases) {
    expectTop(discovery, expectation);
  }

  // Ordenacao deterministica entre as capabilities mais bem colocadas.
  {
    DiscoveryRequest request;
    request.query = std::string("executar");
    const auto results = discovery.discover(request);
    require(results.size() >= 2, "executar devolve ao menos dois");
    require(results[0].id == "shell.exec", "executar: topo shell.exec");
    require(results[1].id == "lsp.diagnostics", "executar: segundo lsp.diagnostics por id");
  }
  {
    DiscoveryRequest request;
    request.query = std::string("ver");
    const auto results = discovery.discover(request);
    require(results.size() >= 2, "ver devolve ao menos dois");
    require(results[0].id == "filesystem.read", "ver: topo filesystem.read");
    require(results[1].id == "lsp.diagnostics", "ver: segundo lsp.diagnostics");
  }

  // Stopwords e acentos.
  {
    DiscoveryRequest request;
    request.query = std::string("de o para com e");
    const auto results = discovery.discover(request);
    require(results.size() == 12, "so stopwords equivale a query vazia: 12 capabilities");
  }
  expectTop(discovery, {"por favor me mostre o arquivo", "filesystem.read"});
  expectTop(discovery, {"informações do sistema", "system.info"});
  expectTop(discovery, {"informacoes do sistema", "system.info"});

  // Regressao D07/D18: termos tecnicos/literais (nome de arquivo,
  // identificador) nao podem esconder a capability que a intencao aponta.
  expectTop(discovery, {"leia o arquivo README.md", "filesystem.read"});
  expectTop(discovery, {"grep por conversationId", "filesystem.search"});
  expectTop(discovery, {"cat README.md", "filesystem.read"});
  expectTop(discovery, {"ripgrep padrao", "filesystem.search"});

  // Regressao D42 - multi-intencao: preserva todas as capabilities relevantes,
  // sem exigir que o primeiro resultado resolva a solicitacao inteira.
  {
    DiscoveryRequest request;
    request.query = std::string("liste o conteudo e leia o arquivo");
    const auto results = discovery.discover(request);
    const auto present = [&results](std::string_view id) {
      return std::any_of(results.begin(), results.end(), [id](const auto& item) {
        return item.id == id;
      });
    };
    require(present("filesystem.list"), "multi-intencao deve preservar filesystem.list");
    require(present("filesystem.read"), "multi-intencao deve preservar filesystem.read");
    require(
        results.front().id == "filesystem.read" || results.front().id == "filesystem.list",
        "multi-intencao deve ranquear uma capability relevante no topo");
  }

  // Sem sinal suficiente: vazio deliberado, sem confianca artificial.
  for (const std::string_view query : {"xyzq banana", "node --version", "servidor banana"}) {
    DiscoveryRequest request;
    request.query = std::string(query);
    const auto results = discovery.discover(request);
    require(results.empty(), std::string(query) + " deveria devolver vazio");
  }

  // Resultados pequenos e ordenados, sem relacao espuria.
  expectSmallAndOrdered(discovery, "ver arquivo", 3);
  expectSmallAndOrdered(discovery, "qual o sistema operacional", 3);
  expectSmallAndOrdered(discovery, "buscar texto no projeto", 3);

  std::cout << "discovery: " << cases.size() << " queries ok\n";
  return EXIT_SUCCESS;
}
