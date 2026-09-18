#include "registry.hpp"

#include <algorithm>
#include <mutex>
#include <unordered_map>
#include <unordered_set>
#include <utility>

namespace atlas::capabilities {
namespace {

std::string foldChar(std::string_view value, std::size_t& index) {
  // Vogais acentuadas e cedilha em UTF-8 (latin-1 de 2 bytes: 0xC3 + segundo).
  static const std::unordered_map<unsigned char, char> folded = {
      {0x80, 'a'},  // À
      {0x81, 'a'},  // Á
      {0x82, 'a'},  // Â
      {0x83, 'a'},  // Ã
      {0x84, 'a'},  // Ä
      {0x85, 'a'},  // Å
      {0x87, 'c'},  // Ç
      {0x88, 'e'},  // È
      {0x89, 'e'},  // É
      {0x8a, 'e'},  // Ê
      {0x8b, 'e'},  // Ë
      {0x8c, 'i'},  // Ì
      {0x8d, 'i'},  // Í
      {0x8e, 'i'},  // Î
      {0x8f, 'i'},  // Ï
      {0x91, 'n'},  // Ñ
      {0x92, 'o'},  // Ò
      {0x93, 'o'},  // Ó
      {0x94, 'o'},  // Ô
      {0x95, 'o'},  // Õ
      {0x96, 'o'},  // Ö
      {0x99, 'u'},  // Ù
      {0x9a, 'u'},  // Ú
      {0x9b, 'u'},  // Û
      {0x9c, 'u'},  // Ü
      {0xa0, 'a'},  // à
      {0xa1, 'a'},  // á
      {0xa2, 'a'},  // â
      {0xa3, 'a'},  // ã
      {0xa4, 'a'},  // ä
      {0xa5, 'a'},  // å
      {0xa7, 'c'},  // ç
      {0xa8, 'e'},  // è
      {0xa9, 'e'},  // é
      {0xaa, 'e'},  // ê
      {0xab, 'e'},  // ë
      {0xac, 'i'},  // ì
      {0xad, 'i'},  // í
      {0xae, 'i'},  // î
      {0xaf, 'i'},  // ï
      {0xb1, 'n'},  // ñ
      {0xb2, 'o'},  // ò
      {0xb3, 'o'},  // ó
      {0xb4, 'o'},  // ô
      {0xb5, 'o'},  // õ
      {0xb6, 'o'},  // ö
      {0xb9, 'u'},  // ù
      {0xba, 'u'},  // ú
      {0xbb, 'u'},  // û
      {0xbc, 'u'},  // ü
  };
  const auto second = static_cast<unsigned char>(value[index + 1]);
  const auto iterator = folded.find(second);
  index += 2;
  if (iterator != folded.end()) {
    return std::string(1, iterator->second);
  }
  return "?";
}

/// Normaliza para busca por intencao: minusculas, sem acento, tokenizado em
/// palavras. Palavra inteira, nao substring: "ver" nao casa "server".
std::vector<std::string> normalizeWords(std::string_view value) {
  std::vector<std::string> words;
  std::string current;
  const std::size_t size = value.size();
  for (std::size_t index = 0; index < size;) {
    const auto character = static_cast<unsigned char>(value[index]);
    if (character < 0x80) {
      const bool isAlnum = (character >= 'a' && character <= 'z') || (character >= 'A' && character <= 'Z') ||
          (character >= '0' && character <= '9');
      if (isAlnum) {
        current.push_back(character >= 'A' && character <= 'Z' ? static_cast<char>(character - 'A' + 'a') : static_cast<char>(character));
      } else if (!current.empty()) {
        words.push_back(std::move(current));
        current.clear();
      }
      ++index;
    } else if (character == 0xC3 && index + 1 < size) {
      current += foldChar(value, index);
    } else {
      if (!current.empty()) {
        words.push_back(std::move(current));
        current.clear();
      }
      ++index;
    }
  }
  if (!current.empty()) {
    words.push_back(std::move(current));
  }
  return words;
}

bool isStopword(std::string_view word) {
  static const std::unordered_set<std::string_view> stopwords = {
      // Portugues: artigos, preposicoes, interrogativas, pronomes, enchimento.
      "de", "da", "do", "das", "dos", "dum", "duma", "em", "num", "numa", "um", "uma", "uns", "umas", "o", "a",
      "os", "as", "e", "ou", "que", "com", "para", "pra", "por", "pelo", "pela", "no", "na", "nos", "nas", "ao",
      "aos", "se", "me", "mim", "te", "ti", "lhe", "lhes", "nos", "vos", "como", "qual", "quais", "onde", "quando",
      "quanto", "quantos", "quanta", "quantas", "porque", "isso", "isto", "esse", "essa", "esses", "essas", "este",
      "esta", "estes", "estas", "aquele", "aquela", "quero", "queria", "gostaria", "preciso", "precisava", "pode",
      "podem", "poderia", "favor", "obrigado", "obrigada", "oi", "ola", "aqui", "ai", "ali", "agora", "hoje",
      "coisa", "algo", "algum", "alguma", "tipo", "tao", "muito", "mais", "menos", "sobre", "entre", "ate", "ja",
      "ainda", "tambem", "ser", "sao", "foi", "foram", "tem", "ha", "meu", "minha", "meus", "minhas", "seu",
      "sua", "deste", "desta", "nesse", "nessa", "desse", "dessa", "num", "faz", "fazer", "vez", "vezes", "todo",
      "toda", "cada", "outro", "outra", "mesmo", "mesma", "proprio", "propria",
      // Ingles basico.
      "the", "a", "an", "of", "in", "on", "to", "for", "with", "how", "what", "where", "when", "which", "is",
      "are", "was", "were", "do", "does", "did", "me", "my", "please", "thanks", "hello", "hi",
  };
  return stopwords.find(word) != stopwords.end();
}

/// Grupos de sinonimos (pt-BR principal): a intencao casa qualquer membro.
/// Membros ambiguos aparecem em mais de um grupo; os demais tokens decidem.
const std::vector<std::vector<std::string>>& synonymGroups() {
  static const std::vector<std::vector<std::string>> groups = {
      {"ler", "leia", "leitura", "ver", "veja", "visualizar", "visualize", "exibir", "exiba", "mostrar",
       "mostre", "abrir", "abra", "consultar", "consulte", "cat", "read"},
      {"criar", "crie", "criacao", "novo", "nova", "escrever", "escreva", "gerar", "gere", "salvar", "salve",
       "write"},
      {"editar", "edite", "edicao", "alterar", "altere", "modificar", "modifique", "trocar", "troque",
       "substituir", "substitua", "atualizar", "atualize", "mudar", "mude", "edit"},
      {"corrigir", "corrija", "correcao", "consertar", "conserte", "ajustar", "ajuste", "fix", "bug", "bugs",
       "defeito", "defeitos"},
      {"listar", "liste", "lista", "listagem", "diretorio", "diretorios", "pasta", "pastas", "ls", "dir",
       "list"},
      {"buscar", "busque", "busca", "procurar", "procure", "procura", "pesquisar", "pesquise", "pesquisa",
       "achar", "ache", "localizar", "localize", "encontrar", "encontre", "grep", "search", "definir",
       "definida", "definido", "definicao"},
      {"executar", "execute", "executa", "execucao", "rodar", "rode", "iniciar", "inicie", "run", "exec"},
      {"sistema", "sistemas", "operacional", "maquina", "maquinas", "computador", "plataforma", "ambiente",
       "system", "info"},
      {"erro", "erros", "errado", "errada", "incorreto", "incorreta", "falha", "falhas", "diagnostico",
       "diagnosticos", "diagnosticar", "diagnostique", "analisar", "analise", "verificar", "verifique",
       "validar", "valide", "aviso", "avisos", "warning", "warnings", "problema", "problemas", "tipagem",
       "tipo", "tipos", "checagem", "diagnostics", "servidor", "servidores"},
      {"web", "internet", "online", "site", "sites", "pagina", "paginas", "url", "urls", "link", "links",
       "http", "https", "html"},
      {"comando", "comandos", "shell", "terminal", "bash", "pipe", "pipes", "redirecionamento",
       "redirecionar", "encadear", "encadeamento", "globbing", "expansao", "variavel", "script", "saida",
       "saidas", "stdin", "stdout", "stderr", "command"},
      {"processo", "processos", "programa", "programas", "binario", "executavel", "process"},
      {"texto", "textos", "conteudo", "trecho", "trechos", "string", "substring", "palavra", "palavras",
       "padrao", "linha", "linhas", "text"},
      {"arquivo", "arquivos", "file", "files"},
      {"codigo", "codigos", "fonte", "funcao", "funcoes", "classe", "classes", "metodo", "metodos",
       "linguagem", "linguagens", "code"},
      {"projeto", "projetos", "repositorio", "project"},
      {"informacao", "informacoes", "dados", "detalhes", "versao", "versoes", "arquitetura", "resumo"},
      {"obter", "obtenha", "obtencao", "baixar", "baixe", "fetch"},
  };
  return groups;
}

int synonymGroupOf(std::string_view word) {
  const auto& groups = synonymGroups();
  for (std::size_t index = 0; index < groups.size(); ++index) {
    const auto& group = groups[index];
    if (std::find(group.begin(), group.end(), word) != group.end()) {
      return static_cast<int>(index);
    }
  }
  return -1;
}

bool wordsMatch(std::string_view queryWord, std::string_view fieldWord) {
  if (queryWord == fieldWord) {
    return true;
  }
  const int queryGroup = synonymGroupOf(queryWord);
  return queryGroup >= 0 && queryGroup == synonymGroupOf(fieldWord);
}

std::vector<std::string> queryTokens(std::string_view query) {
  std::vector<std::string> tokens;
  for (std::string& word : normalizeWords(query)) {
    if (!isStopword(word) && std::find(tokens.begin(), tokens.end(), word) == tokens.end()) {
      tokens.push_back(std::move(word));
    }
  }
  return tokens;
}

int fieldRank(const Capability& capability, std::string_view token) {
  int rank = 0;
  for (const std::string& word : normalizeWords(capability.id)) {
    if (wordsMatch(token, word)) {
      rank = std::max(rank, 4);
    }
  }
  for (const std::string& alias : capability.aliases) {
    for (const std::string& word : normalizeWords(alias)) {
      if (wordsMatch(token, word)) {
        rank = std::max(rank, 3);
      }
    }
  }
  for (const std::string& word : normalizeWords(capability.summary)) {
    if (wordsMatch(token, word)) {
      rank = std::max(rank, 2);
    }
  }
  for (const std::string& word : normalizeWords(capability.description)) {
    if (wordsMatch(token, word)) {
      rank = std::max(rank, 1);
    }
  }
  return rank;
}

int scoreStrict(const Capability& capability, const std::vector<std::string>& tokens, int& matched) {
  int highestRank = 0;
  int rankTotal = 0;
  matched = 0;
  for (const std::string& token : tokens) {
    const int rank = fieldRank(capability, token);
    if (rank > 0) {
      ++matched;
      highestRank = std::max(highestRank, rank);
      rankTotal += rank;
    }
  }
  if (matched != static_cast<int>(tokens.size())) {
    return -1;
  }
  // Uma correspondencia mais especifica domina os campos de menor prioridade.
  return highestRank * 10000 + rankTotal * 100 + matched;
}

int searchScore(const Capability& capability, const std::vector<std::string>& tokens) {
  if (tokens.empty()) {
    return 0;
  }
  int matched = 0;
  const int strict = scoreStrict(capability, tokens, matched);
  if (strict >= 0) {
    return strict;
  }
  // Reserva para consultas longas: aceita todos-menos-um token, penalizado,
  // para nunca devolver vazio quando ha relacao clara com a intencao.
  if (tokens.size() >= 3 && matched == static_cast<int>(tokens.size()) - 1) {
    int highestRank = 0;
    int rankTotal = 0;
    for (const std::string& token : tokens) {
      const int rank = fieldRank(capability, token);
      if (rank > 0) {
        highestRank = std::max(highestRank, rank);
        rankTotal += rank;
      }
    }
    return highestRank * 1000 + rankTotal * 10 + matched;
  }
  return -1;
}

}  // namespace

bool Registry::isValid(const Capability& capability) {
  if (capability.id.empty() || capability.type.empty() || capability.summary.empty() ||
      capability.id.find('\0') != std::string::npos ||
      capability.type.find('\0') != std::string::npos || capability.summary.find('\0') != std::string::npos ||
      capability.description.find('\0') != std::string::npos ||
      capability.implementation.kind.find('\0') != std::string::npos ||
      capability.implementation.entrypoint.find('\0') != std::string::npos) {
    return false;
  }
  if (capability.parent.has_value() &&
      (capability.parent->empty() || capability.parent->find('\0') != std::string::npos)) {
    return false;
  }

  std::vector<std::string> aliases;
  aliases.reserve(capability.aliases.size());
  for (const std::string& alias : capability.aliases) {
    if (alias.empty() || alias.find('\0') != std::string::npos || alias == capability.id) {
      return false;
    }
    if (std::find(aliases.begin(), aliases.end(), alias) != aliases.end()) {
      return false;
    }
    aliases.push_back(alias);
  }
  if (capability.type == "group") {
    return capability.implementation.kind.empty() && capability.implementation.entrypoint.empty();
  }
  return !capability.implementation.empty();
}

bool Registry::registerCapability(Capability capability) {
  if (!isValid(capability)) {
    return false;
  }

  std::string id = capability.id;
  std::unique_lock lock(mutex_);
  const auto [iterator, inserted] = capabilities_.try_emplace(std::move(id), std::move(capability));
  return inserted;
}

bool Registry::unregister(std::string_view id) {
  std::unique_lock lock(mutex_);
  const auto iterator = capabilities_.find(id);
  if (iterator == capabilities_.end()) {
    return false;
  }
  capabilities_.erase(iterator);
  return true;
}

bool Registry::update(Capability capability) {
  if (!isValid(capability)) {
    return false;
  }

  std::unique_lock lock(mutex_);
  const auto iterator = capabilities_.find(capability.id);
  if (iterator == capabilities_.end()) {
    return false;
  }
  iterator->second = std::move(capability);
  return true;
}

bool Registry::update(std::string_view id, Capability capability) {
  if (capability.id != id) {
    return false;
  }
  return update(std::move(capability));
}

std::optional<Capability> Registry::get(std::string_view id) const {
  return getDefinition(id);
}

std::optional<Capability> Registry::getDefinition(std::string_view id) const {
  std::shared_lock lock(mutex_);
  const auto iterator = capabilities_.find(id);
  if (iterator == capabilities_.end()) {
    return std::nullopt;
  }
  return iterator->second;
}

bool Registry::registerNativeEntrypoint(std::string entrypoint, NativeEntrypoint function) {
  if (entrypoint.empty() || entrypoint.find('\0') != std::string::npos || !function) {
    return false;
  }

  std::unique_lock lock(mutex_);
  return native_entrypoints_.try_emplace(std::move(entrypoint), std::move(function)).second;
}

// Copia o callback sob lock para permitir execucao sem reter o Registry bloqueado.
std::optional<NativeEntrypoint> Registry::resolveNativeEntrypoint(std::string_view entrypoint) const {
  std::shared_lock lock(mutex_);
  const auto iterator = native_entrypoints_.find(entrypoint);
  if (iterator == native_entrypoints_.end()) {
    return std::nullopt;
  }
  return iterator->second;
}

bool Registry::unregisterNativeEntrypoint(std::string_view entrypoint) {
  std::unique_lock lock(mutex_);
  const auto iterator = native_entrypoints_.find(entrypoint);
  if (iterator == native_entrypoints_.end()) {
    return false;
  }
  native_entrypoints_.erase(iterator);
  return true;
}

std::vector<Capability> Registry::list() const {
  std::shared_lock lock(mutex_);
  std::vector<Capability> result;
  result.reserve(capabilities_.size());
  for (const auto& entry : capabilities_) {
    result.push_back(entry.second);
  }
  return result;
}

std::vector<Capability> Registry::rootGroups() const {
  std::shared_lock lock(mutex_);
  std::vector<Capability> result;
  for (const auto& entry : capabilities_) {
    const Capability& capability = entry.second;
    if (capability.type == "group" && !capability.parent.has_value()) {
      result.push_back(capability);
    }
  }
  return result;
}

std::vector<Capability> Registry::children(std::string_view path) const {
  std::shared_lock lock(mutex_);
  std::vector<Capability> result;
  for (const auto& entry : capabilities_) {
    const Capability& capability = entry.second;
    const bool isChild = capability.parent.has_value()
        ? capability.parent.value() == path
        : path.empty();
    if (isChild) {
      result.push_back(capability);
    }
  }
  return result;
}

std::vector<Capability> Registry::search(
    std::string_view query,
    std::optional<std::size_t> limit) const {
  const std::vector<std::string> tokens = queryTokens(query);
  std::shared_lock lock(mutex_);
  struct ScoredCapability {
    Capability capability;
    int score;
  };
  std::vector<ScoredCapability> scored;
  for (const auto& entry : capabilities_) {
    const Capability& capability = entry.second;
    const int score = searchScore(capability, tokens);
    if (score >= 0) {
      scored.push_back({capability, score});
    }
  }
  // Reserva (todos-menos-um) so vale quando a intencao estrita nao casa nada:
  // havendo correspondencia total, parciais nao poluem o resultado.
  const bool hasStrict = tokens.empty() ||
      std::any_of(scored.begin(), scored.end(), [](const ScoredCapability& entry) {
        return entry.score >= 10000;
      });
  if (hasStrict && !tokens.empty()) {
    scored.erase(
        std::remove_if(
            scored.begin(),
            scored.end(),
            [](const ScoredCapability& entry) { return entry.score < 10000; }),
        scored.end());
  }

  std::sort(
      scored.begin(),
      scored.end(),
      [](const ScoredCapability& left, const ScoredCapability& right) {
        if (left.score != right.score) {
          return left.score > right.score;
        }
        return left.capability.id < right.capability.id;
      });

  std::vector<Capability> result;
  const std::size_t resultSize = limit.has_value() ? std::min(*limit, scored.size()) : scored.size();
  result.reserve(resultSize);
  for (std::size_t index = 0; index < resultSize; ++index) {
    result.push_back(std::move(scored[index].capability));
  }
  return result;
}

}  // namespace atlas::capabilities
