#include "analyzer.hpp"
#include <iostream>
#include <string>

// 固定 CLI 只解释启动参数，一次分析的状态全部由 module 管理。
int main(int argc, char **argv) {
  if((argc==2 && (std::string(argv[1])=="--help" || std::string(argv[1])=="--version")) || (argc==3 && std::string(argv[1])=="analyze" && std::string(argv[2])=="--help")) {
    std::cout<<(std::string(argv[1])=="--version"?"codegraph-c-cpp 0.1.2\n":"Usage: codegraph-c-cpp analyze\nReads one UTF-8 JSON request followed by EOF.\n"); return 0;
  }
  if(argc!=2 || std::string(argv[1])!="analyze") { std::cerr<<"Usage: codegraph-c-cpp analyze\n"; return 2; }
  return codegraph::clang_analyzer::runPublicAnalysis(std::cin, std::cout);
}
