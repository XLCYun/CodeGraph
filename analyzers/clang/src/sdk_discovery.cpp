#include "detail/analysis.hpp"
#include <algorithm>
#include <cstdlib>
#include <limits>
#include <sstream>
#ifdef _WIN32
#include <windows.h>
#endif
#ifdef __APPLE__
#include <mach-o/dyld.h>
#endif
namespace codegraph::clang_analyzer::detail {

// 从环境读取 Unicode 安装线索，Windows 不经过当前 ANSI 代码页。
static fs::path environmentPath(const char *name) {
#ifdef _WIN32
  std::wstring wide; for(const char *p=name;*p;++p) wide.push_back(static_cast<wchar_t>(*p));
  auto value=_wgetenv(wide.c_str()); return value?fs::path(value):fs::path();
#else
  auto value=std::getenv(name); return value?fs::u8path(value):fs::path();
#endif
}
// 版本只接受稳定数字段，按数字比较已安装补丁。
static std::vector<unsigned> numericVersion(const std::string &value) {
  std::vector<unsigned> result; std::istringstream stream(value); std::string part;
  while(std::getline(stream,part,'.')) {
    if(part.empty() || !std::all_of(part.begin(),part.end(),[](unsigned char c){return c>='0' && c<='9';})) return {};
    try { auto number=std::stoull(part); if(number>std::numeric_limits<unsigned>::max()) return {}; result.push_back(static_cast<unsigned>(number)); } catch(...) { return {}; }
  }
  if(value.empty() || value.back()=='.' || result.size()<2) return {}; return result;
}
// 常规安装位置不存在时继续其他线索；目录顺序固定以便同版本选择可重复。
static std::vector<fs::path> installedDirectories(const fs::path &root) {
  std::vector<fs::path> result; std::error_code ec;
  fs::directory_iterator entry(root,ec),end;
  while(!ec && entry!=end) { if(entry->is_directory(ec)) result.push_back(entry->path()); entry.increment(ec); }
  std::sort(result.begin(),result.end()); return result;
}
// 取得本程序真实位置，包内资源从相对位置定位，不读 catalog 或执行查询工具。
static fs::path executablePath() {
#ifdef _WIN32
  std::vector<wchar_t> buffer(32768); auto length=GetModuleFileNameW(nullptr,buffer.data(),static_cast<DWORD>(buffer.size()));
  require(length>0 && length<buffer.size(),"cannot locate analyzer","internal_failure"); return real(fs::path(std::wstring(buffer.data(),length)));
#elif defined(__APPLE__)
  uint32_t length=0; _NSGetExecutablePath(nullptr,&length); std::vector<char> buffer(length);
  require(_NSGetExecutablePath(buffer.data(),&length)==0,"cannot locate analyzer","internal_failure"); return real(fs::u8path(buffer.data()));
#else
  return real("/proc/self/exe");
#endif
}
// bin 的父目录是安装根，本机构建输出目录也可作为独立测试安装。
fs::path installationRoot() {
  auto directory=executablePath().parent_path(); return directory.filename()=="bin"?directory.parent_path():directory;
}
// 读取 SDK 自身声明，沿用默认部署目标及 C/C++ 标准头验证。
j::Object macosPlatform(const fs::path &path) {
  auto root=real(path); auto settings=parse(read(root/"SDKSettings.json")); auto object=settings.getAsObject(); require(object,"invalid macOS SDK settings","target_unavailable");
  auto version=str(*object,"Version"); require(!numericVersion(version).empty(),"invalid macOS SDK version","target_unavailable");
  for(auto header:{"usr/include/stdio.h","usr/include/c++/v1/vector"}) { auto file=real(root/header); require(under(file,root) && fs::is_regular_file(file),"missing macOS standard header","target_unavailable"); }
  std::string target=version;
  if(auto defaults=object->getObject("DefaultProperties")) if(auto deployment=defaults->getString("MACOSX_DEPLOYMENT_TARGET")) if(!numericVersion(deployment->str()).empty()) target=deployment->str();
  return j::Object{{"id","macos-"+version},{"kind","clang-target"},{"version",version},{"root",root.u8string()},
    {"metadata",j::Object{{"target_triple","arm64-apple-macosx"+target},{"system_include_directories",j::Array{"usr/include/c++/v1","usr/include"}},{"resource_platform_id","clang-resource-21"}}}};
}
// 优先显式 SDK、当前开发工具的默认 SDK，再查找常规安装中的最高稳定版本。
static j::Object discoverMacOS() {
  for(auto key:{"CODEGRAPH_MACOS_SDK","SDKROOT"}) { auto path=environmentPath(key); if(path.is_absolute()) return macosPlatform(path); }
  std::vector<fs::path> developers; auto explicitDeveloper=environmentPath("DEVELOPER_DIR"); if(!explicitDeveloper.empty()) developers.push_back(explicitDeveloper);
  for(auto path:{"/var/db/xcode_select_link","/Library/Developer/CommandLineTools","/Applications/Xcode.app/Contents/Developer"}) developers.emplace_back(path);
  for(auto &application:installedDirectories("/Applications")) { auto name=application.filename().u8string(); if(name.rfind("Xcode",0)==0 && application.extension()==".app") developers.push_back(application/"Contents/Developer"); }
  for(auto &developer:developers) for(auto &directory:{developer/"Platforms/MacOSX.platform/Developer/SDKs",developer/"SDKs"}) {
    try { return macosPlatform(directory/"MacOSX.sdk"); } catch(const std::exception &) {}
    std::vector<std::pair<std::vector<unsigned>,fs::path>> installed;
    for(auto &candidate:installedDirectories(directory)) try { auto value=macosPlatform(candidate); installed.push_back({numericVersion(str(value,"version")),candidate}); } catch(const std::exception &) {}
    std::sort(installed.begin(),installed.end()); if(!installed.empty()) return macosPlatform(installed.back().second);
  }
  throw Failure("target_unavailable","未找到 macOS SDK 头文件，请安装 Xcode 或 Command Line Tools");
}
// 从版本目录选择带有必要头文件的最高稳定安装。
static fs::path newestInstallation(const fs::path &root,const fs::path &header) {
  std::vector<std::pair<std::vector<unsigned>,fs::path>> candidates;
  for(auto &directory:installedDirectories(root)) { auto version=numericVersion(directory.filename().u8string()); if(!version.empty() && fs::is_regular_file(directory/header)) candidates.push_back({version,directory}); }
  std::sort(candidates.begin(),candidates.end()); return candidates.empty()?fs::path():candidates.back().second;
}
// Windows SDK 与 MSVC 独立授权具体 include 根，不把任意 compiler query-driver 作为发现手段。
j::Array discoverWindows() {
  auto kits=environmentPath("ProgramFiles(x86)"),programs=environmentPath("ProgramFiles");
  if(kits.empty()) kits=fs::path(L"C:\\Program Files (x86)"); if(programs.empty()) programs=fs::path(L"C:\\Program Files");
  auto sdk=environmentPath("CODEGRAPH_WINDOWS_SDK"),msvc=environmentPath("CODEGRAPH_MSVC_ROOT");
  if(sdk.empty()) sdk=newestInstallation(kits/"Windows Kits/10/Include","um/Windows.h");
  require(!sdk.empty(),"未找到 Windows SDK 头文件","target_unavailable");
  fs::path standard;
  if(!msvc.empty()) standard=msvc/"include";
  else {
    std::vector<std::pair<std::vector<unsigned>,fs::path>> candidates;
    for(auto &base:{programs,kits}) for(auto &release:installedDirectories(base/"Microsoft Visual Studio")) for(auto &edition:installedDirectories(release)) {
      auto tools=newestInstallation(edition/"VC/Tools/MSVC","include/vector"); if(!tools.empty()) candidates.push_back({numericVersion(tools.filename().u8string()),tools});
    }
    std::sort(candidates.begin(),candidates.end()); require(!candidates.empty(),"未找到 MSVC C++ 标准库头文件","target_unavailable"); standard=candidates.back().second/"include";
  }
  sdk=real(sdk); standard=real(standard); auto version=sdk.filename().u8string(),msvcVersion=standard.parent_path().filename().u8string();
  require(!numericVersion(version).empty() && !numericVersion(msvcVersion).empty(),"invalid Windows SDK/MSVC version directory","target_unavailable");
  for(auto header:{"ucrt","shared","um","winrt","cppwinrt","um/Windows.h"}) require(under(real(sdk/header),sdk),"Windows SDK header escapes root","target_unavailable");
  for(auto header:{"vector","yvals_core.h"}) require(under(real(standard/header),standard),"MSVC header escapes root","target_unavailable");
  return j::Array{
    j::Object{{"id","msvc-standard-headers"},{"kind","clang-system-headers"},{"version",msvcVersion},{"root",standard.u8string()},{"metadata",j::Object{}}},
    j::Object{{"id","windows-sdk-"+version},{"kind","clang-target"},{"version",version},{"root",sdk.u8string()},
      {"metadata",j::Object{{"target_triple","x86_64-pc-windows-msvc"},{"system_include_directories",j::Array{"ucrt","shared","um","winrt","cppwinrt"}},{"resource_platform_id","clang-resource-21"},{"additional_system_platform_ids",j::Array{"msvc-standard-headers"}}}}}};
}
// 目标 SDK 与随包的固定 Clang builtin headers 分开定位，编译器运行时不是项目 SDK。
void discoverPlatforms(Request &request) {
  auto resource=real(installationRoot()/"runtime/platform/clang-resource");
  require(fs::is_regular_file(resource/"include/stddef.h"),"missing bundled Clang resource headers","invalid_pack");
#ifdef _WIN32
  auto catalog=discoverWindows();
#else
  j::Array catalog{discoverMacOS()};
#endif
  catalog.push_back(j::Object{{"id","clang-resource-21"},{"kind","clang-resource"},{"version","21.1.8"},{"root",resource.u8string()},{"metadata",j::Object{}}});
  platforms(request,catalog);
  for(auto &platform:request.platforms) {
    auto roots=platform.includes; roots.push_back(platform.root); roots.push_back(platform.resource);
    for(auto &root:roots) require(!under(root,request.root) && !under(request.root,root),"target SDK overlaps source repository","invalid_input");
  }
}

} // namespace codegraph::clang_analyzer::detail
