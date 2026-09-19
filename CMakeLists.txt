cmake_minimum_required(VERSION 3.20)

project(atlas-capabilities LANGUAGES CXX)

set(CMAKE_CXX_STANDARD 23)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
set(CMAKE_CXX_EXTENSIONS OFF)

add_library(atlas-capabilities-executable-runtime STATIC
  src/capabilities/runtime/executable/adapter.cpp
  src/capabilities/runtime/executable/protocol.cpp
)
target_include_directories(atlas-capabilities-executable-runtime PUBLIC src/capabilities)

add_library(atlas-capabilities-core STATIC
  src/capabilities/core/arguments.cpp
  src/capabilities/core/command_runner.cpp
  src/capabilities/core/discovery.cpp
  src/capabilities/core/executor.cpp
  src/capabilities/core/loader.cpp
  src/capabilities/core/registry.cpp
  src/capabilities/core/schema.cpp
  src/capabilities/core/spawn.cpp
)
target_include_directories(atlas-capabilities-core PUBLIC src/capabilities/core)
target_link_libraries(atlas-capabilities-core PUBLIC atlas-capabilities-executable-runtime)

add_executable(atlas-shell-exec-runtime
  src/capabilities/runtime/executable/main.cpp
  src/capabilities/tools/shell/exec/shell.cpp
)
target_link_libraries(atlas-shell-exec-runtime PRIVATE atlas-capabilities-core)
set_target_properties(atlas-shell-exec-runtime PROPERTIES
  OUTPUT_NAME runtime
  RUNTIME_OUTPUT_DIRECTORY "${CMAKE_CURRENT_SOURCE_DIR}/src/capabilities/tools/shell/exec"
)

add_executable(atlas-git-status-runtime
  src/capabilities/runtime/executable/main.cpp
  src/capabilities/tools/git/status/status.cpp
  src/capabilities/tools/git/status/shim.cpp
)
target_link_libraries(atlas-git-status-runtime PRIVATE atlas-capabilities-core)
set_target_properties(atlas-git-status-runtime PROPERTIES
  OUTPUT_NAME runtime
  RUNTIME_OUTPUT_DIRECTORY "${CMAKE_CURRENT_SOURCE_DIR}/src/capabilities/tools/git/status"
)

add_executable(atlas-git-diff-runtime
  src/capabilities/runtime/executable/main.cpp
  src/capabilities/tools/git/diff/diff.cpp
  src/capabilities/tools/git/diff/shim.cpp
)
target_link_libraries(atlas-git-diff-runtime PRIVATE atlas-capabilities-core)
set_target_properties(atlas-git-diff-runtime PROPERTIES
  OUTPUT_NAME runtime
  RUNTIME_OUTPUT_DIRECTORY "${CMAKE_CURRENT_SOURCE_DIR}/src/capabilities/tools/git/diff"
)

# Núcleo Rust da tool lsp.diagnostics: compila via cargo e entrega o
# binário no mesmo layout dos runtimes C++ (entrypoint "runtime").
find_program(ATLAS_LSP_CARGO cargo)
if(ATLAS_LSP_CARGO)
  set(ATLAS_LSP_DIR "${CMAKE_CURRENT_SOURCE_DIR}/src/capabilities/tools/lsp")
  set(ATLAS_LSP_BINARY "${ATLAS_LSP_DIR}/diagnostics/runtime")
  file(GLOB ATLAS_LSP_SOURCES "${ATLAS_LSP_DIR}/src/*.rs" "${ATLAS_LSP_DIR}/Cargo.toml" "${ATLAS_LSP_DIR}/Cargo.lock")
  add_custom_command(
    OUTPUT "${ATLAS_LSP_BINARY}"
    COMMAND "${ATLAS_LSP_CARGO}" build --locked --release --manifest-path "${ATLAS_LSP_DIR}/Cargo.toml"
    COMMAND "${CMAKE_COMMAND}" -E rm -f "${ATLAS_LSP_BINARY}"
    COMMAND "${CMAKE_COMMAND}" -E copy "${ATLAS_LSP_DIR}/target/release/atlas-lsp-runtime" "${ATLAS_LSP_BINARY}"
    DEPENDS ${ATLAS_LSP_SOURCES}
    COMMENT "Building atlas-lsp-runtime (Rust)"
  )
  add_custom_target(atlas-lsp-runtime ALL DEPENDS "${ATLAS_LSP_BINARY}")
  install(PROGRAMS "${ATLAS_LSP_BINARY}" DESTINATION capabilities/tools/lsp/diagnostics)
  add_test(NAME atlas-lsp COMMAND "${ATLAS_LSP_CARGO}" test --locked --manifest-path "${ATLAS_LSP_DIR}/Cargo.toml")
  set_tests_properties(atlas-lsp PROPERTIES WORKING_DIRECTORY "${CMAKE_CURRENT_SOURCE_DIR}")
