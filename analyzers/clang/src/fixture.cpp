#include "analyzer.hpp"
#include "detail/analysis.hpp"
#include <istream>
#include <iterator>
#include <ostream>
namespace codegraph::clang_analyzer::detail {

// 将历史语义 fixture 转换为进程内请求，以保留精确平台、来源和资源限制断言。
static void fixtureRequest(Request &r,const j::Object &message) {
  r.id=str(message,"id"); auto input=message.getObject("input"),environment=message.getObject("environment"),limits=message.getObject("limits");
  require(input && environment && limits,"invalid fixture"); r.root=real(fs::u8path(str(*input,"repository_root"))); r.discovery=!input->get("files");
  auto timeout=limits->getInteger("analysis_timeout_ms"),result=limits->getInteger("max_result_bytes"),diagnostic=limits->getInteger("max_diagnostic_bytes");
  require(timeout && *timeout>0 && result && *result>0 && diagnostic && *diagnostic>0,"invalid fixture limits");
  r.deadline=std::chrono::steady_clock::now()+std::chrono::milliseconds(*timeout); r.maxResult=*result; r.maxDiagnostic=*diagnostic;
  if(!r.discovery) {
    auto files=input->getArray("files"); require(files && files->size()<=MaxFiles,"invalid fixture files");
    for(auto &entry:*files) { auto file=entry.getAsString(); require(file.has_value(),"fixture file must be string"); auto path=relativePath(file->str()),full=real(r.root/path); require(under(full,r.root) && !excluded(path) && fs::is_regular_file(full) && r.selected.insert(file->str()).second,"invalid fixture source"); }
    if(files->empty()) return;
  }
  auto catalog=environment->getArray("platforms"); require(catalog,"fixture catalog must be array"); platforms(r,*catalog); configureProject(r);
}

} // namespace codegraph::clang_analyzer::detail

namespace codegraph::clang_analyzer::testing {

// 测试 adapter 不编排内部阶段，也不改变夹具的错误码或退出约定。
int runFixtureAnalysis(std::istream &input, std::ostream &output) {
  using namespace detail;
  Request request; j::Object result;
  try { std::string bytes(std::istreambuf_iterator<char>(input),{}); auto value=parse(bytes); auto fixture=value.getAsObject(); require(fixture,"fixture must be object"); fixtureRequest(request,*fixture); result=analyze(request);
    if(jsonText(j::Value(j::Object(result))).size()>request.maxResult) throw Failure("resource_limit","result byte limit exceeded");
  } catch(const Failure &failure) { result=error(request.id,failure); }
    catch(const std::exception &exception) { result=error(request.id,Failure("internal_failure",exception.what())); }
  output<<jsonText(j::Value(std::move(result)))<<'\n'; return 0;
}

} // namespace codegraph::clang_analyzer::testing
