#include "loader.hpp"

#include <algorithm>
#include <cctype>
#include <cstdint>
#include <fstream>
#include <iterator>
#include <map>
#include <set>
#include <stdexcept>
#include <system_error>
#include <utility>
#include <vector>

namespace atlas::capabilities {
namespace {

// Representa somente os tipos JSON necessarios para ler um manifesto.
struct JsonValue {
  enum class Kind {
    null_value,
    boolean,
    number,
    string,
    array,
    object,
  };

  Kind kind{Kind::null_value};
  bool boolean_value{false};
  std::string string_value;
  std::vector<JsonValue> array_value;
  std::map<std::string, JsonValue, std::less<>> object_value;
};

class JsonParser {
 public:
  explicit JsonParser(std::string_view source) : source_(source) {}

  JsonValue parse() {
    skipWhitespace();
    JsonValue value = parseValue();
    skipWhitespace();
    if (position_ != source_.size()) {
      fail("unexpected data after the JSON value");
    }
    return value;
  }

 private:
  [[noreturn]] void fail(const std::string& message) const {
    throw std::runtime_error(message + " at byte " + std::to_string(position_));
  }

  void skipWhitespace() {
    while (position_ < source_.size()) {
      const unsigned char character = static_cast<unsigned char>(source_[position_]);
      if (character != ' ' && character != '\t' && character != '\n' && character != '\r') {
        return;
      }
      ++position_;
    }
  }

  bool consume(char expected) {
    if (position_ < source_.size() && source_[position_] == expected) {
      ++position_;
      return true;
    }
    return false;
  }

  void expect(char expected) {
    if (!consume(expected)) {
      fail(std::string("expected '") + expected + "'");
    }
  }

  JsonValue parseValue() {
    skipWhitespace();
    if (position_ == source_.size()) {
      fail("expected a JSON value");
    }

    switch (source_[position_]) {
      case '{':
        return parseObject();
      case '[':
        return parseArray();
      case '"': {
        JsonValue value;
        value.kind = JsonValue::Kind::string;
        value.string_value = parseString();
        return value;
      }
      case 't':
        return parseLiteral("true", JsonValue::Kind::boolean, true);
      case 'f':
        return parseLiteral("false", JsonValue::Kind::boolean, false);
      case 'n':
        return parseLiteral("null", JsonValue::Kind::null_value, false);
      default:
        if (source_[position_] == '-' || std::isdigit(static_cast<unsigned char>(source_[position_]))) {
          return parseNumber();
        }
        fail("unexpected character in JSON value");
    }
  }

  JsonValue parseLiteral(std::string_view literal, JsonValue::Kind kind, bool booleanValue) {
    if (source_.substr(position_, literal.size()) != literal) {
      fail("invalid JSON literal");
    }
    position_ += literal.size();
    JsonValue value;
    value.kind = kind;
    value.boolean_value = booleanValue;
    return value;
  }

  JsonValue parseNumber() {
    const std::size_t start = position_;
    consume('-');
    if (consume('0')) {
      if (position_ < source_.size() && std::isdigit(static_cast<unsigned char>(source_[position_]))) {
        fail("leading zero in JSON number");
      }
    } else {
      if (position_ == source_.size() || source_[position_] < '1' || source_[position_] > '9') {
        fail("invalid JSON number");
      }
      while (position_ < source_.size() && std::isdigit(static_cast<unsigned char>(source_[position_]))) {
        ++position_;
      }
    }
    if (consume('.')) {
      if (position_ == source_.size() || !std::isdigit(static_cast<unsigned char>(source_[position_]))) {
        fail("invalid JSON number fraction");
      }
      while (position_ < source_.size() && std::isdigit(static_cast<unsigned char>(source_[position_]))) {
        ++position_;
      }
    }
    if (position_ < source_.size() && (source_[position_] == 'e' || source_[position_] == 'E')) {
      ++position_;
      if (position_ < source_.size() && (source_[position_] == '+' || source_[position_] == '-')) {
        ++position_;
      }
      if (position_ == source_.size() || !std::isdigit(static_cast<unsigned char>(source_[position_]))) {
        fail("invalid JSON number exponent");
      }
      while (position_ < source_.size() && std::isdigit(static_cast<unsigned char>(source_[position_]))) {
        ++position_;
      }
    }
    JsonValue value;
    value.kind = JsonValue::Kind::number;
    value.string_value = std::string(source_.substr(start, position_ - start));
    return value;
  }

