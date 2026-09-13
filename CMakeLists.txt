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

add_executable(atlas-capability-loader-test tests/native/capability_loader_test.cpp)
target_link_libraries(atlas-capability-loader-test PRIVATE atlas-capabilities-core)

add_executable(atlas-capability-executor-test tests/native/capability_executor_test.cpp)
target_link_libraries(atlas-capability-executor-test PRIVATE atlas-capabilities-core)
add_dependencies(atlas-capability-executor-test atlas-process-exec-runtime)

foreach(target
    atlas-capabilities-executable-runtime
    atlas-capabilities-core
    atlas-process-exec-runtime
    atlas-capability-bridge-runtime
    atlas-process-exec-test
    atlas-capability-loader-test
    atlas-capability-executor-test)
  target_compile_options(${target} PRIVATE -Wall -Wextra -Werror -pedantic)
endforeach()

add_test(NAME atlas-process-exec COMMAND atlas-process-exec-test)
add_test(NAME atlas-capability-loader COMMAND atlas-capability-loader-test)
add_test(NAME atlas-capability-executor COMMAND atlas-capability-executor-test)
set_tests_properties(
  atlas-process-exec
  atlas-capability-loader
  atlas-capability-executor
  PROPERTIES WORKING_DIRECTORY "${CMAKE_CURRENT_SOURCE_DIR}"
)

install(TARGETS atlas-process-exec-runtime
  RUNTIME DESTINATION capabilities/tools/process/exec
)
install(TARGETS atlas-capability-bridge-runtime
  RUNTIME DESTINATION capabilities/runtime/bridge
)
install(FILES src/capabilities/tools/process/exec/capability.json
  DESTINATION capabilities/tools/process/exec
)
install(FILES src/capabilities/tools/process/group.json
  DESTINATION capabilities/tools/process
)