endif()

# Núcleo Rust da tool web.fetch: mesmo layout do lsp (entrypoint "runtime").
find_program(ATLAS_WEB_FETCH_CARGO cargo)
if(ATLAS_WEB_FETCH_CARGO)
  set(ATLAS_WEB_FETCH_DIR "${CMAKE_CURRENT_SOURCE_DIR}/src/capabilities/tools/web/fetch")
  set(ATLAS_WEB_FETCH_BINARY "${ATLAS_WEB_FETCH_DIR}/runtime")
  file(GLOB ATLAS_WEB_FETCH_SOURCES "${ATLAS_WEB_FETCH_DIR}/src/*.rs" "${ATLAS_WEB_FETCH_DIR}/Cargo.toml" "${ATLAS_WEB_FETCH_DIR}/Cargo.lock")
  add_custom_command(
    OUTPUT "${ATLAS_WEB_FETCH_BINARY}"
    COMMAND "${ATLAS_WEB_FETCH_CARGO}" build --locked --release --manifest-path "${ATLAS_WEB_FETCH_DIR}/Cargo.toml"
    COMMAND "${CMAKE_COMMAND}" -E rm -f "${ATLAS_WEB_FETCH_BINARY}"
    COMMAND "${CMAKE_COMMAND}" -E copy "${ATLAS_WEB_FETCH_DIR}/target/release/atlas-web-fetch-runtime" "${ATLAS_WEB_FETCH_BINARY}"
    DEPENDS ${ATLAS_WEB_FETCH_SOURCES}
    COMMENT "Building atlas-web-fetch-runtime (Rust)"
  )
  add_custom_target(atlas-web-fetch-runtime ALL DEPENDS "${ATLAS_WEB_FETCH_BINARY}")
  install(PROGRAMS "${ATLAS_WEB_FETCH_BINARY}" DESTINATION capabilities/tools/web/fetch)
  add_test(NAME atlas-web-fetch COMMAND "${ATLAS_WEB_FETCH_CARGO}" test --locked --manifest-path "${ATLAS_WEB_FETCH_DIR}/Cargo.toml")
  set_tests_properties(atlas-web-fetch PROPERTIES WORKING_DIRECTORY "${CMAKE_CURRENT_SOURCE_DIR}")
endif()

# Entrypoint TypeScript da tool web.search: compila para JS com shebang.
find_program(ATLAS_WEB_SEARCH_NPM npm)
if(ATLAS_WEB_SEARCH_NPM)
  set(ATLAS_WEB_SEARCH_DIR "${CMAKE_CURRENT_SOURCE_DIR}/src/capabilities/tools/web/search")
  set(ATLAS_WEB_SEARCH_BINARY "${ATLAS_WEB_SEARCH_DIR}/runtime")
  add_custom_command(
    OUTPUT "${ATLAS_WEB_SEARCH_BINARY}"
    COMMAND "${ATLAS_WEB_SEARCH_NPM}" run build:web-search
    DEPENDS "${ATLAS_WEB_SEARCH_DIR}/search.ts"
    WORKING_DIRECTORY "${CMAKE_CURRENT_SOURCE_DIR}"
    COMMENT "Building web-search runtime (TypeScript)"
  )
  add_custom_target(atlas-web-search-runtime ALL DEPENDS "${ATLAS_WEB_SEARCH_BINARY}")
  install(PROGRAMS "${ATLAS_WEB_SEARCH_BINARY}" DESTINATION capabilities/tools/web/search)
endif()

add_executable(atlas-system-info-runtime
  src/capabilities/runtime/executable/main.cpp
  src/capabilities/tools/system/info/info.cpp
)
target_link_libraries(atlas-system-info-runtime PRIVATE atlas-capabilities-core)
set_target_properties(atlas-system-info-runtime PROPERTIES
  OUTPUT_NAME runtime
  RUNTIME_OUTPUT_DIRECTORY "${CMAKE_CURRENT_SOURCE_DIR}/src/capabilities/tools/system/info"
)

