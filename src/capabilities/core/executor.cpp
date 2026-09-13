#include "executor.hpp"

#include <array>
#include <cerrno>
#include <charconv>
#include <cmath>
#include <cstdint>
#include <cstdlib>
#include <cstring>
#include <exception>
#include <fcntl.h>
#include <iomanip>
#include <limits>
#include <poll.h>
#include <signal.h>
#include <sstream>
#include <stdexcept>
#include <sys/wait.h>
#include <type_traits>
#include <unistd.h>
#include <utility>
#include <vector>

extern char** environ;

namespace atlas::capabilities {
namespace {

// Compartilha um codec pequeno para o contrato das implementacoes executaveis.
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

struct Pipes {
  int input_read{-1};
  int input_write{-1};
  int output_read{-1};
  int output_write{-1};
  int error_read{-1};
  int error_write{-1};
};

class SigpipeGuard {
 public:
  SigpipeGuard() {
    struct sigaction ignored {};
    ignored.sa_handler = SIG_IGN;
    sigemptyset(&ignored.sa_mask);
    installed_ = sigaction(SIGPIPE, &ignored, &previous_) == 0;
  }

  ~SigpipeGuard() {
    if (installed_) {
      sigaction(SIGPIPE, &previous_, nullptr);
    }
  }

  SigpipeGuard(const SigpipeGuard&) = delete;
  SigpipeGuard& operator=(const SigpipeGuard&) = delete;

