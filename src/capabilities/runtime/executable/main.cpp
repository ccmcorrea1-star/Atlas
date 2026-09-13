#include "adapter.hpp"

int main() {
  return atlas::capabilities::runtime::executable::run(
      &atlas::capabilities::atlas_executable_dispatch);
}
