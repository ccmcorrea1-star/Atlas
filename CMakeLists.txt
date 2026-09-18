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
  src/capabilities/core/discovery.cpp
  src/capabilities/core/executor.cpp
  src/capabilities/core/loader.cpp
  src/capabilities/core/registry.cpp
)
target_include_directories(atlas-capabilities-core PUBLIC src/capabilities/core)
target_link_libraries(atlas-capabilities-core PUBLIC atlas-capabilities-executable-runtime)

add_executable(atlas-process-exec-runtime
  src/capabilities/runtime/executable/main.cpp
  src/capabilities/tools/process/exec/exec.cpp
)
target_link_libraries(atlas-process-exec-runtime PRIVATE atlas-capabilities-core)
set_target_properties(atlas-process-exec-runtime PROPERTIES
  OUTPUT_NAME runtime
  RUNTIME_OUTPUT_DIRECTORY "${CMAKE_CURRENT_SOURCE_DIR}/src/capabilities/tools/process/exec"
)

add_executable(atlas-system-info-runtime
  src/capabilities/runtime/executable/main.cpp
  src/capabilities/tools/system/info/info.cpp
)
target_link_libraries(atlas-system-info-runtime PRIVATE atlas-capabilities-core)
set_target_properties(atlas-system-info-runtime PROPERTIES
  OUTPUT_NAME runtime
  RUNTIME_OUTPUT_DIRECTORY "${CMAKE_CURRENT_SOURCE_DIR}/src/capabilities/tools/system/info"
)

function(atlas_filesystem_runtime id directory source)
  add_executable(atlas-filesystem-${id}-runtime
    src/capabilities/runtime/executable/main.cpp
    src/capabilities/tools/filesystem/filesystem.cpp
    src/capabilities/tools/filesystem/${directory}/${source}
  )
  target_link_libraries(atlas-filesystem-${id}-runtime PRIVATE atlas-capabilities-core)
  set_target_properties(atlas-filesystem-${id}-runtime PROPERTIES
    OUTPUT_NAME runtime
    RUNTIME_OUTPUT_DIRECTORY "${CMAKE_CURRENT_SOURCE_DIR}/src/capabilities/tools/filesystem/${directory}"
  )
endfunction()

atlas_filesystem_runtime(read read read.cpp)
atlas_filesystem_runtime(write write write.cpp)
atlas_filesystem_runtime(edit edit edit.cpp)
atlas_filesystem_runtime(list list list.cpp)
atlas_filesystem_runtime(search search search.cpp)

add_executable(atlas-capability-bridge-runtime
  src/capabilities/runtime/bridge/main.cpp
)
target_link_libraries(atlas-capability-bridge-runtime PRIVATE atlas-capabilities-core)
set_target_properties(atlas-capability-bridge-runtime PROPERTIES
  OUTPUT_NAME runtime
  RUNTIME_OUTPUT_DIRECTORY "${CMAKE_CURRENT_SOURCE_DIR}/src/capabilities/runtime/bridge"
)

enable_testing()

add_executable(atlas-process-exec-test tests/native/process_exec_test.cpp)
target_sources(atlas-process-exec-test PRIVATE src/capabilities/tools/process/exec/exec.cpp)
target_link_libraries(atlas-process-exec-test PRIVATE atlas-capabilities-core)
add_dependencies(atlas-process-exec-test atlas-process-exec-runtime)

add_executable(atlas-system-info-test tests/native/system_info_test.cpp)
target_sources(atlas-system-info-test PRIVATE src/capabilities/tools/system/info/info.cpp)
target_link_libraries(atlas-system-info-test PRIVATE atlas-capabilities-core)
add_dependencies(atlas-system-info-test atlas-system-info-runtime)

add_executable(atlas-capability-loader-test tests/native/capability_loader_test.cpp)
target_link_libraries(atlas-capability-loader-test PRIVATE atlas-capabilities-core)

add_executable(atlas-filesystem-test tests/native/filesystem_test.cpp)
target_sources(atlas-filesystem-test PRIVATE src/capabilities/tools/filesystem/filesystem.cpp)
target_link_libraries(atlas-filesystem-test PRIVATE atlas-capabilities-core)
add_dependencies(atlas-filesystem-test
  atlas-filesystem-read-runtime
  atlas-filesystem-write-runtime
  atlas-filesystem-edit-runtime
  atlas-filesystem-list-runtime
  atlas-filesystem-search-runtime
)

add_executable(atlas-capability-executor-test tests/native/capability_executor_test.cpp)
target_link_libraries(atlas-capability-executor-test PRIVATE atlas-capabilities-core)
add_dependencies(atlas-capability-executor-test atlas-process-exec-runtime)

foreach(target
    atlas-capabilities-executable-runtime
    atlas-capabilities-core
    atlas-process-exec-runtime
    atlas-system-info-runtime
    atlas-capability-bridge-runtime
    atlas-filesystem-read-runtime
    atlas-filesystem-write-runtime
    atlas-filesystem-edit-runtime
    atlas-filesystem-list-runtime
    atlas-filesystem-search-runtime
    atlas-process-exec-test
    atlas-system-info-test
    atlas-filesystem-test
    atlas-capability-loader-test
    atlas-capability-executor-test)
  target_compile_options(${target} PRIVATE -Wall -Wextra -Werror -pedantic)
endforeach()

add_test(NAME atlas-process-exec COMMAND atlas-process-exec-test)
add_test(NAME atlas-system-info COMMAND atlas-system-info-test)
add_test(NAME atlas-filesystem COMMAND atlas-filesystem-test)
add_test(NAME atlas-capability-loader COMMAND atlas-capability-loader-test)
add_test(NAME atlas-capability-executor COMMAND atlas-capability-executor-test)
set_tests_properties(
  atlas-process-exec
  atlas-system-info
  atlas-filesystem
  atlas-capability-loader
  atlas-capability-executor
  PROPERTIES WORKING_DIRECTORY "${CMAKE_CURRENT_SOURCE_DIR}"
)

install(TARGETS atlas-process-exec-runtime
  RUNTIME DESTINATION capabilities/tools/process/exec
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
install(TARGETS atlas-filesystem-write-runtime
  RUNTIME DESTINATION capabilities/tools/filesystem/write
)
install(TARGETS atlas-filesystem-edit-runtime
  RUNTIME DESTINATION capabilities/tools/filesystem/edit
)
install(TARGETS atlas-filesystem-list-runtime
  RUNTIME DESTINATION capabilities/tools/filesystem/list
)
install(TARGETS atlas-filesystem-search-runtime
  RUNTIME DESTINATION capabilities/tools/filesystem/search
)
install(FILES src/capabilities/tools/filesystem/read/capability.json
  DESTINATION capabilities/tools/filesystem/read
)
install(FILES src/capabilities/tools/filesystem/write/capability.json
  DESTINATION capabilities/tools/filesystem/write
)
install(FILES src/capabilities/tools/filesystem/edit/capability.json
  DESTINATION capabilities/tools/filesystem/edit
)
install(FILES src/capabilities/tools/filesystem/list/capability.json
  DESTINATION capabilities/tools/filesystem/list
)
install(FILES src/capabilities/tools/filesystem/search/capability.json
  DESTINATION capabilities/tools/filesystem/search
)
install(FILES src/capabilities/tools/filesystem/group.json
  DESTINATION capabilities/tools/filesystem
)
install(FILES src/capabilities/tools/process/exec/capability.json
  DESTINATION capabilities/tools/process/exec
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
