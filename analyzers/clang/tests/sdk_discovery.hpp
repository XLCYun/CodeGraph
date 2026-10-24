#pragma once

#include <filesystem>

namespace codegraph::clang_analyzer::testing {

// SDK 专项验收链接实际发现代码；测试对象不编入发布入口。
int sdkDiscoveryTests(const std::filesystem::path &root);

} // namespace codegraph::clang_analyzer::testing
