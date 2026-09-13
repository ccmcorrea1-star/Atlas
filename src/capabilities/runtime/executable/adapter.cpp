#include "adapter.hpp"

#include "protocol.hpp"

#include <exception>
#include <iostream>
#include <iterator>
#include <string>

namespace atlas::capabilities::runtime::executable {

int run(Dispatch dispatch) {
  // Cada processo atende uma requisicao JSON completa pela entrada padrao.
  const std::string input{
      std::istreambuf_iterator<char>(std::cin),
      std::istreambuf_iterator<char>()};
  const RequestParseResult parsed = parseRequest(input);

  ExecutionResult result;
  if (!parsed.request.has_value()) {
    result.target = parsed.target;
    result.status = ExecutionStatus::failed;
    result.error = parsed.error;
  } else if (dispatch == nullptr) {
    result.target = parsed.target;
    result.status = ExecutionStatus::failed;
    result.error = "executable dispatch is not configured";
  } else {
    try {
      result = dispatch(parsed.request.value());
      result.target = parsed.target;
    } catch (const std::exception& exception) {
      result.target = parsed.target;
      result.status = ExecutionStatus::failed;
      result.error = "executable dispatch threw an exception: ";
      result.error += exception.what();
    } catch (...) {
      result.target = parsed.target;
      result.status = ExecutionStatus::failed;
      result.error = "executable dispatch threw an unknown exception";
    }
  }

  std::cout << serializeJson(responseValue(result)) << '\n';
  return 0;
}

}  // namespace atlas::capabilities::runtime::executable
