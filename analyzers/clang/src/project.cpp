#include "detail/analysis.hpp"
#include <clang/Basic/DiagnosticIDs.h>
#include <clang/Basic/LangStandard.h>
#include <clang/Driver/Types.h>
#include <algorithm>
#include <cstdlib>
#include <fstream>
#ifdef _WIN32
#include <windows.h>
#else
#include <unistd.h>
#endif

using namespace clang;
namespace codegraph::clang_analyzer::detail {

// 校验平台 catalog；SDK 与工具 resource headers 分别来自批准根。
void platforms(Request &r,const j::Array &catalog) {
  std::map<std::string,const j::Object *> entries;
  for(auto &v:catalog) {
    r.checkpoint(); auto o=v.getAsObject(); require(o,"platform must be an object"); keys(*o,{"id","kind","version","root","metadata"});
    auto id=str(*o,"id"); require(!id.empty() && entries.emplace(id,o).second,"duplicate/empty platform id");
    require(!str(*o,"version").empty(),"empty platform version");
  }
  for(auto &entry:entries) {
    r.checkpoint(); auto &o=*entry.second; auto kind=str(o,"kind"); require(kind=="clang-target" || kind=="clang-resource" || kind=="clang-system-headers","unsupported platform kind");
    auto meta=o.getObject("metadata"); require(meta,"platform metadata must be an object");
    if(kind=="clang-system-headers") { keys(*meta,{}); require(fs::u8path(str(o,"root")).is_absolute() && fs::is_directory(real(fs::u8path(str(o,"root")))),"invalid system header root"); continue; }
    if(kind=="clang-resource") { keys(*meta,{}); require(str(o,"version")=="21.1.8","incompatible resource headers","target_unavailable"); continue; }
    auto required=j::Object(*meta); required.erase("additional_system_platform_ids");
    keys(required,{"target_triple","system_include_directories","resource_platform_id"});
    Platform p; p.id=entry.first; p.root=real(fs::u8path(str(o,"root"))); require(fs::u8path(str(o,"root")).is_absolute(),"platform root must be absolute");
    p.triple=str(*meta,"target_triple"); require(!p.triple.empty() && p.triple.find_first_of(" \t\r\n") == std::string::npos,"invalid target triple");
    p.resourceId=str(*meta,"resource_platform_id"); auto resource=entries.find(p.resourceId); require(resource!=entries.end() && str(*resource->second,"kind")=="clang-resource","missing resource platform","target_unavailable");
    require(str(*resource->second,"version")=="21.1.8","incompatible resource headers","target_unavailable");
    p.resource=real(fs::u8path(str(*resource->second,"root"))); require(fs::u8path(str(*resource->second,"root")).is_absolute(),"resource root must be absolute");
    auto dirs=meta->getArray("system_include_directories"); require(dirs,"system include directories must be an array");
    for(auto &d:*dirs) { auto s=d.getAsString(); require(s.has_value(),"include directory must be string"); auto dir=real(p.root/relativePath(s->str())); require(under(dir,p.root) && fs::is_directory(dir),"include directory escapes platform"); p.includes.push_back(dir); }
    if(auto value=meta->get("additional_system_platform_ids")) {
      auto ids=value->getAsArray(); require(ids,"additional system platform IDs must be an array"); std::set<std::string> seen;
      for(auto &idValue:*ids) {
        auto id=idValue.getAsString(); require(id && !id->empty() && seen.insert(id->str()).second && *id!=p.id,"invalid additional system platform ID");
        auto found=entries.find(id->str()); require(found!=entries.end() && str(*found->second,"kind")=="clang-system-headers","missing additional system header platform","target_unavailable");
        auto root=real(fs::u8path(str(*found->second,"root"))); p.includes.push_back(root); p.additionalIncludes.push_back(root); p.additionalIds.push_back(id->str());
      }
    }
    r.platforms.push_back(std::move(p));
  }
  require(!r.platforms.empty(),"no approved Clang target platform","target_unavailable");
}
// 使用固定 LLVM 的语言表解析标准及其别名，只排除该版本仍标记为草案的方言。
static const LangStandard &languageStandard(const std::string &name) {
  auto standard=LangStandard::getLangStandardForName(name);
  require(standard && (standard->getLanguage()==Language::C || standard->getLanguage()==Language::CXX),
          "unsupported C/C++ language standard: "+name,"unsupported_configuration");
  require(!standard->isC2y() && !standard->isCPlusPlus26(),
          "preview language standard is unsupported: "+name,"unsupported_configuration");
  return *standard;
}
// GNU 方言保留原生扩展；后续 ISO 标准的语法仍通过 LLVM 自己的诊断分组严格检查。
static std::vector<std::string> standardDiagnostics(const LangStandard &standard) {
  if(!standard.isGNUMode()) return {"-pedantic-errors"};
  std::vector<std::string> flags;
  for(const auto &flag:DiagnosticIDs::getDiagnosticFlags()) {
    llvm::StringRef name(flag);
    if(!name.consume_front("-W") || !name.consume_back("-extensions")) continue;
    auto extension=LangStandard::getLangStandardForName(name);
    if(extension && extension->getLanguage()==standard.getLanguage()) flags.push_back("-Werror="+flag.substr(2));
  }
  return flags;
}
// 按 Clang 的文件类型表和 C++ driver 规则推断未被 -x 覆盖的输入语言。
static std::string sourceLanguage(const fs::path &file,const std::string &driverName) {
  auto extension=file.extension().u8string();
  auto type=driver::types::lookupTypeForExtension(llvm::StringRef(extension).drop_front(extension.empty()?0:1));
  if(driverName=="clang++" || driverName=="clang++.exe" || driverName=="c++" || driverName=="g++") type=driver::types::lookupCXXTypeForCType(type);
  if(type==driver::types::TY_C || type==driver::types::TY_CHeader) return "c";
  if(type==driver::types::TY_CXX || type==driver::types::TY_CXXHeader) return "cpp";
  throw Failure("unsupported_configuration","unsupported C/C++ source language: "+file.filename().u8string());
}
// 编译数据库参数是数据；只重建经过白名单校验的编译上下文。
static Config configuration(Request &r,const j::Object &o,size_t index) {
  for(auto &kv:o) require(kv.first=="directory" || kv.first=="file" || kv.first=="arguments" || kv.first=="output","unsupported compilation database field","unsupported_configuration");
  Config c; c.id="tu-"+std::to_string(index); c.directory=real(fs::u8path(str(o,"directory"))); require(under(c.directory,r.root),"CDB directory outside repository","unsupported_configuration");
  auto path=fs::u8path(str(o,"file")); c.file=real(path.is_absolute()?path:c.directory/path); require(under(c.file,r.root) && !excluded(c.file.lexically_relative(r.root)),"CDB source outside allowed repository","unsupported_configuration");
  auto args=o.getArray("arguments"); require(args && !args->empty(),"CDB requires arguments array, command strings are not executable input","unsupported_configuration");
  std::vector<std::string> input; for(auto &v:*args) { r.checkpoint(); auto s=v.getAsString(); require(s.has_value() && s->find('\0')==llvm::StringRef::npos,"invalid CDB argument"); input.push_back(s->str()); }
  auto driver=fs::u8path(input.front()).filename().u8string(); require(driver=="clang" || driver=="clang++" || driver=="clang.exe" || driver=="clang++.exe" || driver=="cc" || driver=="c++" || driver=="gcc" || driver=="g++","compiler wrappers are unsupported","unsupported_configuration");
  std::string triple,standard,explicitLanguage; std::vector<fs::path> roots; bool sourceSeen=false;
  // 参数值必须存在，禁止忽略截断的命令。
  auto next=[&](size_t &i)->std::string { require(i+1<input.size(),"missing CDB argument value","unsupported_configuration"); return input[++i]; };
  // 仓内 include 目录也必须排除安装缓存和符号链接越界。
  auto include=[&](std::string value)->std::string {
    fs::path p=fs::u8path(value); auto resolved=real(p.is_absolute()?p:c.directory/p);
    bool allowed=under(resolved,r.root) && !excluded(resolved.lexically_relative(r.root));
    for(auto &platform:r.platforms) for(auto &dir:platform.includes) if(under(resolved,dir)) allowed=true;
    require(allowed && fs::is_directory(resolved),"unapproved include directory","unsupported_configuration"); return resolved.u8string();
  };
  for(size_t i=1;i<input.size();++i) {
    r.checkpoint(); auto a=input[i];
    if(a=="-c" || a=="-fsyntax-only" || a=="-g") continue;
    if(a=="-O0" || a=="-O1" || a=="-O2" || a=="-O3" || a=="-Os" || a=="-Oz" || a=="-Og") { c.args.push_back(a); continue; }
    if(a=="-o") { next(i); continue; }
    if(a.rfind("-x",0)==0) {
      auto mode=a=="-x"?next(i):a.substr(2);
      require(mode=="c" || mode=="c++" || mode=="none","unsupported language mode: "+mode,"unsupported_configuration");
      explicitLanguage=mode=="none"?"":mode=="c"?"c":"cpp"; continue;
    }
    if(a.rfind("-std=",0)==0 || a.rfind("--std=",0)==0 || a=="--std") {
      standard=a=="--std"?next(i):a.substr(a.find('=')+1); languageStandard(standard); continue;
    }
    if(a=="-target" || a=="--target") { triple=next(i); continue; }
    if(a.rfind("--target=",0)==0) { triple=a.substr(9); continue; }
    if(a=="-isysroot" || a=="--sysroot") { roots.push_back(real(fs::u8path(next(i)))); continue; }
    if(a.rfind("--sysroot=",0)==0) { roots.push_back(real(fs::u8path(a.substr(10)))); continue; }
    if(a=="-I" || a=="-isystem" || a=="-iquote") { c.args.push_back(a); c.args.push_back(include(next(i))); continue; }
    if(a.rfind("-I",0)==0 && a.size()>2) { c.args.push_back("-I"+include(a.substr(2))); continue; }
    if(a=="-D" || a=="-U") { c.args.push_back(a+next(i)); continue; }
    if((a.rfind("-D",0)==0 || a.rfind("-U",0)==0) && a.size()>2) { c.args.push_back(a); continue; }
    if(!a.empty() && a[0]!='-' && a[0]!='@') {
      fs::path f=fs::u8path(a); std::error_code ec; auto resolved=fs::canonical(f.is_absolute()?f:c.directory/f,ec);
      require(!ec && resolved==c.file && !sourceSeen,"unexpected compilation input","unsupported_configuration");
      c.language=explicitLanguage.empty()?sourceLanguage(f,driver):explicitLanguage; sourceSeen=true; continue;
    }
    throw Failure("unsupported_configuration","unapproved compiler argument: "+a);
  }
  require(sourceSeen,"CDB arguments omit source file","unsupported_configuration");
  require(!standard.empty(),"explicit -std language standard required","unsupported_configuration");
  const auto &dialect=languageStandard(standard);
  require(dialect.getLanguage()==(c.language=="c"?Language::C:Language::CXX),
          "language standard "+standard+" does not match input language "+c.language,"unsupported_configuration");
  std::vector<size_t> matches;
  for(size_t i=0;i<r.platforms.size();++i) {
    auto &p=r.platforms[i]; if(!triple.empty() && triple!=p.triple) continue;
    if(std::any_of(roots.begin(),roots.end(),[&](auto &root){return root!=p.root;})) continue; matches.push_back(i);
  }
  require(matches.size()==1,"target platform missing or ambiguous","target_unavailable"); c.platform=matches[0]; auto &p=r.platforms[c.platform];
  std::vector<std::string> fixed={"-x",c.language=="c"?"c":"c++","-std="+standard,"--target="+p.triple,"-fsyntax-only","-nostdinc","-nostdinc++","-fno-modules","-fno-implicit-modules","-fno-implicit-module-maps","-fno-delayed-template-parsing","-resource-dir",p.resource.u8string(),"-isysroot",p.root.u8string()};
  auto diagnostics=standardDiagnostics(dialect); fixed.insert(fixed.end(),diagnostics.begin(),diagnostics.end());
  // 标准搜索次序为 C++ 包装头、编译器内建头、目标 C 头；否则 include_next 会选错 stddef。
  auto isCxx=[](const fs::path &dir) { return std::any_of(dir.begin(),dir.end(),[](const fs::path &part){return part=="c++";}); };
  for(auto &dir:p.includes) if(isCxx(dir) || std::find(p.additionalIncludes.begin(),p.additionalIncludes.end(),dir)!=p.additionalIncludes.end()) { fixed.push_back("-isystem"); fixed.push_back(dir.u8string()); }
  fixed.push_back("-isystem"); fixed.push_back((p.resource/"include").u8string());
  for(auto &dir:p.includes) if(!isCxx(dir) && std::find(p.additionalIncludes.begin(),p.additionalIncludes.end(),dir)==p.additionalIncludes.end()) { fixed.push_back("-isystem"); fixed.push_back(dir.u8string()); }
  fixed.insert(fixed.end(),c.args.begin(),c.args.end()); c.args=std::move(fixed); return c;
}
// 真实 CDB 已读取后发布只读验收的外部观察；硬链接原子发布且绝不覆盖旧目标。
static void observeDiscovery(const std::string &id) {
#ifdef _WIN32
  auto value=_wgetenv(L"CODEGRAPH_TEST_DISCOVERY_PROBE"); if(!value) return;
  auto destination=fs::path(value);
  auto pid=GetCurrentProcessId();
#else
  auto value=std::getenv("CODEGRAPH_TEST_DISCOVERY_PROBE"); if(!value) return;
  auto destination=fs::u8path(value);
  auto pid=getpid();
#endif
  auto pending=destination.parent_path()/"reached.pending";
  auto bytes=jsonText(j::Object{{"schema_version",1},{"helper_id","codegraph-clang-engine"},{"request_id",id},{"pid",static_cast<int64_t>(pid)},{"milestone","compilation-database-read"}});
  require(bytes.size()<=4096,"acceptance observation exceeds limit");
  require(fs::create_directory(pending),"acceptance observation already exists");
  try {
    auto temporary=pending/"marker.json";
    { std::ofstream file(temporary,std::ios::binary); file<<bytes; file.close(); require(!file.fail(),"acceptance observation write failed"); }
    fs::create_hard_link(temporary,destination);
  } catch(...) { std::error_code ignored; fs::remove_all(pending,ignored); throw; }
  std::error_code ignored; fs::remove_all(pending,ignored);
}
// 从已经确定的进程内选集与目标加载静态 CDB，不执行其中的命令。
void configureProject(Request &r) {
  auto path=r.root/"compile_commands.json"; require(fs::exists(path),"clang-static-v1 requires compile_commands.json","unsupported_configuration"); require(under(real(path),r.root),"CDB symlink escapes repository");
  require(fs::file_size(path)<=1024*1024,"CDB exceeds control byte limit","resource_limit");
  auto cdb=parse(read(path)); auto entries=cdb.getAsArray(); require(entries && !entries->empty(),"CDB must be a nonempty array","unsupported_configuration");
  if(r.discovery) observeDiscovery(r.id);
  for(auto &v:*entries) { r.checkpoint(); auto o=v.getAsObject(); require(o,"CDB entry must be object"); auto c=configuration(r,*o,r.configs.size()); if(r.discovery) {
      auto relative=c.file.lexically_relative(r.root).generic_u8string(); relativePath(relative);
      r.selected.insert(relative); require(r.selected.size()<=MaxFiles,"Clang discovery exceeds file limit","resource_limit");
    }
    r.configs.push_back(std::move(c)); }
}

} // namespace codegraph::clang_analyzer::detail
