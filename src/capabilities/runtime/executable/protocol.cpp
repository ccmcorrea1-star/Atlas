#include "protocol.hpp"

#include <charconv>
#include <cmath>
#include <cstdint>
#include <exception>
#include <iomanip>
#include <limits>
#include <sstream>
#include <stdexcept>
#include <type_traits>
#include <utility>

namespace atlas::capabilities {
namespace {

// Implementa apenas os tipos JSON usados pelo contrato de execucao.
class JsonParser {
 public:
  explicit JsonParser(std::string_view source) : source_(source) {}

  StructuredValue parse() {
    skipWhitespace();
    StructuredValue value = parseValue();
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

  StructuredValue parseValue() {
    skipWhitespace();
    if (position_ == source_.size()) {
      fail("expected a JSON value");
    }

    switch (source_[position_]) {
      case '{':
        return parseObject();
      case '[':
        return parseArray();
      case '"':
        return parseStringValue();
      case 't':
        return parseLiteral("true", StructuredValue(true));
      case 'f':
        return parseLiteral("false", StructuredValue(false));
      case 'n':
        return parseLiteral("null", StructuredValue(nullptr));
      default:
        if (source_[position_] == '-' || isDigit(source_[position_])) {
          return parseNumber();
        }
        fail("unexpected character in JSON value");
    }
  }

  StructuredValue parseLiteral(std::string_view literal, StructuredValue value) {
    if (source_.substr(position_, literal.size()) != literal) {
      fail("invalid JSON literal");
    }
    position_ += literal.size();
    return value;
  }

  StructuredValue parseNumber() {
    const std::size_t start = position_;
    consume('-');
    if (consume('0')) {
      if (position_ < source_.size() && isDigit(source_[position_])) {
        fail("leading zero in JSON number");
      }
    } else {
      if (position_ == source_.size() || source_[position_] < '1' || source_[position_] > '9') {
        fail("invalid JSON number");
      }
      while (position_ < source_.size() && isDigit(source_[position_])) {
        ++position_;
      }
    }
    bool floating = false;
    if (consume('.')) {
      floating = true;
      if (position_ == source_.size() || !isDigit(source_[position_])) {
        fail("invalid JSON number fraction");
      }
      while (position_ < source_.size() && isDigit(source_[position_])) {
        ++position_;
      }
    }
    if (position_ < source_.size() && (source_[position_] == 'e' || source_[position_] == 'E')) {
      floating = true;
      ++position_;
      if (position_ < source_.size() && (source_[position_] == '+' || source_[position_] == '-')) {
        ++position_;
      }
      if (position_ == source_.size() || !isDigit(source_[position_])) {
        fail("invalid JSON number exponent");
      }
      while (position_ < source_.size() && isDigit(source_[position_])) {
        ++position_;
      }
    }

    const std::string number(source_.substr(start, position_ - start));
    if (!floating) {
      std::int64_t integer = 0;
      const auto parsed = std::from_chars(number.data(), number.data() + number.size(), integer);
      if (parsed.ec == std::errc{} && parsed.ptr == number.data() + number.size()) {
        return StructuredValue(integer);
      }
    }

    double decimal = 0.0;
    const auto parsed = std::from_chars(
        number.data(), number.data() + number.size(), decimal, std::chars_format::general);
    if (parsed.ec != std::errc{} || parsed.ptr != number.data() + number.size() || !std::isfinite(decimal)) {
      fail("JSON number is outside the supported range");
    }
    return StructuredValue(decimal);
  }

  static bool isDigit(char character) {
    return character >= '0' && character <= '9';
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
            if (position_ + 6 > source_.size() || source_[position_] != '\\' || source_[position_ + 1] != 'u') {
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

  StructuredValue parseStringValue() {
    return StructuredValue(parseString());
  }

  StructuredValue parseArray() {
    expect('[');
    StructuredValue::Array value;
    skipWhitespace();
    if (consume(']')) {
      return StructuredValue(std::move(value));
    }
    while (true) {
      value.push_back(parseValue());
      skipWhitespace();
      if (consume(']')) {
        return StructuredValue(std::move(value));
      }
      expect(',');
      skipWhitespace();
    }
  }

  StructuredValue parseObject() {
    expect('{');
    StructuredValue::Object value;
    skipWhitespace();
    if (consume('}')) {
      return StructuredValue(std::move(value));
    }
    while (true) {
      skipWhitespace();
      if (position_ == source_.size() || source_[position_] != '"') {
        fail("JSON object keys must be strings");
      }
      std::string key = parseString();
      skipWhitespace();
      expect(':');
      StructuredValue member = parseValue();
      if (!value.emplace(std::move(key), std::move(member)).second) {
        fail("duplicate JSON object key");
      }
      skipWhitespace();
      if (consume('}')) {
        return StructuredValue(std::move(value));
      }
      expect(',');
      skipWhitespace();
    }
  }

  std::string_view source_;
  std::size_t position_{0};
};

void appendEscapedJson(std::string& output, std::string_view value) {
  output.push_back('"');
  constexpr char hex[] = "0123456789abcdef";
  for (const unsigned char character : value) {
    switch (character) {
      case '"':
        output += "\\\"";
        break;
      case '\\':
        output += "\\\\";
        break;
      case '\b':
        output += "\\b";
        break;
      case '\f':
        output += "\\f";
        break;
      case '\n':
        output += "\\n";
        break;
      case '\r':
        output += "\\r";
        break;
      case '\t':
        output += "\\t";
        break;
      default:
        if (character < 0x20) {
          output += "\\u00";
          output.push_back(hex[character >> 4]);
          output.push_back(hex[character & 0x0f]);
        } else {
          output.push_back(static_cast<char>(character));
        }
        break;
    }
  }
  output.push_back('"');
}

void appendJson(std::string& output, const StructuredValue& value) {
  std::visit(
      [&output](const auto& current) {
        using Value = std::decay_t<decltype(current)>;
        if constexpr (std::is_same_v<Value, std::nullptr_t>) {
          output += "null";
        } else if constexpr (std::is_same_v<Value, bool>) {
          output += current ? "true" : "false";
        } else if constexpr (std::is_same_v<Value, std::int64_t>) {
          output += std::to_string(current);
        } else if constexpr (std::is_same_v<Value, double>) {
          if (!std::isfinite(current)) {
            output += "null";
            return;
          }
          std::ostringstream number;
          number << std::setprecision(std::numeric_limits<double>::max_digits10) << current;
          output += number.str();
        } else if constexpr (std::is_same_v<Value, std::string>) {
          appendEscapedJson(output, current);
        } else if constexpr (std::is_same_v<Value, StructuredValue::Array>) {
          output.push_back('[');
          bool first = true;
          for (const StructuredValue& item : current) {
            if (!first) {
              output.push_back(',');
            }
            first = false;
            appendJson(output, item);
          }
          output.push_back(']');
        } else if constexpr (std::is_same_v<Value, StructuredValue::Object>) {
          output.push_back('{');
          bool first = true;
          for (const auto& [key, item] : current) {
            if (!first) {
              output.push_back(',');
            }
            first = false;
            appendEscapedJson(output, key);
            output.push_back(':');
            appendJson(output, item);
          }
          output.push_back('}');
        }
      },
      value.value);
}

}  // namespace

std::string serializeJson(const StructuredValue& value) {
  std::string output;
  appendJson(output, value);
  return output;
}

std::optional<StructuredValue> parseJson(std::string_view source, std::string& error) {
  try {
    error.clear();
    return JsonParser(source).parse();
  } catch (const std::exception& exception) {
    error = exception.what();
    return std::nullopt;
  }
}

const char* executionStatusName(ExecutionStatus status) noexcept {
  switch (status) {
    case ExecutionStatus::success:
      return "success";
    case ExecutionStatus::failed:
      return "failed";
    case ExecutionStatus::timed_out:
      return "timed_out";
    case ExecutionStatus::unavailable:
      return "unavailable";
  }
  return "failed";
}

}  // namespace atlas::capabilities

namespace atlas::capabilities::runtime::executable {

RequestParseResult parseRequest(std::string_view source) {
  RequestParseResult result;
  std::string parseError;
  const auto parsed = parseJson(source, parseError);
  if (!parsed.has_value()) {
    result.error = "invalid JSON request: " + parseError;
    return result;
  }

  const auto* object = std::get_if<StructuredValue::Object>(&parsed->value);
  if (object == nullptr) {
    result.error = "request must be a JSON object";
    return result;
  }

  const auto targetValue = object->find("target");
  const auto* target = targetValue == object->end()
      ? nullptr
      : std::get_if<std::string>(&targetValue->second.value);
  if (target == nullptr) {
    result.error = "field 'target' must be a string";
    return result;
  }

  result.target = *target;
  NativeRequest request;
  request.target = *target;
  request.arguments = *object;
  request.arguments.erase("target");
  result.request = std::move(request);
  return result;
}

StructuredValue responseValue(const ExecutionResult& result) {
  StructuredValue::Object response;
  if (const auto* output = std::get_if<StructuredValue::Object>(&result.output.value); output != nullptr) {
    response.insert(output->begin(), output->end());
  }
  response["target"] = result.target;
  response["status"] = executionStatusName(result.status);
  response["error"] = result.error;
  return StructuredValue(std::move(response));
}

}  // namespace atlas::capabilities::runtime::executable
