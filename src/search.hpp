#pragma once

#include <span>
#include <string>
#include <string_view>
#include <vector>

namespace atlas::search {

// Metadados pesquisáveis, sem dependência de Tools ou Skills.
struct Document {
  std::string_view id;
  std::string_view summary;
  std::span<const std::string> aliases = {};
  std::string_view description = {};
};

struct Match {
  int score;
  bool full;
};

class Query {
 public:
  explicit Query(std::string_view query);
  Match match(const Document& document) const;

 private:
  std::vector<std::string> tokens_;
};

}  // namespace atlas::search
