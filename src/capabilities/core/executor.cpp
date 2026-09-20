#include "executor.hpp"

#include "../runtime/executable/protocol.hpp"
#include "schema.hpp"

#include <algorithm>
#include <array>
#include <cerrno>
#include <cstdlib>
#include <cstring>
#include <exception>
#include <fcntl.h>
#include <poll.h>
#include <signal.h>
#include <sys/wait.h>
#include <unistd.h>
#include <utility>
#include <vector>

extern char** environ;

namespace atlas::capabilities {
namespace {

constexpr std::size_t kMaximumExecutableOutputBytes = 8 * 1024 * 1024;

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

void consumeOutputEvents(
    std::string& buffer,
    std::string_view chunk,
    const ExecutionOutputCallback& on_output) {
  buffer.append(chunk.data(), chunk.size());
  while (true) {
    const std::size_t newline = buffer.find('\n');
    if (newline == std::string::npos) {
      return;
    }
    const std::string line = buffer.substr(0, newline);
    buffer.erase(0, newline + 1);
    if (!on_output || line.empty()) {
      continue;
    }
    std::string parseError;
    const auto parsed = parseJson(line, parseError);
    if (!parsed.has_value()) {
      continue;
    }
    const auto* object = std::get_if<StructuredValue::Object>(&parsed->value);
    if (object == nullptr) {
      continue;
    }
    const auto event = object->find("event");
    const auto channel = object->find("channel");
    const auto delta = object->find("delta");
    if (event == object->end() || channel == object->end() || delta == object->end()) {
      continue;
    }
    const auto* eventName = std::get_if<std::string>(&event->second.value);
    const auto* channelName = std::get_if<std::string>(&channel->second.value);
    const auto* deltaValue = std::get_if<std::string>(&delta->second.value);
    if (eventName != nullptr && *eventName == "execution.output.delta" &&
        channelName != nullptr && deltaValue != nullptr) {
      on_output(*channelName, *deltaValue);
    }
  }
}

bool drainPipe(
    int& fd,
    std::string& output,
    bool& truncated,
    std::string& error,
    std::string* eventBuffer = nullptr,
    const ExecutionOutputCallback& on_output = {}) {
  std::array<char, 4096> buffer{};
  while (fd != -1) {
    const ssize_t count = read(fd, buffer.data(), buffer.size());
    if (count > 0) {
      const std::size_t size = static_cast<std::size_t>(count);
      const std::size_t remaining = output.size() < kMaximumExecutableOutputBytes
          ? kMaximumExecutableOutputBytes - output.size()
          : 0;
      const std::size_t captured = std::min(size, remaining);
      if (captured > 0) {
        output.append(buffer.data(), captured);
        if (eventBuffer != nullptr) {
          consumeOutputEvents(*eventBuffer, std::string_view(buffer.data(), captured), on_output);
        }
      }
      if (captured < size) {
        truncated = true;
      }
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
    std::string& error,
    const ExecutionOutputCallback& on_output) {
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
  std::string eventBuffer;
  bool outputTruncated = false;
  bool errorTruncated = false;
  std::size_t written = 0;
  bool childReaped = false;
  int waitStatus = 0;
  while (!childReaped || !allClosed(pipes)) {
    drainPipe(pipes.output_read, output, outputTruncated, error, &eventBuffer, on_output);
    drainPipe(pipes.error_read, standardError, errorTruncated, error);

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
  drainPipe(pipes.output_read, output, outputTruncated, error, &eventBuffer, on_output);
  drainPipe(pipes.error_read, standardError, errorTruncated, error);
  closePipes(pipes);

  exitCode = WIFEXITED(waitStatus) ? WEXITSTATUS(waitStatus) : 128 + WTERMSIG(waitStatus);
  if (outputTruncated || errorTruncated) {
    error = "capability executable output exceeded the 8 MiB capture limit";
  }
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
    const CapabilityImplementation& implementation,
    const ExecutionOutputCallback& on_output) {
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
      processError,
      on_output);
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

  std::size_t end = output->size();
  while (end > 0 && ((*output)[end - 1] == '\n' || (*output)[end - 1] == '\r')) {
    --end;
  }
  const std::size_t lastLine = end == 0 ? std::string::npos : output->rfind('\n', end - 1);
  const std::string response = lastLine == std::string::npos
      ? output->substr(0, end)
      : output->substr(lastLine + 1, end - lastLine - 1);
  std::string parseError;
  const auto parsed = parseJson(response, parseError);
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

ExecutionResult Executor::execute(
    const ExecutionRequest& request,
    const ExecutionOutputCallback& on_output) const {
  if (request.capability_id.empty()) {
    return failure(request.target, "capability id cannot be empty");
  }

  const auto capability = registry_.get(request.capability_id);
  if (!capability.has_value()) {
    return failure(request.target, "capability '" + request.capability_id + "' is not registered");
  }

  if (capability->type != "tool") {
    return failure(
        request.target,
        "capability '" + request.capability_id + "' of type '" + capability->type + "' is not executable");
  }

  if (const auto validationError = validateArguments(request.arguments, capability->schema);
      validationError.has_value()) {
    return failure(
        request.target,
        "invalid arguments for capability '" + request.capability_id + "': " + validationError.value());
  }

  const CapabilityImplementation& implementation = capability->implementation;
  if (implementation.empty()) {
    return failure(
        request.target,
        "capability '" + request.capability_id + "' has no valid implementation");
  }
  if (implementation.kind == "executable") {
    return executeExecutable(request, implementation, on_output);
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
    StructuredArguments arguments,
    const ExecutionOutputCallback& on_output) const {
  return execute(
      {std::string(capability_id), std::move(target), std::move(arguments)},
      on_output);
}

}  // namespace atlas::capabilities
