// 公共一次性 JSON 边界与 CLI；所有图构造继续在当前 LibTooling 进程内进行。
#include <limits>
#ifdef _WIN32
#include <fcntl.h>
#include <io.h>
#endif
#include "sdk_discovery.hpp"

// Unicode 空白不能构成有效请求 ID；输入已通过严格 UTF-8 检查。
static bool blankId(const std::string &text) {
  size_t index=0;
  while(index<text.size()) {
    unsigned char first=text[index++]; unsigned codepoint=first;
    if(first>=0xc0) { unsigned count=first<0xe0?1:first<0xf0?2:3; codepoint=first&((1u<<(6-count))-1); while(count--) codepoint=(codepoint<<6)|(static_cast<unsigned char>(text[index++])&0x3f); }
    if(!(codepoint==0x20 || (codepoint>=9 && codepoint<=13) || codepoint==0x85 || codepoint==0xa0 || codepoint==0x1680 || (codepoint>=0x2000 && codepoint<=0x200a) || codepoint==0x2028 || codepoint==0x2029 || codepoint==0x202f || codepoint==0x205f || codepoint==0x3000)) return false;
  }
  return true;
}
// 解码阶段只检查公共字段，不在字符串解析期间访问项目或 SDK。
static void decodePublic(Request &request,const j::Object &message) {
  auto fields=j::Object(message); fields.erase("files"); keys(fields,{"protocol","type","id","repository_root","options"});
  auto id=str(message,"id"); require(!id.empty() && id.size()<=1024 && id.find('\0')==std::string::npos &&
    !blankId(id),"invalid request id"); request.id=id;
  require(str(message,"protocol")==Protocol,"unsupported analyzer protocol","incompatible_protocol");
  require(str(message,"type")=="analyze","expected analyze request");
  auto options=message.getObject("options"); require(options,"options must be an object"); keys(*options,{"include_tests","rust_dependency_scope"});
  require(options->getBoolean("include_tests").has_value(),"include_tests must be boolean"); auto scope=str(*options,"rust_dependency_scope");
  require(scope=="repository-only" || scope=="standard-library" || scope=="all-dependencies","invalid rust_dependency_scope");
  auto root=str(message,"repository_root"); require(root.find('\0')==std::string::npos && fs::u8path(root).is_absolute(),"repository_root must be an absolute directory"); request.root=fs::u8path(root);
  request.discovery=!message.get("files");
  if(!request.discovery) {
    auto files=message.getArray("files"); require(files,"files must be an array"); require(files->size()<=MaxFiles,"selection exceeds file limit","resource_limit");
    for(auto &entry:*files) { auto file=entry.getAsString(); require(file.has_value(),"files must contain strings"); relativePath(file->str()); require(request.selected.insert(file->str()).second,"duplicate selected path"); }
  }
}
// 对显式及发现选集共用真实路径边界；完整图不能包含同一物理文件的两个别名。
static void validateSourceRoot(Request &request) {
  request.root=real(request.root); require(fs::is_directory(request.root),"repository_root must be a directory");
  auto installation=installationRoot(); require(!under(request.root,installation) && !under(installation,request.root),"source overlaps analyzer installation");
  std::set<fs::path> physical;
  for(auto &file:request.selected) { auto relative=relativePath(file),full=real(request.root/relative);
    require(under(full,request.root) && !excluded(relative) && fs::is_regular_file(full) && physical.insert(full).second,"source outside allowed repository or physical alias");
    std::ifstream readable(full,std::ios::binary); require(bool(readable),"source is unreadable");
  }
}
// 在自然输出边界一次核对公共图和内部来源，不自动补节点、补引用或重跑分析。
static void validateOutput(const Request &request,const j::Object &result) {
  auto graph=result.getObject("graph"),evidence=result.getObject("evidence"); require(graph && evidence,"missing graph/source evidence","internal_failure");
  auto nodes=graph->getArray("nodes"),roots=graph->getArray("root_contains"),externals=graph->getArray("external_symbols");
  auto configurations=evidence->getArray("configurations"),files=evidence->getArray("files"),origins=evidence->getArray("origins");
  require(nodes && roots && externals && configurations && files && origins,"invalid result shape","internal_failure");
  std::set<std::string> configIds,platformIds,coverage,analyzed,nodeIds,originIds,externalIds,owned;
  std::map<std::string,const j::Object *> nodeMap;
  for(auto &platform:request.platforms) { platformIds.insert(platform.id); platformIds.insert(platform.resourceId); platformIds.insert(platform.additionalIds.begin(),platform.additionalIds.end()); }
  for(auto &entry:*configurations) {
    auto config=entry.getAsObject(); require(config && configIds.insert(str(*config,"id")).second,"duplicate configuration","internal_failure");
    auto sources=config->getArray("source_paths"),ids=config->getArray("platform_ids"); require(sources && ids,"invalid configuration evidence","internal_failure");
    for(auto &source:*sources) { auto file=source.getAsString(); require(file.has_value(),"invalid configuration source","internal_failure"); auto full=real(request.root/relativePath(file->str())); require(under(full,request.root) && fs::is_regular_file(full),"configuration source escapes root","internal_failure"); }
    for(auto &id:*ids) { auto value=id.getAsString(); require(value && platformIds.count(value->str()),"configuration uses unapproved SDK","internal_failure"); }
  }
  require(!request.discovery || !configIds.empty(),"discovery lacks configuration","internal_failure");
  require(files->size()<=MaxFiles,"coverage exceeds limit","internal_failure"); std::set<fs::path> physical;
  for(auto &entry:*files) {
    auto file=entry.getAsObject(); require(file,"invalid file evidence","internal_failure"); auto path=str(*file,"path"),disposition=str(*file,"disposition");
    require(request.selected.count(path) && coverage.insert(path).second,"coverage differs from selection","internal_failure");
    auto full=real(request.root/relativePath(path)); require(under(full,request.root) && fs::is_regular_file(full) && physical.insert(full).second,"invalid physical source coverage","internal_failure");
    auto ids=file->getArray("configuration_ids"); require(ids && !ids->empty(),"source lacks configuration","internal_failure"); std::set<std::string> seen;
    for(auto &id:*ids) { auto value=id.getAsString(); require(value && configIds.count(value->str()) && seen.insert(value->str()).second,"invalid source configuration","internal_failure"); }
    require(disposition=="analyzed","invalid C/C++ source disposition","internal_failure"); analyzed.insert(path);
  }
  require(coverage==request.selected,"source coverage mismatch","internal_failure");
  const std::set<std::string> categories{"container","type","callable","value","execution_region","extension"};
  for(auto &entry:*nodes) { auto node=entry.getAsObject(); require(node,"invalid graph node","internal_failure"); auto id=str(*node,"id"),language=str(*node,"language");
    require(!id.empty() && nodeIds.insert(id).second && (language=="c" || language=="cpp") && id.rfind(language+":",0)==0 && categories.count(str(*node,"category")) && !str(*node,"native_kind").empty(),"invalid graph identity","internal_failure"); nodeMap[id]=node; }
  for(auto &entry:*origins) { auto origin=entry.getAsObject(); require(origin,"invalid origin","internal_failure"); auto id=str(*origin,"node_id"); auto paths=origin->getArray("selected_paths"); require(nodeIds.count(id) && originIds.insert(id).second && paths && !paths->empty(),"invalid node origin","internal_failure"); std::set<std::string> seen;
    for(auto &path:*paths) { auto value=path.getAsString(); require(value && analyzed.count(value->str()) && seen.insert(value->str()).second,"origin outside selected source","internal_failure"); } }
  require(originIds==nodeIds,"node/source evidence mismatch","internal_failure");
  for(auto &entry:*externals) { auto external=entry.getAsObject(); require(external,"invalid external descriptor","internal_failure"); auto scope=str(*external,"scope"); require(externalIds.insert(str(*external,"symbol")).second && (scope=="repository" || scope=="dependency"),"invalid external symbol","internal_failure"); }
  std::vector<std::string> pending;
  for(auto &entry:*roots) { auto id=entry.getAsString(); require(id && nodeIds.count(id->str()) && owned.insert(id->str()).second,"invalid graph root","internal_failure"); pending.push_back(id->str()); }
  for(auto &entry:nodeMap) {
    auto children=entry.second->getArray("contains"),references=entry.second->getArray("references"); require(children && references,"missing node members","internal_failure");
    for(auto &child:*children) { auto id=child.getAsString(); require(id && nodeIds.count(id->str()) && owned.insert(id->str()).second,"invalid containment","internal_failure"); }
    for(auto &reference:*references) { auto ref=reference.getAsObject(); require(ref && !str(*ref,"semantic").empty(),"invalid reference","internal_failure"); auto target=ref->getObject("target"); require(target,"missing reference target","internal_failure"); auto kind=str(*target,"kind");
      require(target->size()==2 && ((kind=="node" && nodeIds.count(str(*target,"id"))) || (kind=="external" && externalIds.count(str(*target,"symbol"))) || (kind=="unresolved" && !str(*target,"name").empty())),"invalid reference target","internal_failure"); }
  }
  std::set<std::string> visited;
  while(!pending.empty()) { auto id=std::move(pending.back()); pending.pop_back(); require(visited.insert(id).second,"containment cycle","internal_failure"); for(auto &child:*nodeMap.at(id)->getArray("contains")) pending.push_back(child.getAsString()->str()); }
  require(visited==nodeIds,"unreachable nodes","internal_failure");
}
// 固定错误码与内部语言诊断分开；普通 unresolved 关系不会触发失败。
static std::string publicCode(const std::string &code) {
  static const std::set<std::string> codes{"invalid_request","incompatible_protocol","invalid_input","invalid_pack","analysis_failed","resource_limit","timeout","io_failure","cleanup_failed","internal_failure"};
  return codes.count(code)?code:"analysis_failed";
}
// 标准输入以原始字节读取直到 EOF，Windows 关闭 CRT 文本转换和 Ctrl-Z 提前终止。
static std::string readPublic() {
#ifdef _WIN32
  require(_setmode(_fileno(stdin),_O_BINARY)!=-1 && _setmode(_fileno(stdout),_O_BINARY)!=-1,"cannot set binary pipes","io_failure");
#endif
  std::string bytes; char chunk[65536];
  while(std::cin) { std::cin.read(chunk,sizeof(chunk)); auto count=std::cin.gcount(); require(bytes.size()+static_cast<size_t>(count)<=MaxPublicBytes,"request exceeds byte limit","resource_limit"); bytes.append(chunk,static_cast<size_t>(count)); }
  require(std::cin.eof(),"stdin read failed","io_failure"); return bytes;
}
// 公共 CLI 只允许一次 analyze；调用方取消通过终止进程树实现。
int main(int argc,char **argv) {
  if((argc==2 && (std::string(argv[1])=="--help" || std::string(argv[1])=="--version")) || (argc==3 && std::string(argv[1])=="analyze" && std::string(argv[2])=="--help")) {
    std::cout<<(std::string(argv[1])=="--version"?"codegraph-c-cpp 0.1.0\n":"Usage: codegraph-c-cpp analyze\nReads one UTF-8 JSON request followed by EOF.\n"); return 0;
  }
  if(argc!=2 || std::string(argv[1])!="analyze") { std::cerr<<"Usage: codegraph-c-cpp analyze\n"; return 2; }
  Request request; j::Object result; bool success=false;
  try {
    auto bytes=readPublic();
    try {
      require(j::isUTF8(bytes),"request must be UTF-8"); auto value=parse(bytes); auto object=value.getAsObject(); require(object,"request must be an object"); decodePublic(request,*object);
    } catch(const Failure &failure) { throw Failure(failure.code=="incompatible_protocol" || failure.code=="resource_limit"?failure.code:"invalid_request",failure.what()); }
    validateSourceRoot(request);
    if(request.discovery || !request.selected.empty()) { discoverPlatforms(request); configureProject(request); }
    for(auto name:{"CPATH","C_INCLUDE_PATH","CPLUS_INCLUDE_PATH","OBJC_INCLUDE_PATH","SDKROOT","MACOSX_DEPLOYMENT_TARGET","CCC_OVERRIDE_OPTIONS","CLANG_CONFIG_FILE_SYSTEM_DIR","CLANG_CONFIG_FILE_USER_DIR","INCLUDE","CL","_CL_"}) clearEnvironment(name);
    result=analyze(request); validateOutput(request,result); result.erase("evidence"); success=true;
  } catch(const Failure &failure) { result=error(request.id,Failure(publicCode(failure.code),failure.what())); }
    catch(const std::exception &exception) { result=error(request.id,Failure("internal_failure",exception.what())); }
  auto output=jsonText(j::Value(std::move(result)));
  if(output.size()+1>MaxPublicBytes) { output=jsonText(error(request.id,Failure("resource_limit","result exceeds byte limit"))); success=false; }
  std::cout<<output<<'\n'; std::cout.flush(); if(!std::cout) return 1; return success?0:1;
}