add_library(atlas-filesystem-search-ignore STATIC
  src/capabilities/tools/filesystem/search/ignore.cpp
)
target_include_directories(atlas-filesystem-search-ignore PUBLIC
  src/capabilities/tools/filesystem/search
)

function(atlas_filesystem_runtime id directory source)
  add_executable(atlas-filesystem-${id}-runtime
    src/capabilities/runtime/executable/main.cpp
    src/capabilities/tools/filesystem/filesystem.cpp
    src/capabilities/tools/filesystem/${directory}/${source}
  )
  target_link_libraries(atlas-filesystem-${id}-runtime PRIVATE
    atlas-capabilities-core
    atlas-filesystem-search-ignore
  )
  set_target_properties(atlas-filesystem-${id}-runtime PROPERTIES
    OUTPUT_NAME runtime
    RUNTIME_OUTPUT_DIRECTORY "${CMAKE_CURRENT_SOURCE_DIR}/src/capabilities/tools/filesystem/${directory}"
  )
endfunction()

atlas_filesystem_runtime(read read read.cpp)
atlas_filesystem_runtime(list list list.cpp)
atlas_filesystem_runtime(search search search.cpp)
atlas_filesystem_runtime(glob glob glob.cpp)
atlas_filesystem_runtime(patch patch patch.cpp)

add_executable(atlas-capability-bridge-runtime
  src/capabilities/runtime/bridge/main.cpp
)
target_link_libraries(atlas-capability-bridge-runtime PRIVATE atlas-capabilities-core)
set_target_properties(atlas-capability-bridge-runtime PROPERTIES
  OUTPUT_NAME runtime
  RUNTIME_OUTPUT_DIRECTORY "${CMAKE_CURRENT_SOURCE_DIR}/src/capabilities/runtime/bridge"
)

enable_testing()

add_executable(atlas-shell-exec-test tests/native/shell_exec_test.cpp)
target_sources(atlas-shell-exec-test PRIVATE src/capabilities/tools/shell/exec/shell.cpp)
target_link_libraries(atlas-shell-exec-test PRIVATE atlas-capabilities-core)
add_dependencies(atlas-shell-exec-test atlas-shell-exec-runtime)

add_executable(atlas-system-info-test tests/native/system_info_test.cpp)
target_sources(atlas-system-info-test PRIVATE src/capabilities/tools/system/info/info.cpp)
target_link_libraries(atlas-system-info-test PRIVATE atlas-capabilities-core)
add_dependencies(atlas-system-info-test atlas-system-info-runtime)

add_executable(atlas-capability-loader-test tests/native/capability_loader_test.cpp)
target_link_libraries(atlas-capability-loader-test PRIVATE atlas-capabilities-core)

add_executable(atlas-filesystem-test tests/native/filesystem_test.cpp)
target_sources(atlas-filesystem-test PRIVATE src/capabilities/tools/filesystem/filesystem.cpp)
target_link_libraries(atlas-filesystem-test PRIVATE
  atlas-capabilities-core
  atlas-filesystem-search-ignore
)
add_dependencies(atlas-filesystem-test
  atlas-filesystem-read-runtime
  atlas-filesystem-list-runtime
  atlas-filesystem-search-runtime
  atlas-filesystem-glob-runtime
  atlas-filesystem-patch-runtime
)

add_executable(atlas-git-test tests/native/git_test.cpp)
target_sources(atlas-git-test PRIVATE
  src/capabilities/tools/git/status/status.cpp
  src/capabilities/tools/git/diff/diff.cpp
)
target_link_libraries(atlas-git-test PRIVATE atlas-capabilities-core)
add_dependencies(atlas-git-test atlas-git-status-runtime atlas-git-diff-runtime)

add_executable(atlas-capability-executor-test tests/native/capability_executor_test.cpp)
target_link_libraries(atlas-capability-executor-test PRIVATE atlas-capabilities-core)
add_dependencies(atlas-capability-executor-test atlas-shell-exec-runtime)

add_executable(atlas-command-runner-test tests/native/command_runner_test.cpp)
target_link_libraries(atlas-command-runner-test PRIVATE atlas-capabilities-core)

