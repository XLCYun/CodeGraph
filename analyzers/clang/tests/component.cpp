// 测试专用驱动直接调用 LibTooling 组件，不编入或交付生产二进制。
#define CODEGRAPH_COMPONENT_TEST
#include "../src/main.cpp"
#include "sdk_discovery.hpp"

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
// EOF 后调用核心并输出来源断言所需的内部证据，测试 driver 不提供控制消息循环。
int main(int argc,char **argv) {
  if(argc==3 && std::string(argv[1])=="--sdk-discovery-tests") return sdkDiscoveryTests(fs::u8path(argv[2]));
  Request request; j::Object result;
  try { std::string bytes(std::istreambuf_iterator<char>(std::cin),{}); auto value=parse(bytes); auto fixture=value.getAsObject(); require(fixture,"fixture must be object"); fixtureRequest(request,*fixture); result=analyze(request);
    if(jsonText(j::Value(j::Object(result))).size()>request.maxResult) throw Failure("resource_limit","result byte limit exceeded");
  } catch(const Failure &failure) { result=error(request.id,failure); }
    catch(const std::exception &exception) { result=error(request.id,Failure("internal_failure",exception.what())); }
  std::cout<<jsonText(j::Value(std::move(result)))<<'\n'; return 0;
}