  std::uint32_t parseHexCodePoint() {
    std::uint32_t codePoint = 0;
    for (int index = 0; index < 4; ++index) {
      if (position_ == source_.size()) {
        fail("incomplete Unicode escape");
      }
      const char character = source_[position_++];
      codePoint <<= 4;
      if (character >= '0' && character <= '9') {
        codePoint += static_cast<std::uint32_t>(character - '0');
      } else if (character >= 'a' && character <= 'f') {
        codePoint += static_cast<std::uint32_t>(character - 'a' + 10);
      } else if (character >= 'A' && character <= 'F') {
        codePoint += static_cast<std::uint32_t>(character - 'A' + 10);
      } else {
        fail("invalid Unicode escape");
      }
    }
    return codePoint;
  }

  static void appendCodePoint(std::string& result, std::uint32_t codePoint) {
    if (codePoint <= 0x7f) {
      result.push_back(static_cast<char>(codePoint));
    } else if (codePoint <= 0x7ff) {
      result.push_back(static_cast<char>(0xc0 | (codePoint >> 6)));
      result.push_back(static_cast<char>(0x80 | (codePoint & 0x3f)));
    } else if (codePoint <= 0xffff) {
      result.push_back(static_cast<char>(0xe0 | (codePoint >> 12)));
      result.push_back(static_cast<char>(0x80 | ((codePoint >> 6) & 0x3f)));
      result.push_back(static_cast<char>(0x80 | (codePoint & 0x3f)));
    } else {
      result.push_back(static_cast<char>(0xf0 | (codePoint >> 18)));
      result.push_back(static_cast<char>(0x80 | ((codePoint >> 12) & 0x3f)));
      result.push_back(static_cast<char>(0x80 | ((codePoint >> 6) & 0x3f)));
      result.push_back(static_cast<char>(0x80 | (codePoint & 0x3f)));
    }
  }

  std::string parseString() {
    expect('"');
    std::string result;
    while (position_ < source_.size()) {
      const unsigned char character = static_cast<unsigned char>(source_[position_++]);
      if (character == '"') {
        return result;
      }
      if (character < 0x20) {
        fail("unescaped control character in JSON string");
      }
      if (character != '\\') {
        result.push_back(static_cast<char>(character));
        continue;
      }
      if (position_ == source_.size()) {
        fail("incomplete JSON string escape");
      }
      const char escape = source_[position_++];
      switch (escape) {
        case '"':
        case '\\':
        case '/':
          result.push_back(escape);
          break;
        case 'b':
          result.push_back('\b');
          break;
        case 'f':
          result.push_back('\f');
          break;
        case 'n':
          result.push_back('\n');
          break;
        case 'r':
          result.push_back('\r');
          break;
        case 't':
          result.push_back('\t');
          break;
        case 'u': {
          std::uint32_t codePoint = parseHexCodePoint();
          if (codePoint >= 0xd800 && codePoint <= 0xdbff) {
            if (position_ + 5 > source_.size() || source_[position_] != '\\' || source_[position_ + 1] != 'u') {
              fail("high surrogate must be followed by a low surrogate");
            }
            position_ += 2;
            const std::uint32_t lowSurrogate = parseHexCodePoint();
            if (lowSurrogate < 0xdc00 || lowSurrogate > 0xdfff) {
              fail("invalid low surrogate");
            }
            codePoint = 0x10000 + ((codePoint - 0xd800) << 10) + (lowSurrogate - 0xdc00);
          } else if (codePoint >= 0xdc00 && codePoint <= 0xdfff) {
            fail("unexpected low surrogate");
          }
          appendCodePoint(result, codePoint);
          break;
        }
        default:
          fail("invalid JSON string escape");
      }
    }
    fail("unterminated JSON string");
  }

  JsonValue parseArray() {
    expect('[');
    JsonValue value;
    value.kind = JsonValue::Kind::array;
    skipWhitespace();
    if (consume(']')) {
      return value;
    }
    while (true) {
      value.array_value.push_back(parseValue());
      skipWhitespace();
      if (consume(']')) {
        return value;
      }
      expect(',');
      skipWhitespace();
    }
  }

