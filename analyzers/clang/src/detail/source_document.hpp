#pragma once

#include <string>
#include <vector>

namespace codegraph::clang_analyzer::detail {

// 原始预处理片段只记录物理范围和显示身份，不求值或展开内容。
struct SourceRegion {
  unsigned begin = 0, end = 0;
  std::string kind, name;
};

// 供原生语法提取使用的等长视图；原文始终由 Graph::files 保存。
struct SourceDocument {
  std::string view;
  std::vector<SourceRegion> regions;
};

// 识别逻辑指令行、完整条件块与标准头文件保护，屏蔽部分不改变任何偏移。
SourceDocument sourceDocument(const std::string &source);

} // namespace codegraph::clang_analyzer::detail
