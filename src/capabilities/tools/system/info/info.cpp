#include "info.hpp"

#include "../../../runtime/executable/adapter.hpp"
#include <array>
#include <cstdio>
#include <cstdlib>
#include <optional>
#include <pwd.h>
#include <string_view>
#include <sys/utsname.h>
#include <unistd.h>
#include <utility>

namespace atlas::capabilities::tools::system {
namespace {

std::string toLowerCase(std::string_view value) {
  std::string lowered;
  lowered.reserve(value.size());
  for (const char character : value) {
    lowered.push_back(
        character >= 'A' && character <= 'Z'
            ? static_cast<char>(character - 'A' + 'a')
            : character);
  }
  return lowered;
}

std::optional<std::string> readFileField(const char* path, std::string_view key) {
  if (FILE* file = std::fopen(path, "rb")) {
    std::string contents;
    char buffer[4096];
    std::size_t read = 0;
    while ((read = std::fread(buffer, 1, sizeof(buffer), file)) > 0) {
      contents.append(buffer, read);
    }
    std::fclose(file);

    std::string_view remaining(contents);
    while (!remaining.empty()) {
      const std::size_t newline = remaining.find('\n');
      const std::string_view line = remaining.substr(0, newline);
      if (line.starts_with(key) && line.size() > key.size() && line[key.size()] == '=') {
        std::string value{line.substr(key.size() + 1)};
        if (value.size() >= 2 && value.front() == '"' && value.back() == '"') {
          value = value.substr(1, value.size() - 2);
        }
        return value;
      }
      if (newline == std::string_view::npos) {
        break;
      }
      remaining.remove_prefix(newline + 1);
    }
  }
  return std::nullopt;
}

// Resolve o fuso horario sem executar comandos: TZ, link de /etc/localtime ou UTC.
std::string timezone() {
  if (const char* environment = std::getenv("TZ");
      environment != nullptr && environment[0] != '\0') {
    return environment;
  }

  static constexpr char kLocaltime[] = "/etc/localtime";
  std::array<char, 512> buffer{};
  const ssize_t length = ::readlink(kLocaltime, buffer.data(), buffer.size() - 1);
  if (length > 0) {
    const std::string_view target(buffer.data(), static_cast<std::size_t>(length));
    constexpr std::string_view kZoneInfo = "zoneinfo/";
    if (const std::size_t position = target.rfind(kZoneInfo);
        position != std::string_view::npos) {
      return std::string(target.substr(position + kZoneInfo.size()));
    }
    if (const std::size_t position = target.rfind('/');
        position != std::string_view::npos) {
      return std::string(target.substr(position + 1));
    }
  }
  return "UTC";
}

}  // namespace

SystemInfo systemInfo() {
  utsname names{};
  // O kernel sempre fornece os campos base; falha aqui e tratada como campo vazio.
  const bool kernel_available = ::uname(&names) == 0;

  SystemInfo info;
  const char* kernelName = kernel_available ? names.sysname : "";
  info.platform = toLowerCase(kernelName);
  info.os_name = readFileField("/etc/os-release", "NAME").value_or(kernelName);
  info.os_version =
      readFileField("/etc/os-release", "VERSION_ID")
          .value_or(kernel_available ? names.release : "");
  info.kernel_version = kernel_available ? names.release : "";
  info.architecture = kernel_available ? names.machine : "";
  info.hostname = kernel_available ? names.nodename : "";

  passwd* user = ::getpwuid(::geteuid());
  if (user != nullptr) {
    info.username = user->pw_name != nullptr ? user->pw_name : "";
    info.shell = user->pw_shell != nullptr ? user->pw_shell : "";
  }
  info.timezone = timezone();
  return info;
}

}  // namespace atlas::capabilities::tools::system

namespace atlas::capabilities::tools::system {

atlas::capabilities::ExecutionResult dispatch(
    const atlas::capabilities::NativeRequest& request,
    const atlas::capabilities::ExecutionOutputCallback&) {
  const SystemInfo info = systemInfo();

  atlas::capabilities::ExecutionResult result;
  result.target = request.target;
  result.status = atlas::capabilities::ExecutionStatus::success;
  result.output = StructuredValue::Object{
      {"platform", info.platform},
      {"os_name", info.os_name},
      {"os_version", info.os_version},
      {"kernel_version", info.kernel_version},
      {"architecture", info.architecture},
      {"hostname", info.hostname},
      {"username", info.username},
      {"shell", info.shell},
      {"timezone", info.timezone},
  };
  return result;
}

}  // namespace atlas::capabilities::tools::system

namespace atlas::capabilities::runtime::executable {

Dispatch dispatch() {
  return &tools::system::dispatch;
}

}  // namespace atlas::capabilities::runtime::executable