  JsonValue parseObject() {
    expect('{');
    JsonValue value;
    value.kind = JsonValue::Kind::object;
    skipWhitespace();
    if (consume('}')) {
      return value;
    }
    while (true) {
      skipWhitespace();
      if (position_ == source_.size() || source_[position_] != '"') {
        fail("JSON object keys must be strings");
      }
      std::string key = parseString();
      skipWhitespace();
      expect(':');
      JsonValue member = parseValue();
      if (!value.object_value.emplace(std::move(key), std::move(member)).second) {
        fail("duplicate JSON object key");
      }
      skipWhitespace();
      if (consume('}')) {
        return value;
      }
      expect(',');
      skipWhitespace();
    }
  }

  std::string_view source_;
  std::size_t position_{0};
};

const JsonValue* member(const JsonValue& object, std::string_view name) {
  if (object.kind != JsonValue::Kind::object) {
    return nullptr;
  }
  const auto iterator = object.object_value.find(name);
  return iterator == object.object_value.end() ? nullptr : &iterator->second;
}

bool isString(const JsonValue* value) {
  return value != nullptr && value->kind == JsonValue::Kind::string;
}

bool supportedImplementationKind(std::string_view kind) {
  return kind == "native" || kind == "executable" || kind == "python" || kind == "service" || kind == "mcp";
}

bool hasNull(std::string_view value) {
  return value.find('\0') != std::string_view::npos;
}

bool validateCapability(const Capability& capability, std::string& error) {
  if (capability.id.empty() || hasNull(capability.id)) {
    error = "field 'id' must be a non-empty string";
    return false;
  }
  if (capability.type.empty() || hasNull(capability.type)) {
    error = "field 'type' must be a non-empty string";
    return false;
  }
  if (capability.summary.empty() || hasNull(capability.summary)) {
    error = "field 'summary' must be a non-empty string";
    return false;
  }
  if (capability.parent.has_value() && (capability.parent->empty() || hasNull(capability.parent.value()))) {
    error = "field 'parent' must be a non-empty string when present";
    return false;
  }
  if (capability.implementation.kind.empty() || !supportedImplementationKind(capability.implementation.kind)) {
    error = "field 'implementation.kind' has an unsupported value";
    return false;
  }
  if (capability.implementation.entrypoint.empty() || hasNull(capability.implementation.entrypoint)) {
    error = "field 'implementation.entrypoint' must be a non-empty string";
    return false;
  }

  std::set<std::string, std::less<>> aliases;
  for (const std::string& alias : capability.aliases) {
    if (alias.empty() || hasNull(alias) || alias == capability.id || !aliases.insert(alias).second) {
      error = "field 'aliases' contains an invalid or duplicated alias";
      return false;
    }
  }
  return true;
}

bool requiredString(
    const JsonValue& object,
    std::string_view name,
    std::string& target,
    std::string& error) {
  const JsonValue* value = member(object, name);
  if (!isString(value)) {
    error = "field '" + std::string(name) + "' must be a string";
    return false;
  }
  target = value->string_value;
  if (target.empty()) {
    error = "field '" + std::string(name) + "' must be non-empty";
    return false;
  }
  return true;
}

bool parseCapability(const JsonValue& root, Capability& capability, std::string& error) {
  if (root.kind != JsonValue::Kind::object) {
    error = "manifest root must be a JSON object";
    return false;
  }
  if (!requiredString(root, "id", capability.id, error) ||
      !requiredString(root, "type", capability.type, error) ||
      !requiredString(root, "summary", capability.summary, error)) {
    return false;
  }

  if (const JsonValue* parent = member(root, "parent"); parent != nullptr) {
    if (!isString(parent) || parent->string_value.empty()) {
      error = "field 'parent' must be a non-empty string when present";
      return false;
    }
    capability.parent = parent->string_value;
  }

  if (const JsonValue* aliases = member(root, "aliases"); aliases != nullptr) {
    if (aliases->kind != JsonValue::Kind::array) {
      error = "field 'aliases' must be an array of strings";
      return false;
    }
    for (const JsonValue& alias : aliases->array_value) {
      if (!isString(&alias)) {
        error = "field 'aliases' must be an array of strings";
        return false;
      }
      capability.aliases.push_back(alias.string_value);
    }
  }

  const JsonValue* implementation = member(root, "implementation");
  if (implementation == nullptr || implementation->kind != JsonValue::Kind::object) {
    error = "field 'implementation' must be an object";
    return false;
  }
  const JsonValue* kind = member(*implementation, "kind");
  const JsonValue* entrypoint = member(*implementation, "entrypoint");
  if (!isString(kind) || kind->string_value.empty()) {
    error = "field 'implementation.kind' must be a non-empty string";
    return false;
  }
  if (!supportedImplementationKind(kind->string_value)) {
    error = "field 'implementation.kind' has an unsupported value '" + kind->string_value + "'";
    return false;
  }
  if (!isString(entrypoint) || entrypoint->string_value.empty()) {
    error = "field 'implementation.entrypoint' must be a non-empty string";
    return false;
  }
  capability.implementation = CapabilityImplementation{kind->string_value, entrypoint->string_value};
  return validateCapability(capability, error);
}

}  // namespace

bool Loader::parseManifest(
    const std::filesystem::path& path,
    Capability& capability,
    std::string& error) {
  std::ifstream file(path, std::ios::binary);
  if (!file) {
    error = "cannot open manifest";
    return false;
  }
  const std::string contents{
      std::istreambuf_iterator<char>(file),
      std::istreambuf_iterator<char>()};
  if (file.bad()) {
    error = "cannot read manifest";
    return false;
  }

  try {
    const JsonValue root = JsonParser(contents).parse();
    return parseCapability(root, capability, error);
  } catch (const std::runtime_error& exception) {
    error = "invalid JSON: ";
    error += exception.what();
    return false;
  }
}

std::filesystem::path Loader::sourcePath(const std::filesystem::path& path) {
  std::error_code error;
  const std::filesystem::path absolute = std::filesystem::absolute(path, error);
  return (error ? path : absolute).lexically_normal();
}

bool Loader::fail(std::string message) {
  last_error_ = std::move(message);
  return false;
}

bool Loader::load(const std::filesystem::path& path) {
  last_error_.clear();
  Capability capability;
  std::string error;
  if (!parseManifest(path, capability, error)) {
    return fail("failed to load manifest '" + path.string() + "': " + error);
  }
  if (registry_.get(capability.id).has_value()) {
    return fail("capability '" + capability.id + "' is already registered");
  }
  const std::string id = capability.id;
  if (!registry_.registerCapability(std::move(capability))) {
    return fail("failed to register capability '" + id + "'");
  }
  sources_[id] = sourcePath(path);
  return true;
}

bool Loader::unload(std::string_view id) {
  last_error_.clear();
  if (id.empty()) {
    return fail("capability id cannot be empty");
  }
  if (!registry_.unregister(id)) {
    return fail("capability '" + std::string(id) + "' is not registered");
  }
  sources_.erase(std::string(id));
  return true;
}

bool Loader::reload(std::string_view id) {
  last_error_.clear();
  const auto source = sources_.find(id);
  if (source == sources_.end()) {
    return fail("capability '" + std::string(id) + "' has no loaded manifest");
  }

  Capability capability;
  std::string error;
  if (!parseManifest(source->second, capability, error)) {
    return fail("failed to reload capability '" + std::string(id) + "': " + error);
  }
  if (capability.id != id) {
    return fail("reloaded manifest id '" + capability.id + "' does not match '" + std::string(id) + "'");
  }
  if (!registry_.update(std::move(capability))) {
    return fail("failed to update capability '" + std::string(id) + "' in Registry");
  }
  return true;
}

bool Loader::scan(const std::filesystem::path& directory) {
  last_error_.clear();
  std::error_code errorCode;
  if (!std::filesystem::is_directory(directory, errorCode)) {
    return fail("cannot scan '" + directory.string() + "': directory does not exist");
  }

  std::vector<std::filesystem::path> manifests;
  std::filesystem::recursive_directory_iterator iterator(directory, errorCode);
  if (errorCode) {
    return fail("cannot scan '" + directory.string() + "': " + errorCode.message());
  }
  const std::filesystem::recursive_directory_iterator end;
  while (iterator != end) {
    std::error_code entryError;
    if (iterator->is_regular_file(entryError)) {
      if (iterator->path().filename() == "capability.json") {
        manifests.push_back(iterator->path());
      }
    } else if (entryError) {
      return fail("cannot inspect '" + iterator->path().string() + "': " + entryError.message());
    }
    iterator.increment(errorCode);
    if (errorCode) {
      return fail("cannot scan '" + directory.string() + "': " + errorCode.message());
    }
  }
  std::sort(manifests.begin(), manifests.end());

  std::string firstError;
  bool success = true;
  for (const std::filesystem::path& manifest : manifests) {
    if (!load(manifest)) {
      success = false;
      if (firstError.empty()) {
        firstError = last_error_;
      }
    }
  }
  if (!success) {
    last_error_ = std::move(firstError);
  }
  return success;
}

}  // namespace atlas::capabilities
