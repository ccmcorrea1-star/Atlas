#include "search.hpp"

#include <algorithm>
#include <unordered_map>
#include <unordered_set>
#include <utility>

namespace atlas::search {
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

// Token de consulta com a marca de termo tecnico/literal (arquivo, caminho,
// flag ou identificador). Termos tecnicos nao exigem cobertura por sinonimo.
struct QueryToken {
  std::string word;
  bool technical = false;
};

bool isPathBoundary(char character) {
  switch (character) {
    case '.': case '_': case '/': case '\\': case ':': case '@': case '-': case '~':
      return true;
    default:
      return false;
  }
}

std::vector<QueryToken> normalizeQueryTokens(std::string_view value) {
  std::vector<QueryToken> tokens;
  std::string current;
  bool uppercase = false;
  bool digit = false;
  char leadingBoundary = ' ';
  char previousBoundary = ' ';
  const std::size_t size = value.size();
  const auto flush = [&](char trailingBoundary) {
    if (current.empty()) {
      return;
    }
    const bool technical = uppercase || digit || isPathBoundary(leadingBoundary) ||
        isPathBoundary(trailingBoundary);
    tokens.push_back({std::move(current), technical});
    current.clear();
    uppercase = false;
    digit = false;
  };
  for (std::size_t index = 0; index < size;) {
    const auto character = static_cast<unsigned char>(value[index]);
    if (character < 0x80) {
      const bool isAlnum = (character >= 'a' && character <= 'z') || (character >= 'A' && character <= 'Z') ||
          (character >= '0' && character <= '9');
      if (isAlnum) {
        if (current.empty()) {
          leadingBoundary = previousBoundary;
        }
        if (character >= 'A' && character <= 'Z') {
          uppercase = true;
          current.push_back(static_cast<char>(character - 'A' + 'a'));
        } else {
          digit = digit || (character >= '0' && character <= '9');
          current.push_back(static_cast<char>(character));
        }
      } else {
        flush(static_cast<char>(character));
        previousBoundary = static_cast<char>(character);
      }
      ++index;
    } else if (character == 0xC3 && index + 1 < size) {
      if (current.empty()) {
        leadingBoundary = previousBoundary;
      }
      current += foldChar(value, index);
    } else {
      flush(' ');
      previousBoundary = ' ';
      ++index;
    }
  }
  flush(' ');
  return tokens;
}

/// Normaliza para busca por intencao: minusculas, sem acento, tokenizado em
/// palavras. Palavra inteira, nao substring: "ver" nao casa "server".
std::vector<std::string> normalizeWords(std::string_view value) {
  std::vector<std::string> words;
  for (QueryToken& token : normalizeQueryTokens(value)) {
    words.push_back(std::move(token.word));
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
  std::vector<std::string> intent;
  std::vector<std::string> literals;
  for (QueryToken& token : normalizeQueryTokens(query)) {
    if (isStopword(token.word)) {
      continue;
    }
    std::vector<std::string>& target = token.technical ? literals : intent;
    if (std::find(target.begin(), target.end(), token.word) == target.end()) {
      target.push_back(std::move(token.word));
    }
  }
  // Sem intencao natural, o literal ainda direciona a busca.
  if (intent.empty()) {
    return literals;
  }
  return intent;
}

int fieldCount(int fields) {
  int count = 0;
  while (fields != 0) {
    count += fields & 1;
    fields >>= 1;
  }
  return count;
}

struct FieldMatch {
  int rank = 0;
  int fields = 0;
};

/// Campos casados pelo token: id(8) > alias(4) > summary(2) > description(1).
FieldMatch fieldMatch(const Document& capability, std::string_view token) {
  FieldMatch match;
  for (const std::string& word : normalizeWords(capability.id)) {
    if (wordsMatch(token, word)) {
      match.rank = std::max(match.rank, 4);
      match.fields |= 8;
      break;
    }
  }
  for (const std::string& alias : capability.aliases) {
    const std::vector<std::string> words = normalizeWords(alias);
    const bool matched = std::any_of(
        words.begin(),
        words.end(),
        [token](const std::string& word) { return wordsMatch(token, word); });
    if (matched) {
      match.rank = std::max(match.rank, 3);
      match.fields |= 4;
    }
  }
  for (const std::string& word : normalizeWords(capability.summary)) {
    if (wordsMatch(token, word)) {
      match.rank = std::max(match.rank, 2);
      match.fields |= 2;
      break;
    }
  }
  for (const std::string& word : normalizeWords(capability.description)) {
    if (wordsMatch(token, word)) {
      match.rank = std::max(match.rank, 1);
      match.fields |= 1;
      break;
    }
  }
  return match;
}

/// Pontua por cobertura de tokens e forca do campo; exige todos os tokens de
/// intencao (ou todos-menos-um em consultas longas) para evitar ruido.
int searchScore(const Document& capability, const std::vector<std::string>& tokens, bool& full) {
  full = false;
  if (tokens.empty()) {
    full = true;
    return 0;
  }
  int matched = 0;
  int highestRank = 0;
  int rankTotal = 0;
  int fields = 0;
  for (const std::string& token : tokens) {
    const FieldMatch match = fieldMatch(capability, token);
    if (match.rank > 0) {
      ++matched;
      highestRank = std::max(highestRank, match.rank);
      rankTotal += match.rank;
      fields |= match.fields;
    }
  }
  if (matched == 0) {
    return -1;
  }
  full = matched == static_cast<int>(tokens.size());
  if (!full) {
    // Reserva para consultas longas: aceita todos-menos-um token, penalizado,
    // para nunca devolver vazio quando ha relacao clara com a intencao.
    const bool partialAllowed = tokens.size() >= 3 && matched == static_cast<int>(tokens.size()) - 1;
    if (!partialAllowed) {
      return -1;
    }
  }
  // Cobertura domina; depois forca do campo, amplitude de campos e soma.
  return matched * 1000000 + highestRank * 100000 + fieldCount(fields) * 1000 + rankTotal;
}

}  // namespace

Query::Query(std::string_view query) : tokens_(queryTokens(query)) {}

Match Query::match(const Document& document) const {
  bool full = false;
  const int score = searchScore(document, tokens_, full);
  return {score, full};
}

}  // namespace atlas::search