 private:
  struct sigaction previous_ {};
  bool installed_{false};
};

void closeFd(int& fd) {
  if (fd != -1) {
    close(fd);
    fd = -1;
  }
}

void closePipes(Pipes& pipes) {
  closeFd(pipes.input_read);
  closeFd(pipes.input_write);
  closeFd(pipes.output_read);
  closeFd(pipes.output_write);
  closeFd(pipes.error_read);
  closeFd(pipes.error_write);
}

bool setCloseOnExec(int fd) {
  const int flags = fcntl(fd, F_GETFD);
  return flags != -1 && fcntl(fd, F_SETFD, flags | FD_CLOEXEC) != -1;
}

bool setNonBlocking(int fd) {
  const int flags = fcntl(fd, F_GETFL);
  return flags != -1 && fcntl(fd, F_SETFL, flags | O_NONBLOCK) != -1;
}

bool createPipe(int (&pipeFds)[2]) {
  if (pipe(pipeFds) == -1) {
    return false;
  }
  for (int& fd : pipeFds) {
    if (!setCloseOnExec(fd)) {
      close(pipeFds[0]);
      close(pipeFds[1]);
      pipeFds[0] = -1;
      pipeFds[1] = -1;
      return false;
    }
  }
  return true;
}

void executeDirectly(const std::string& program, char* const argv[]) {
  if (program.find('/') != std::string::npos) {
    execve(program.c_str(), argv, environ);
    _exit(127);
  }

  const char* path = getenv("PATH");
  if (path == nullptr) {
    path = "/bin:/usr/bin";
  }
  std::string_view remaining(path);
  while (true) {
    const std::size_t separator = remaining.find(':');
    const std::string_view directory = remaining.substr(0, separator);
    std::string candidate(directory.empty() ? "." : std::string(directory));
    candidate.push_back('/');
    candidate += program;
    execve(candidate.c_str(), argv, environ);
    if (separator == std::string_view::npos) {
      break;
    }
    remaining.remove_prefix(separator + 1);
  }
  _exit(127);
}

void prepareChild(const std::string& program, const Pipes& pipes) {
  if (dup2(pipes.input_read, STDIN_FILENO) == -1 ||
      dup2(pipes.output_write, STDOUT_FILENO) == -1 ||
      dup2(pipes.error_write, STDERR_FILENO) == -1) {
    _exit(127);
  }
  close(pipes.input_read);
  close(pipes.input_write);
  close(pipes.output_read);
  close(pipes.output_write);
  close(pipes.error_read);
  close(pipes.error_write);

  std::vector<char*> argv;
  argv.push_back(const_cast<char*>(program.c_str()));
  argv.push_back(nullptr);
  executeDirectly(program, argv.data());
}

bool drainPipe(int& fd, std::string& output, std::string& error) {
  std::array<char, 4096> buffer{};
  while (fd != -1) {
    const ssize_t count = read(fd, buffer.data(), buffer.size());
    if (count > 0) {
      output.append(buffer.data(), static_cast<std::size_t>(count));
      continue;
    }
    if (count == 0) {
      closeFd(fd);
      return true;
    }
    if (errno == EINTR) {
      continue;
    }
    if (errno == EAGAIN || errno == EWOULDBLOCK) {
      return true;
    }
    error = std::strerror(errno);
    closeFd(fd);
    return false;
  }
  return true;
}

bool allClosed(const Pipes& pipes) {
  return pipes.input_write == -1 && pipes.output_read == -1 && pipes.error_read == -1;
}

std::optional<std::string> runExecutable(
    std::string_view program,
    std::string payload,
    std::string& standardError,
    int& exitCode,
    std::string& error) {
  // Le e escreve em paralelo para evitar bloquear em qualquer pipe do processo.
  int inputPipe[2] = {-1, -1};
  int outputPipe[2] = {-1, -1};
  int errorPipe[2] = {-1, -1};
  Pipes pipes;
  if (!createPipe(inputPipe) || !createPipe(outputPipe) || !createPipe(errorPipe)) {
    if (inputPipe[0] != -1) {
      close(inputPipe[0]);
      close(inputPipe[1]);
    }
    if (outputPipe[0] != -1) {
      close(outputPipe[0]);
      close(outputPipe[1]);
    }
    if (errorPipe[0] != -1) {
      close(errorPipe[0]);
      close(errorPipe[1]);
    }
    error = "failed to create executable pipes";
    return std::nullopt;
  }
  pipes.input_read = inputPipe[0];
  pipes.input_write = inputPipe[1];
  pipes.output_read = outputPipe[0];
  pipes.output_write = outputPipe[1];
  pipes.error_read = errorPipe[0];
  pipes.error_write = errorPipe[1];

  if (!setNonBlocking(pipes.input_write) || !setNonBlocking(pipes.output_read) ||
      !setNonBlocking(pipes.error_read)) {
    error = "failed to configure executable pipes";
    closePipes(pipes);
    return std::nullopt;
  }

  const pid_t childPid = fork();
  if (childPid == -1) {
    error = "failed to fork executable";
    closePipes(pipes);
    return std::nullopt;
  }
  if (childPid == 0) {
    prepareChild(std::string(program), pipes);
  }

  closeFd(pipes.input_read);
  closeFd(pipes.output_write);
  closeFd(pipes.error_write);
  SigpipeGuard sigpipeGuard;

  std::string output;
  std::size_t written = 0;
  bool childReaped = false;
  int waitStatus = 0;
  while (!childReaped || !allClosed(pipes)) {
    drainPipe(pipes.output_read, output, error);
    drainPipe(pipes.error_read, standardError, error);

    if (pipes.input_write != -1 && written < payload.size()) {
      const ssize_t count = write(pipes.input_write, payload.data() + written, payload.size() - written);
      if (count > 0) {
        written += static_cast<std::size_t>(count);
      } else if (count == -1 && errno != EINTR && errno != EAGAIN && errno != EWOULDBLOCK) {
        error = "failed to write executable input";
        closeFd(pipes.input_write);
      }
    }
    if (pipes.input_write != -1 && written == payload.size()) {
      closeFd(pipes.input_write);
    }

    if (!childReaped) {
      const pid_t waitResult = waitpid(childPid, &waitStatus, WNOHANG);
      if (waitResult == childPid) {
        childReaped = true;
        closeFd(pipes.input_write);
      } else if (waitResult == -1 && errno != EINTR) {
        error = "failed to wait for executable";
        kill(childPid, SIGKILL);
        while (waitpid(childPid, &waitStatus, 0) == -1 && errno == EINTR) {
        }
        childReaped = true;
        closeFd(pipes.input_write);
      }
    }

    if (!childReaped || !allClosed(pipes)) {
      pollfd pollFds[3]{};
      nfds_t pollCount = 0;
      if (pipes.input_write != -1) {
        pollFds[pollCount++] = {pipes.input_write, POLLOUT, 0};
      }
      if (pipes.output_read != -1) {
        pollFds[pollCount++] = {pipes.output_read, POLLIN | POLLHUP, 0};
      }
      if (pipes.error_read != -1) {
        pollFds[pollCount++] = {pipes.error_read, POLLIN | POLLHUP, 0};
      }
      poll(pollFds, pollCount, 20);
    }
  }
  drainPipe(pipes.output_read, output, error);
  drainPipe(pipes.error_read, standardError, error);
  closePipes(pipes);

  exitCode = WIFEXITED(waitStatus) ? WEXITSTATUS(waitStatus) : 128 + WTERMSIG(waitStatus);
  if (!error.empty()) {
    return std::nullopt;
  }
  return output;
}

ExecutionStatus statusFromOutput(const StructuredValue& output) {
  const auto* object = std::get_if<StructuredValue::Object>(&output.value);
  if (object == nullptr) {
    return ExecutionStatus::success;
  }
  const auto status = object->find("status");
  if (status == object->end()) {
    return ExecutionStatus::success;
  }
  const auto* name = std::get_if<std::string>(&status->second.value);
  if (name == nullptr || *name == "success") {
    return ExecutionStatus::success;
  }
  if (*name == "timed_out") {
    return ExecutionStatus::timed_out;
  }
  if (*name == "unavailable") {
    return ExecutionStatus::unavailable;
  }
  return ExecutionStatus::failed;
}

std::string outputError(const StructuredValue& output) {
  const auto* object = std::get_if<StructuredValue::Object>(&output.value);
  if (object == nullptr) {
    return {};
  }
  const auto error = object->find("error");
  if (error == object->end()) {
    return {};
  }
  const auto* message = std::get_if<std::string>(&error->second.value);
  return message == nullptr ? std::string{} : *message;
}

ExecutionResult executeExecutable(
    const ExecutionRequest& request,
    const CapabilityImplementation& implementation) {
  // O adaptador recebe os argumentos como um objeto JSON e nunca usa shell.
  if (implementation.entrypoint.find('\0') != std::string::npos) {
    return {request.target, ExecutionStatus::failed, {}, "executable entrypoint cannot contain NUL bytes"};
  }

  StructuredValue::Object input = request.arguments;
  input["target"] = request.target;
  std::string standardError;
  std::string processError;
  int exitCode = -1;
  const auto output = runExecutable(
      implementation.entrypoint,
      serializeJson(StructuredValue(std::move(input))) + "\n",
      standardError,
      exitCode,
      processError);
  if (!output.has_value()) {
    const std::string detail = processError.empty() ? standardError : processError;
    return {
        request.target,
        ExecutionStatus::failed,
        {},
        "failed to execute capability executable '" + implementation.entrypoint + "'" +
            (detail.empty() ? std::string{} : ": " + detail),
    };
  }
  if (exitCode != 0) {
    return {
        request.target,
        ExecutionStatus::failed,
        {},
        "capability executable '" + implementation.entrypoint + "' exited with code " +
            std::to_string(exitCode),
    };
  }

  std::string parseError;
  const auto parsed = parseJson(output.value(), parseError);
  if (!parsed.has_value()) {
    return {
        request.target,
        ExecutionStatus::failed,
        {},
        "capability executable '" + implementation.entrypoint + "' returned invalid JSON: " + parseError,
    };
  }
  ExecutionResult result;
  result.target = request.target;
  result.output = parsed.value();
  result.status = statusFromOutput(result.output);
  result.error = outputError(result.output);
  return result;
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

ExecutionResult Executor::failure(std::string target, std::string error) {
  ExecutionResult result;
  result.target = std::move(target);
  result.status = ExecutionStatus::failed;
  result.error = std::move(error);
  return result;
}

ExecutionResult Executor::unavailable(std::string target, std::string kind) {
  ExecutionResult result;
  result.target = std::move(target);
  result.status = ExecutionStatus::unavailable;
  result.error = "executor adapter for implementation kind '" + kind + "' is not available";
  return result;
}

ExecutionResult Executor::execute(const ExecutionRequest& request) const {
  if (request.capability_id.empty()) {
    return failure(request.target, "capability id cannot be empty");
  }

  const auto capability = registry_.get(request.capability_id);
  if (!capability.has_value()) {
    return failure(request.target, "capability '" + request.capability_id + "' is not registered");
  }

  const CapabilityImplementation& implementation = capability->implementation;
  if (implementation.empty()) {
    return failure(
        request.target,
        "capability '" + request.capability_id + "' has no valid implementation");
  }
  if (implementation.kind == "executable") {
    return executeExecutable(request, implementation);
  }
  if (implementation.kind != "native") {
    return unavailable(request.target, implementation.kind);
  }

  const auto entrypoint = registry_.resolveNativeEntrypoint(implementation.entrypoint);
  if (!entrypoint.has_value()) {
    return failure(
        request.target,
        "native entrypoint '" + implementation.entrypoint + "' is not registered");
  }

  try {
    const NativeRequest nativeRequest{request.target, request.arguments};
    ExecutionResult result = entrypoint.value()(nativeRequest);
    result.target = request.target;
    return result;
  } catch (const std::exception& exception) {
    return failure(
        request.target,
        "native entrypoint '" + implementation.entrypoint + "' threw an exception: " +
            exception.what());
  } catch (...) {
    return failure(
        request.target,
        "native entrypoint '" + implementation.entrypoint + "' threw an unknown exception");
  }
}

ExecutionResult Executor::execute(
    std::string_view capability_id,
    std::string target,
    StructuredArguments arguments) const {
  return execute({std::string(capability_id), std::move(target), std::move(arguments)});
}

}  // namespace atlas::capabilities
