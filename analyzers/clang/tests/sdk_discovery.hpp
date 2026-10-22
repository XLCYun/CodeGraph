// 非交付 SDK 回归直接调用生产发现组件，不提供私有生产入口。
#include <limits>
#include "../src/sdk_discovery.hpp"

// 创建只供静态元数据读取的文件，不编译或运行任何项目程序。
static void sdkFixtureFile(const fs::path &path,const std::string &content="") {
  fs::create_directories(path.parent_path()); std::ofstream(path)<<content;
}
// 在测试子进程内设置发现线索；退出后不会修改调用方环境。
static void sdkFixtureEnvironment(const char *key,const fs::path &value) {
#ifdef _WIN32
  std::wstring name; for(const char *p=key;*p;++p) name.push_back(*p); _wputenv_s(name.c_str(),value.c_str());
#else
  setenv(key,value.c_str(),1);
#endif
}
// 保留旧根平台发现的版本、部署目标、独立 Windows/MSVC 根及缺头拒绝断言。
static int sdkDiscoveryTests(const fs::path &root) {
  auto mac=root/"mac-sdk";
  sdkFixtureFile(mac/"SDKSettings.json",R"({"Version":"28.2","DefaultProperties":{"MACOSX_DEPLOYMENT_TARGET":"14.7"}})");
  sdkFixtureFile(mac/"usr/include/stdio.h"); sdkFixtureFile(mac/"usr/include/c++/v1/vector");
  auto platform=macosPlatform(mac);
  require(str(platform,"version")=="28.2","macOS SDK version was replaced");
  require(str(*platform.getObject("metadata"),"target_triple")=="arm64-apple-macosx14.7","deployment target lost");
  fs::remove(mac/"usr/include/c++/v1/vector"); bool missing=false;
  try { (void)macosPlatform(mac); } catch(const Failure &) { missing=true; }
  require(missing,"incomplete macOS SDK admitted");
  auto programs=root/"Programs",kits=root/"Kits";
  auto sdk=kits/"Windows Kits/10/Include/10.0.99999.0";
  for(auto part:{"ucrt","shared","um","winrt","cppwinrt"}) fs::create_directories(sdk/part);
  sdkFixtureFile(sdk/"um/Windows.h");
  auto msvc=programs/"Microsoft Visual Studio/2022/Enterprise/VC/Tools/MSVC/14.99.12345";
  sdkFixtureFile(msvc/"include/vector"); sdkFixtureFile(msvc/"include/yvals_core.h");
  sdkFixtureFile(kits/"Windows Kits/10/Include/10.0.999999.0-preview/um/Windows.h");
  fs::create_directories(kits/"Windows Kits/10/Include/10.0.999999.0");
  sdkFixtureEnvironment("ProgramFiles",programs); sdkFixtureEnvironment("ProgramFiles(x86)",kits);
  clearEnvironment("CODEGRAPH_WINDOWS_SDK"); clearEnvironment("CODEGRAPH_MSVC_ROOT");
  auto found=discoverWindows(); require(found.size()==2,"Windows declaration roots were merged");
  auto standard=found[0].getAsObject(),target=found[1].getAsObject(); require(standard && target,"invalid Windows targets");
  require(str(*target,"version")=="10.0.99999.0" && str(*standard,"version")=="14.99.12345","installed version metadata lost");
  require(str(*target,"root")==real(sdk).u8string() && str(*standard,"root")==real(msvc/"include").u8string(),"approved roots widened");
  std::cout<<"SDK discovery assertions passed\n"; return 0;
}
