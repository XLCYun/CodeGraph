// 非交付测试入口只调用完整操作，不包含生产实现文件。
#include "../src/analyzer.hpp"
#include "sdk_discovery.hpp"
#include <iostream>
#include <string>

// 历史语义夹具和 SDK 专项检查共享真实分析实现。
int main(int argc, char **argv) {
  if (argc == 3 && std::string(argv[1]) == "--sdk-discovery-tests")
    return codegraph::clang_analyzer::testing::sdkDiscoveryTests(std::filesystem::u8path(argv[2]));
  return codegraph::clang_analyzer::testing::runFixtureAnalysis(std::cin, std::cout);
}