foreach(target
    atlas-capabilities-executable-runtime
    atlas-capabilities-core
    atlas-filesystem-search-ignore
    atlas-shell-exec-runtime
    atlas-git-status-runtime
    atlas-git-diff-runtime
    atlas-system-info-runtime
    atlas-capability-bridge-runtime
    atlas-filesystem-read-runtime
    atlas-filesystem-list-runtime
    atlas-filesystem-search-runtime
    atlas-filesystem-glob-runtime
    atlas-filesystem-patch-runtime
    atlas-shell-exec-test
    atlas-system-info-test
    atlas-filesystem-test
    atlas-git-test
    atlas-capability-loader-test
    atlas-capability-executor-test
    atlas-command-runner-test)
  target_compile_options(${target} PRIVATE -Wall -Wextra -Werror -pedantic)
endforeach()

add_test(NAME atlas-shell-exec COMMAND atlas-shell-exec-test)
add_test(NAME atlas-system-info COMMAND atlas-system-info-test)
add_test(NAME atlas-filesystem COMMAND atlas-filesystem-test)
add_test(NAME atlas-git COMMAND atlas-git-test)
add_test(NAME atlas-capability-loader COMMAND atlas-capability-loader-test)
add_test(NAME atlas-capability-executor COMMAND atlas-capability-executor-test)
add_test(NAME atlas-command-runner COMMAND atlas-command-runner-test)
set_tests_properties(
  atlas-shell-exec
  atlas-system-info
  atlas-filesystem
  atlas-git
  atlas-capability-loader
  atlas-capability-executor
  atlas-command-runner
  PROPERTIES WORKING_DIRECTORY "${CMAKE_CURRENT_SOURCE_DIR}"
)

install(TARGETS atlas-shell-exec-runtime
  RUNTIME DESTINATION capabilities/tools/shell/exec
)
install(TARGETS atlas-git-status-runtime
  RUNTIME DESTINATION capabilities/tools/git/status
)
install(TARGETS atlas-git-diff-runtime
  RUNTIME DESTINATION capabilities/tools/git/diff
)
install(TARGETS atlas-system-info-runtime
  RUNTIME DESTINATION capabilities/tools/system/info
)
install(TARGETS atlas-capability-bridge-runtime
  RUNTIME DESTINATION capabilities/runtime/bridge
)
install(TARGETS atlas-filesystem-read-runtime
  RUNTIME DESTINATION capabilities/tools/filesystem/read
)
install(TARGETS atlas-filesystem-list-runtime
  RUNTIME DESTINATION capabilities/tools/filesystem/list
)
install(TARGETS atlas-filesystem-search-runtime
  RUNTIME DESTINATION capabilities/tools/filesystem/search
)
install(TARGETS atlas-filesystem-glob-runtime
  RUNTIME DESTINATION capabilities/tools/filesystem/glob
)
install(TARGETS atlas-filesystem-patch-runtime
  RUNTIME DESTINATION capabilities/tools/filesystem/patch
)
install(FILES src/capabilities/tools/filesystem/read/capability.json
  DESTINATION capabilities/tools/filesystem/read
)
install(FILES src/capabilities/tools/filesystem/list/capability.json
  DESTINATION capabilities/tools/filesystem/list
)
install(FILES src/capabilities/tools/filesystem/search/capability.json
  DESTINATION capabilities/tools/filesystem/search
)
install(FILES src/capabilities/tools/filesystem/glob/capability.json
  DESTINATION capabilities/tools/filesystem/glob
)
install(FILES src/capabilities/tools/filesystem/patch/capability.json
  DESTINATION capabilities/tools/filesystem/patch
)
install(FILES src/capabilities/tools/filesystem/group.json
  DESTINATION capabilities/tools/filesystem
)
install(FILES src/capabilities/tools/shell/exec/capability.json
  DESTINATION capabilities/tools/shell/exec
)
install(FILES src/capabilities/tools/shell/group.json
  DESTINATION capabilities/tools/shell
)
install(FILES src/capabilities/tools/lsp/diagnostics/capability.json
  DESTINATION capabilities/tools/lsp/diagnostics
)
install(FILES src/capabilities/tools/lsp/group.json
  DESTINATION capabilities/tools/lsp
)
install(FILES src/capabilities/tools/web/search/capability.json
  DESTINATION capabilities/tools/web/search
)
install(FILES src/capabilities/tools/web/fetch/capability.json
  DESTINATION capabilities/tools/web/fetch
)
install(FILES src/capabilities/tools/web/group.json
  DESTINATION capabilities/tools/web
)
install(FILES src/capabilities/tools/process/group.json
  DESTINATION capabilities/tools/process
)
install(FILES src/capabilities/tools/system/info/capability.json
  DESTINATION capabilities/tools/system/info
)
install(FILES src/capabilities/tools/system/group.json
  DESTINATION capabilities/tools/system
)
