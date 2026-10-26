#include "detail/analysis.hpp"
#include <clang/Lex/Token.h>
#include <algorithm>
#include <cctype>

namespace codegraph::clang_analyzer::detail {
namespace {

// 目录发现只识别源码后缀；显式选集仍以调用方给出的文件为准。
bool sourceFile(const fs::path &path) {
  static const std::set<std::string> extensions{".c", ".C", ".cc", ".cpp", ".cxx", ".h", ".H", ".hh", ".hpp", ".hxx"};
  return extensions.count(path.extension().u8string());
}

// 自动发现遵守测试排除，显式选集不因该选项被悄悄缩小。
bool testFile(const fs::path &path) {
  for (const auto &part : path) if (part == "test" || part == "tests" || part == "__tests__") return true;
  auto stem = path.stem().u8string();
  return stem.size() >= 5 && stem.substr(stem.size() - 5) == "_test";
}

// 源码发现不把构建产物当作用户源码，显式选集仍由输入边界决定。
bool buildFile(const fs::path &path) {
  for(const auto &part:path) if(part=="build" || part=="target" || part==".tools" || part==".cache" || part==".superpowers") return true;
  return false;
}

// 不透明片段不作为另一个不透明片段的父级，条件块内部保持未解释状态。
bool opaque(const DeclInfo &node) {
  return node.kind == "PreprocessorDirective" || node.kind == "ConditionalBlock" || node.kind == "OpaqueSourceFragment";
}

// 片段挂在能够完整包含它的最近源码对象下。
std::string ownerFor(const Graph &graph, const std::string &path, unsigned begin, unsigned end) {
  auto result = sourceFileKey(path);
  unsigned width = graph.files.at(path).size() + 1;
  for (auto &entry : graph.decls) {
    auto &node = entry.second;
    if (node.path == path && !opaque(node) && node.begin <= begin && node.end >= end && node.end - node.begin < width) {
      result = node.key;
      width = node.end - node.begin;
    }
  }
  return result;
}

// 无法确认的恢复声明不应造成重叠对象；整段原文稍后由缺口扫描接管。
void removeOverlaps(Graph &graph) {
  std::map<std::string, std::vector<DeclInfo *>> groups;
  for (auto &entry : graph.decls) if (!entry.second.parent.empty()) groups[entry.second.parent].push_back(&entry.second);
  std::set<std::string> removed;
  for (auto &group : groups) {
    auto &items = group.second;
    std::sort(items.begin(), items.end(), [](auto left, auto right) { return left->begin < right->begin; });
    for (size_t i = 0; i < items.size(); ++i) for (size_t j = i + 1; j < items.size() && items[j]->begin < items[i]->end; ++j) {
      removed.insert(items[i]->key);
      removed.insert(items[j]->key);
    }
  }
  bool changed = true;
  while (changed) {
    changed = false;
    for (auto &entry : graph.decls) if (removed.count(entry.second.parent)) changed |= removed.insert(entry.first).second;
  }
  for (auto &key : removed) graph.decls.erase(key);
  for (auto it = graph.edges.begin(); it != graph.edges.end();) {
    if (removed.count(it->owner)) it = graph.edges.erase(it); else ++it;
  }
}

// 创建原文节点；规范化实现由统一图收尾计算，因此不会只更新展示用 source。
void addRegion(Graph &graph, const std::string &path, const SourceRegion &region, const std::string &owner) {
  DeclInfo node;
  node.key = path + ":opaque:" + std::to_string(region.begin);
  node.path = path; node.begin = region.begin; node.end = region.end; node.body = region.end;
  node.nameOffset = region.begin; node.name = region.name; node.kind = region.kind;
  if(region.kind=="PreprocessorDirective" && region.name.rfind("#define ",0)==0) node.name=region.name.substr(8);
  node.parent = owner; node.category = "extension"; node.role = "opaque"; node.selected = true;
  node.language = graph.decls.at(sourceFileKey(path)).language;
  node.signature = region.name;
  node.source = graph.files.at(path).substr(region.begin, region.end - region.begin);
  graph.decls.emplace(node.key, std::move(node));
}

// 类和命名空间仅扫描实现区域，声明头与访问标签仍由原对象自身保留。
std::pair<unsigned, unsigned> bodyRange(const Graph &graph, const DeclInfo &owner) {
  if (owner.kind == "SourceFile") return {owner.begin, owner.end};
  if ((owner.category != "container" && owner.category != "type") || owner.body >= owner.end) return {0, 0};
  auto &source = graph.files.at(owner.path);
  auto tokens = rawTokens(source.substr(owner.body, owner.end - owner.body));
  for (auto it = tokens.rbegin(); it != tokens.rend(); ++it) if (it->is(clang::tok::r_brace)) return {owner.body + 1, owner.body + it->getLocation().getRawEncoding()};
  return {0, 0};
}

// 补齐未被原生声明覆盖的非空代码，注释、分号和访问标签留在父对象原文中。
void fillGaps(Graph &graph) {
  std::vector<std::string> owners;
  for (auto &entry : graph.decls) if (!opaque(entry.second)) owners.push_back(entry.first);
  for (auto &key : owners) {
    auto &owner = graph.decls.at(key);
    auto range = bodyRange(graph, owner);
    if (range.first >= range.second) continue;
    std::vector<std::pair<unsigned, unsigned>> children;
    for (auto &entry : graph.decls) if (entry.second.parent == key) children.push_back({entry.second.begin, entry.second.end});
    std::sort(children.begin(), children.end());
    unsigned position = range.first;
    auto append = [&](unsigned end) {
      if (position >= end) return;
      auto code = normalize(graph.files.at(owner.path).substr(position, end - position));
      if (code.empty() || code == "public :" || code == "private :" || code == "protected :" || code.find_first_not_of(" ;") == std::string::npos) return;
      addRegion(graph, owner.path, {position, end, "OpaqueSourceFragment", "<unparsed>"}, key);
    };
    for (auto &child : children) {
      if (child.second <= range.first || child.first >= range.second) continue;
      append(std::min(child.first, range.second));
      position = std::max(position, child.second);
    }
    append(range.second);
  }
}

// 可用的严格上下文仅增强已存在源码对象的关系；宏块内部不进入语义投影。
void enrich(Graph &graph) {
  auto &request = graph.request;
  if (!fs::is_regular_file(request.root / "compile_commands.json") || fs::file_size(request.root / "compile_commands.json") > 1024 * 1024) return;
  Request semantic = request;
  semantic.sourceReview = false; semantic.reviewContext = true; semantic.discovery = false;
  semantic.platforms.clear(); semantic.configs.clear();
  try {
    discoverPlatforms(semantic); configureProject(semantic);
    Graph context(semantic);
    for (auto &config : semantic.configs) collectTranslationUnit(context, config);
    finish(context);
    for (auto &entry : context.decls) {
      auto existing = graph.decls.find(entry.first);
      if (existing == graph.decls.end() || opaque(existing->second) || existing->second.kind == "SourceFile") continue;
      auto &owner = existing->second;
      bool containsDirective = false;
      for (auto &region : graph.documents.at(owner.path).regions) containsDirective |= region.begin < owner.end && region.end > owner.begin;
      if (containsDirective) continue;
      owner.usr=entry.second.usr;
      if(!owner.usr.empty()) graph.symbolKeys[owner.usr].insert(owner.key);
      for (auto it = graph.edges.begin(); it != graph.edges.end();) {
        if (it->owner == owner.key) it = graph.edges.erase(it); else ++it;
      }
      for (auto edge : context.edges) if (edge.owner == owner.key) {
        // 先在完整语义上下文中解析唯一源码定义，再按选集区分图内与图外目标。
        auto symbol=context.symbolKeys.find(edge.usr);
        if(!edge.usr.empty() && symbol!=context.symbolKeys.end()) {
          std::vector<std::string> definitions;
          for(auto &key:symbol->second) if(context.decls.at(key).role=="definition") definitions.push_back(key);
          if(definitions.size()==1) edge.target=definitions.front();
        }
        auto target = context.decls.find(edge.target);
        if (target != context.decls.end() && !graph.decls.count(edge.target)) {
          if (request.selected.count(target->second.path)) {
            edge.unknown=target->second.name; edge.target.clear(); edge.usr.clear();
            graph.edges.insert(std::move(edge)); continue;
          }
          edge.externalSymbol = target->second.id; edge.externalName = target->second.name; edge.externalScope = "repository";
          edge.target.clear(); edge.usr.clear();
        } else if (!edge.usr.empty() && edge.target.empty()) {
          edge.externalSymbol = "cpp:dependency:" + edge.usr; edge.externalName = edge.unknown.empty() ? edge.usr : edge.unknown;
          edge.externalScope = "dependency"; edge.usr.clear();
        }
        graph.edges.insert(std::move(edge));
      }
    }
  } catch (const Failure &failure) {
    if (failure.code == "internal_failure" || failure.code == "cleanup_failed") throw;
    request.checkpoint();
    // 缺失或无法解释的编译上下文不影响已经建立的源码覆盖。
  }
}

} // namespace

// 文件节点身份不依赖当前目录的绝对位置或源码偏移。
std::string sourceFileKey(const std::string &path) { return path + ":file"; }

// 在编译数据库之外确定源文件全集；不查找或执行项目构建工具。
void prepareReviewProject(Request &request) {
  if (request.discovery) {
    for (fs::recursive_directory_iterator it(request.root), end; it != end; ++it) {
      request.checkpoint(); auto relative = it->path().lexically_relative(request.root);
      if (excluded(relative) || buildFile(relative) || (!request.includeTests && testFile(relative))) { if (it->is_directory()) it.disable_recursion_pending(); continue; }
      if (!it->is_regular_file() || !sourceFile(relative)) continue;
      auto physical = real(it->path());
      if (!under(physical, request.root) || excluded(physical.lexically_relative(request.root))) continue;
      request.selected.insert(physical.lexically_relative(request.root).generic_u8string());
      require(request.selected.size() <= MaxFiles, "source discovery exceeds file limit", "resource_limit");
    }
  }
  if (request.selected.empty()) return;
  Platform platform;
  platform.id = "source-review"; platform.resourceId = "clang-resource";
  platform.root = request.root; platform.resource = installationRoot() / "runtime/platform/clang-resource";
  request.platforms.push_back(std::move(platform));
  for (auto &file : request.selected) {
    Config config; config.id = "source-" + std::to_string(request.configs.size());
    config.file = request.root / fs::u8path(file); config.directory = request.root;
    config.language = fs::u8path(file).extension() == ".c" ? "c" : "cpp";
    config.args = {"-x", config.language == "c" ? "c" : "c++", config.language == "c" ? "-std=c23" : "-std=c++23", "-fsyntax-only", "-nostdinc", "-nostdinc++", "-fno-modules", "-fno-implicit-modules", "-ferror-limit=0"};
    config.sourcePaths = {file}; request.configs.push_back(std::move(config));
  }
}

// 先保存所有选中文本，再进行可恢复的原生结构提取与可选语义增强。
j::Object analyzeReview(Request &request) {
  Graph graph(request);
  for (auto &config : request.configs) {
    auto file = config.file.lexically_relative(request.root).generic_u8string();
    auto source = read(config.file);
    require(j::isUTF8(source), "source must be UTF-8", "io_failure");
    graph.files[file] = source; graph.documents[file] = sourceDocument(source);
    graph.selectedPhysical[real(config.file)] = file;
    DeclInfo node;
    node.key = sourceFileKey(file); node.path = file; node.name = file; node.category = "container";
    node.kind = "SourceFile"; node.role = "file"; node.language = config.language; node.selected = true;
    node.end = source.size(); node.signature = file; node.source = source;
    graph.decls.emplace(node.key, std::move(node));
    graph.coverage[file].insert(config.id);
  }
  if(request.discovery) observeDiscovery(request.id,"source-selection-prepared");
  for (auto &config : request.configs) collectTranslationUnit(graph, config);
  try { prepareDeclarators(graph); } catch (const Failure &failure) {
    if (failure.code != "object_extraction_failed") throw;
  }
  removeOverlaps(graph);
  for (auto &document : graph.documents) for (auto &region : document.second.regions) {
    addRegion(graph, document.first, region, ownerFor(graph, document.first, region.begin, region.end));
  }
  fillGaps(graph); enrich(graph);
  return finish(graph);
}

} // namespace codegraph::clang_analyzer::detail
