#include "exec.hpp"

#include <algorithm>
#include <array>
#include <cerrno>
#include <cstring>
#include <cstdlib>
#include <fcntl.h>
#include <optional>
#include <poll.h>
#include <signal.h>
#include <string_view>
#include <sys/wait.h>
#include <unistd.h>
#include <utility>

extern char** environ;

namespace atlas::capabilities::tools::process {
namespace {

constexpr int kChildFailureExitCode = 127;

enum class ChildErrorStage : int {
  change_directory = 1,
  redirect_stdin = 2,
  redirect_stdout = 3,
  redirect_stderr = 4,
  execute = 5,
};

struct ChildError {
  int stage;
  int error_code;
};

struct Pipes {
  int stdout_read{-1};
  int stdout_write{-1};
  int stderr_read{-1};
  int stderr_write{-1};
  int error_read{-1};
  int error_write{-1};
};

void closeFd(int& fd) {
  if (fd != -1) {
    close(fd);
    fd = -1;
  }
}

void closeAllPipes(Pipes& pipes) {
  closeFd(pipes.stdout_read);
  closeFd(pipes.stdout_write);
  closeFd(pipes.stderr_read);
  closeFd(pipes.stderr_write);
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

// Mantem os descritores acima de stdin, stdout e stderr para o fork ser seguro.
bool createPipe(int (&pipe_fds)[2]) {
  if (pipe(pipe_fds) == -1) {
    return false;
  }

  for (int& pipe_fd : pipe_fds) {
    const int normalized_fd = fcntl(pipe_fd, F_DUPFD_CLOEXEC, STDERR_FILENO + 1);
    if (normalized_fd == -1) {
      close(pipe_fds[0]);
      close(pipe_fds[1]);
      pipe_fds[0] = -1;
      pipe_fds[1] = -1;
      return false;
    }
    close(pipe_fd);
    pipe_fd = normalized_fd;
  }
  return true;
}

bool containsNull(const std::string& value) {
  return value.find('\0') != std::string::npos;
}

std::string errnoMessage(int error_code) {
  const char* message = std::strerror(error_code);
  return message == nullptr ? "unknown error" : message;
}

void setDuration(ExecResult& result, std::chrono::steady_clock::time_point started) {
  result.duration = std::chrono::duration_cast<std::chrono::milliseconds>(
      std::chrono::steady_clock::now() - started);
}

ExecResult requestError(
    const ExecRequest& request,
    std::string message,
    std::chrono::steady_clock::time_point started) {
  ExecResult result;
  result.target = request.target;
  result.status = ExecStatus::failed;
  result.error = std::move(message);
  setDuration(result, started);
  return result;
}

void writeChildError(int fd, ChildErrorStage stage, int error_code) noexcept {
  const ChildError child_error{static_cast<int>(stage), error_code};
  const char* bytes = reinterpret_cast<const char*>(&child_error);
  std::size_t written = 0;

  while (written < sizeof(child_error)) {
    const ssize_t count = write(fd, bytes + written, sizeof(child_error) - written);
    if (count > 0) {
      written += static_cast<std::size_t>(count);
      continue;
    }
    if (count == -1 && errno == EINTR) {
      continue;
    }
    break;
  }
}

// Usa execve diretamente e resolve PATH manualmente para evitar o fallback de shell do execvp.
[[noreturn]] void executeDirectly(
    const std::string& program,
    char* const argv[],
    int error_fd) {
  if (program.find('/') != std::string::npos) {
    execve(program.c_str(), argv, environ);
    writeChildError(error_fd, ChildErrorStage::execute, errno);
    _exit(kChildFailureExitCode);
  }

  const char* path = getenv("PATH");
  if (path == nullptr) {
    path = "/bin:/usr/bin";
  }

  int last_error = ENOENT;
  std::string_view remaining(path);
  while (true) {
    const std::size_t separator = remaining.find(':');
    const std::string_view directory = remaining.substr(0, separator);
    std::string candidate(directory.empty() ? "." : std::string(directory));
    candidate.push_back('/');
    candidate += program;

    execve(candidate.c_str(), argv, environ);
    const int current_error = errno;
    if (current_error == EACCES) {
      last_error = current_error;
    } else if (current_error != ENOENT && current_error != ENOTDIR) {
      last_error = current_error;
    }

    if (separator == std::string_view::npos) {
      break;
    }
    remaining.remove_prefix(separator + 1);
  }

  writeChildError(error_fd, ChildErrorStage::execute, last_error);
  _exit(kChildFailureExitCode);
}

// Configura o filho e encerra com 127 quando cwd, redirecionamento ou execve falham.
void prepareChild(const ExecRequest& request, const Pipes& pipes, char* const argv[]) {
  close(pipes.stdout_read);
  close(pipes.stderr_read);
  close(pipes.error_read);

  if (request.cwd.has_value() && chdir(request.cwd->c_str()) == -1) {
    writeChildError(pipes.error_write, ChildErrorStage::change_directory, errno);
    _exit(kChildFailureExitCode);
  }

  const int null_fd = open("/dev/null", O_RDONLY);
  if (null_fd == -1 || dup2(null_fd, STDIN_FILENO) == -1) {
    const int error_code = errno;
    if (null_fd != -1) {
      close(null_fd);
    }
    writeChildError(pipes.error_write, ChildErrorStage::redirect_stdin, error_code);
    _exit(kChildFailureExitCode);
  }
  if (null_fd != STDIN_FILENO) {
    close(null_fd);
  }

  if (dup2(pipes.stdout_write, STDOUT_FILENO) == -1) {
    writeChildError(pipes.error_write, ChildErrorStage::redirect_stdout, errno);
    _exit(kChildFailureExitCode);
  }
  if (dup2(pipes.stderr_write, STDERR_FILENO) == -1) {
    writeChildError(pipes.error_write, ChildErrorStage::redirect_stderr, errno);
    _exit(kChildFailureExitCode);
  }

  if (pipes.stdout_write != STDOUT_FILENO) {
    close(pipes.stdout_write);
  }
  if (pipes.stderr_write != STDERR_FILENO) {
    close(pipes.stderr_write);
  }
  executeDirectly(request.program, argv, pipes.error_write);
}

// Le cada pipe ate EAGAIN para impedir que stdout ou stderr bloqueiem o processo.
bool drainOutput(int& fd, std::string& output, std::string& capture_error) {
  if (fd == -1) {
    return true;
  }

  std::array<char, 4096> buffer{};
  while (true) {
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

    capture_error = "failed to read process output: " + errnoMessage(errno);
    closeFd(fd);
    return false;
  }
}

bool drainChildError(
    int& fd,
    std::array<char, sizeof(ChildError)>& bytes,
    std::size_t& bytes_read,
    std::optional<ChildError>& child_error,
    std::string& capture_error) {
  while (fd != -1 && bytes_read < bytes.size()) {
    const ssize_t count = read(fd, bytes.data() + bytes_read, bytes.size() - bytes_read);
    if (count > 0) {
      bytes_read += static_cast<std::size_t>(count);
      if (bytes_read == bytes.size()) {
        ChildError parsed_error{};
        std::memcpy(&parsed_error, bytes.data(), sizeof(parsed_error));
        child_error = parsed_error;
        closeFd(fd);
      }
      continue;
    }
    if (count == 0) {
      closeFd(fd);
      if (bytes_read != 0 && !child_error.has_value()) {
        capture_error = "failed to read process error status";
      }
      return true;
    }
    if (errno == EINTR) {
      continue;
    }
    if (errno == EAGAIN || errno == EWOULDBLOCK) {
      return true;
    }

    capture_error = "failed to read process error status: " + errnoMessage(errno);
    closeFd(fd);
    return false;
  }
  return true;
}

int exitCode(int wait_status) {
  if (WIFEXITED(wait_status)) {
    return WEXITSTATUS(wait_status);
  }
  if (WIFSIGNALED(wait_status)) {
    return 128 + WTERMSIG(wait_status);
  }
  return -1;
}

std::string childErrorMessage(
    const ExecRequest& request,
    const ChildError& child_error) {
  const std::string detail = errnoMessage(child_error.error_code);
  switch (static_cast<ChildErrorStage>(child_error.stage)) {
    case ChildErrorStage::change_directory:
      return "failed to change directory to '" + request.cwd.value_or("") + "': " + detail;
    case ChildErrorStage::redirect_stdin:
      return "failed to redirect process stdin: " + detail;
    case ChildErrorStage::redirect_stdout:
      return "failed to redirect process stdout: " + detail;
    case ChildErrorStage::redirect_stderr:
      return "failed to redirect process stderr: " + detail;
    case ChildErrorStage::execute:
      return "failed to execute '" + request.program + "': " + detail;
  }
  return "process failed before execution: " + detail;
}

int remainingPollTimeout(
    std::chrono::steady_clock::time_point deadline,
    std::chrono::steady_clock::time_point now) {
  if (now >= deadline) {
    return 0;
  }

  const auto remaining = std::chrono::duration_cast<std::chrono::milliseconds>(deadline - now);
  constexpr auto kMaximumPollInterval = std::chrono::milliseconds(20);
  const auto interval = remaining < kMaximumPollInterval ? remaining : kMaximumPollInterval;
  return static_cast<int>(std::max<std::int64_t>(1, interval.count()));
}

}  // namespace

ExecResult exec(const ExecRequest& request) {
  const auto started = std::chrono::steady_clock::now();
  ExecResult result;
  result.target = request.target;

  if (request.target != kLocalTarget) {
    return requestError(request, "only the local target is supported", started);
  }
  if (request.program.empty()) {
    return requestError(request, "program cannot be empty", started);
  }
  if (containsNull(request.program) ||
      (request.cwd.has_value() && containsNull(request.cwd.value()))) {
    return requestError(request, "program and cwd cannot contain NUL bytes", started);
  }
  for (const std::string& argument : request.args) {
    if (containsNull(argument)) {
      return requestError(request, "arguments cannot contain NUL bytes", started);
    }
  }
  if (request.timeout.has_value() && request.timeout->count() < 0) {
    return requestError(request, "timeout must be omitted or non-negative", started);
  }

  // Mantem as strings vivas enquanto argv e usado pelo processo filho.
  std::vector<std::string> argv_storage;
  argv_storage.reserve(request.args.size() + 1);
  argv_storage.push_back(request.program);
  for (const std::string& argument : request.args) {
    argv_storage.push_back(argument);
  }

  std::vector<char*> argv;
  argv.reserve(argv_storage.size() + 1);
  for (std::string& argument : argv_storage) {
    argv.push_back(argument.data());
  }
  argv.push_back(nullptr);

  Pipes pipes;
  int stdout_pipe[2] = {-1, -1};
  int stderr_pipe[2] = {-1, -1};
  int error_pipe[2] = {-1, -1};
  if (!createPipe(stdout_pipe)) {
    return requestError(request, "failed to create stdout pipe: " + errnoMessage(errno), started);
  }
  if (!createPipe(stderr_pipe)) {
    close(stdout_pipe[0]);
    close(stdout_pipe[1]);
    return requestError(request, "failed to create stderr pipe: " + errnoMessage(errno), started);
  }
  if (!createPipe(error_pipe)) {
    close(stdout_pipe[0]);
    close(stdout_pipe[1]);
    close(stderr_pipe[0]);
    close(stderr_pipe[1]);
    return requestError(request, "failed to create process error pipe: " + errnoMessage(errno), started);
  }

  pipes.stdout_read = stdout_pipe[0];
  pipes.stdout_write = stdout_pipe[1];
  pipes.stderr_read = stderr_pipe[0];
  pipes.stderr_write = stderr_pipe[1];
  pipes.error_read = error_pipe[0];
  pipes.error_write = error_pipe[1];

  if (!setCloseOnExec(pipes.error_write) || !setNonBlocking(pipes.stdout_read) ||
      !setNonBlocking(pipes.stderr_read) || !setNonBlocking(pipes.error_read)) {
    const std::string message = "failed to configure process pipes: " + errnoMessage(errno);
    closeAllPipes(pipes);
    return requestError(request, message, started);
  }

  const pid_t child_pid = fork();
  if (child_pid == -1) {
    const std::string message = "failed to fork process: " + errnoMessage(errno);
    closeAllPipes(pipes);
    return requestError(request, message, started);
  }
  if (child_pid == 0) {
    prepareChild(request, pipes, argv.data());
  }

  // O pai fecha as pontas de escrita e acompanha saidas e encerramento em paralelo.
  closeFd(pipes.stdout_write);
  closeFd(pipes.stderr_write);
  closeFd(pipes.error_write);

  const auto deadline = request.timeout.has_value()
      ? std::optional<std::chrono::steady_clock::time_point>{started + request.timeout.value()}
      : std::optional<std::chrono::steady_clock::time_point>{};
  bool child_reaped = false;
  bool timed_out = false;
  int wait_status = 0;
  std::string capture_error;
  std::array<char, sizeof(ChildError)> child_error_bytes{};
  std::size_t child_error_bytes_read = 0;
  std::optional<ChildError> child_error;

  // O pai alterna leitura dos pipes, waitpid e verificacao do timeout.
  while (!child_reaped) {
    drainOutput(pipes.stdout_read, result.stdout, capture_error);
    drainOutput(pipes.stderr_read, result.stderr, capture_error);
    drainChildError(
        pipes.error_read,
        child_error_bytes,
        child_error_bytes_read,
        child_error,
        capture_error);

    pid_t wait_result = waitpid(child_pid, &wait_status, WNOHANG);
    if (wait_result == child_pid) {
      child_reaped = true;
    } else if (wait_result == -1 && errno != EINTR) {
      capture_error = "failed to wait for process: " + errnoMessage(errno);
      kill(child_pid, SIGKILL);
      while (waitpid(child_pid, &wait_status, 0) == -1 && errno == EINTR) {
      }
      child_reaped = true;
    }

    if (!child_reaped && deadline.has_value() &&
        std::chrono::steady_clock::now() >= deadline.value()) {
      if (kill(child_pid, SIGKILL) == -1 && errno != ESRCH) {
        capture_error = "failed to terminate timed out process: " + errnoMessage(errno);
      }
      timed_out = true;
      while (waitpid(child_pid, &wait_status, 0) == -1 && errno == EINTR) {
      }
      child_reaped = true;
    }

    if (!child_reaped) {
      pollfd poll_fds[3]{};
      nfds_t poll_count = 0;
      if (pipes.stdout_read != -1) {
        poll_fds[poll_count++] = {pipes.stdout_read, POLLIN | POLLHUP, 0};
      }
      if (pipes.stderr_read != -1) {
        poll_fds[poll_count++] = {pipes.stderr_read, POLLIN | POLLHUP, 0};
      }
      if (pipes.error_read != -1) {
        poll_fds[poll_count++] = {pipes.error_read, POLLIN | POLLHUP, 0};
      }

      const int timeout = deadline.has_value()
          ? remainingPollTimeout(deadline.value(), std::chrono::steady_clock::now())
          : 20;
      if (poll_count == 0) {
        poll(nullptr, 0, timeout);
      } else {
        poll(poll_fds, poll_count, timeout);
      }
    }
  }

  // Drena dados já escritos sem esperar por descendentes que herdaram os pipes.
  drainOutput(pipes.stdout_read, result.stdout, capture_error);
  drainOutput(pipes.stderr_read, result.stderr, capture_error);
  drainChildError(
      pipes.error_read,
      child_error_bytes,
      child_error_bytes_read,
      child_error,
      capture_error);
  closeAllPipes(pipes);

  // O erro recebido pelo pipe tem prioridade sobre o codigo 127 do filho.
  result.exit_code = exitCode(wait_status);
  if (child_error.has_value()) {
    result.status = ExecStatus::failed;
    result.error = childErrorMessage(request, child_error.value());
  } else if (timed_out) {
    result.status = ExecStatus::timed_out;
    result.error = "process timed out";
  } else if (!capture_error.empty()) {
    result.status = ExecStatus::failed;
    result.error = capture_error;
  } else if (result.exit_code == 0) {
    result.status = ExecStatus::success;
  } else if (WIFSIGNALED(wait_status)) {
    result.status = ExecStatus::failed;
    result.error = "process terminated by signal " + std::to_string(WTERMSIG(wait_status));
  } else {
    result.status = ExecStatus::failed;
    result.error = "process exited with code " + std::to_string(result.exit_code);
  }

  setDuration(result, started);
  return result;
}

const char* statusName(ExecStatus status) noexcept {
  switch (status) {
    case ExecStatus::success:
      return "success";
    case ExecStatus::failed:
      return "failed";
    case ExecStatus::timed_out:
      return "timed_out";
  }
  return "failed";
}

}  // namespace atlas::capabilities::tools::process
