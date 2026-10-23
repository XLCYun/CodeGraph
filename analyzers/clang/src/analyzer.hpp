#pragma once

#include <iosfwd>

namespace codegraph::clang_analyzer {

// 执行一次公共分析：读取至 EOF、生成最终 JSON，并返回原有成功或失败状态。
int runPublicAnalysis(std::istream &input, std::ostream &output);

namespace testing {

// 非交付夹具使用相同分析实现，保留指定 SDK、细分错误码和内部来源证据。
int runFixtureAnalysis(std::istream &input, std::ostream &output);

} // namespace testing
} // namespace codegraph::clang_analyzer
