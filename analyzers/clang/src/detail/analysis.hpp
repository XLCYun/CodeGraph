#pragma once

#include "source_document.hpp"

// 分析 module 的私有状态和内部声明；入口仅包含 analyzer.hpp。
#include <llvm/ADT/StringRef.h>
#include <llvm/Support/JSON.h>
#include <chrono>
#include <cstddef>
#include <filesystem>
#include <initializer_list>
#include <map>
#include <set>
#include <stdexcept>
#include <string>
#include <tuple>
#include <utility>
#include <vector>

namespace clang { class Token; }

namespace codegraph::clang_analyzer::detail {

namespace fs = std::filesystem;
namespace j = llvm::json;
inline constexpr const char *Protocol = "codegraph.analyzer/1";
inline constexpr size_t MaxPublicBytes = 128 * 1024 * 1024;
inline constexpr size_t MaxFiles = 100000;

// 受控失败保留稳定协议错误码，禁止以空图掩盖错误。
struct Failure : std::runtime_error {
  std::string code;
  // 保存内部失败码及可读说明，公共入口按原有规则映射。
  Failure(std::string c, std::string m) : runtime_error(m), code(std::move(c)) {}
};
// 保存本侧已批准的目标声明路径，不包含任意编译器参数。
struct Platform {
  std::string id, triple, resourceId; fs::path root, resource; std::vector<fs::path> includes, additionalIncludes; std::vector<std::string> additionalIds;
};
// 原生分析配置；只含白名单参数及可验证来源。
struct Config { std::string id, language; fs::path file, directory; std::vector<std::string> args; size_t platform=0; std::vector<std::string> sourcePaths{"compile_commands.json"}; };
// 单请求状态维护最终并集、来源和资源期限。
struct Request {
  fs::path root; std::set<std::string> selected; std::vector<Platform> platforms; std::vector<Config> configs;
  std::string id, language; bool discovery=false, sourceReview=false, reviewContext=false, includeTests=false; size_t maxResult=MaxPublicBytes, maxDiagnostic=262144;
  std::chrono::steady_clock::time_point deadline=std::chrono::steady_clock::now()+std::chrono::minutes(30);
  // 检查总期限，不被诊断日志延长。
  void checkpoint() const { if (std::chrono::steady_clock::now()>deadline) throw Failure("resource_limit","analysis deadline exceeded"); }
};
// 保存原始源码声明片段；偏移仅在当前快照内定位，不直接进入公开 ID。
struct DeclInfo {
  std::string key,path,name,category,kind,parent,usr,role,params,id,code,signature,source,language,commonPrefix;
  unsigned begin=0,end=0,body=0,nameOffset=0,typeEnd=0,declaratorBegin=0; bool selected=false;
};
// 调用关系首先保留后端绑定证据，最终集合完成后再分类。
struct Edge { std::string owner,target,usr,unknown,semantic="code.calls",externalSymbol,externalName,externalScope; bool operator<(const Edge &o) const { return std::tie(owner,target,usr,unknown,semantic,externalSymbol)<std::tie(o.owner,o.target,o.usr,o.unknown,o.semantic,o.externalSymbol); } };
// 完整单图并集先校验同一源片段的内容和父级，再生成稳定身份。
// 宏使用点在 AST 建立后按真实源码范围归属到最近 owner。
struct SourceMacroUse { std::string path,target; unsigned offset; };
struct Graph {
  Request &request; std::map<std::string,DeclInfo> decls; std::set<Edge> edges;
  std::vector<SourceMacroUse> macroUses;
  std::map<std::string,std::set<std::string>> symbolKeys;
  std::map<std::string,std::string> files; std::map<std::string,std::set<std::string>> coverage;
  std::set<std::string> discoveryCandidates;
  std::map<std::string,SourceDocument> documents;
  std::map<fs::path,std::string> selectedPhysical;
  // 绑定请求对象，所有并集数据仅在单次请求中生存。
  explicit Graph(Request &r) : request(r) {}
};

// 检查协议条件，失败时停止整个请求。
void require(bool ok, const std::string &message, const std::string &code = "invalid_input");
// 严格 JSON、文本与真实路径操作由协议和编译配置共用。
std::string jsonText(const j::Value &value);
std::string str(const j::Object &object, llvm::StringRef key);
void keys(const j::Object &object, std::initializer_list<const char *> names);
j::Value parse(const std::string &text);
std::string read(const fs::path &path);
fs::path real(const fs::path &path);
bool under(const fs::path &path, const fs::path &root);
bool excluded(const fs::path &relative);
fs::path relativePath(const std::string &path);
// 清理分析环境不会执行项目配置或外部工具。
void clearEnvironment(const char *name);
// 原始 token 与归一化文本在提取和图收尾间共享。
std::vector<clang::Token> rawTokens(const std::string &source);
std::string normalize(const std::string &source);
// SDK 发现、批准与 CDB 解释保留原有步骤和策略。
fs::path installationRoot();
j::Object macosPlatform(const fs::path &path);
j::Array discoverWindows();
void discoverPlatforms(Request &request);
void platforms(Request &request, const j::Array &catalog);
void configureProject(Request &request);
// 仅在测试观察环境下发布真实准备阶段；不改变公共协议或生成分析 worker。
void observeDiscovery(const std::string &id, const std::string &milestone="compilation-database-read");
// 公共源码审查入口独立准备文件选集，不执行项目配置或依赖编译数据库。
void prepareReviewProject(Request &request);
std::string sourceFileKey(const std::string &path);
j::Object analyzeReview(Request &request);
// 审查层先整理共享声明范围，再补齐不透明片段；严格语义路径在 finish 中调用。
void prepareDeclarators(Graph &graph);
// 每个 TU 的 AST 和 Clang 对象在此调用返回前销毁。
void collectTranslationUnit(Graph &graph, Config &config);
// 请求内完成全部 TU 后才统一确定身份、引用和覆盖。
j::Object finish(Graph &graph);
j::Object analyze(Request &request);
// 错误终结复用固定身份，不携带部分图或来源证据。
j::Object error(const std::string &id, const Failure &failure);

} // namespace codegraph::clang_analyzer::detail
