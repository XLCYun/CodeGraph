#include "detail/analysis.hpp"
namespace codegraph::clang_analyzer::detail {

// 每个请求仅有一个并集状态，全部 TU 完成后才收尾。
j::Object analyze(Request &r) {
  Graph graph(r);
  for (auto &config : r.configs) collectTranslationUnit(graph, config);
  return finish(graph);
}
// 错误终结不带部分图和证据。
j::Object error(const std::string &id,const Failure &f) {
  return j::Object{{"protocol",Protocol},{"engine_version","0.1.1"},{"pack_id","c-cpp"},{"type","completed"},{"id",id.empty()?j::Value(nullptr):j::Value(id)},{"outcome","error"},{"error",j::Object{{"code",f.code},{"message",std::string(f.what())}}}};
}

} // namespace codegraph::clang_analyzer::detail
