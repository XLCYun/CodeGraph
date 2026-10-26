#include "detail/analysis.hpp"
#include <clang/AST/RecursiveASTVisitor.h>
#include <clang/AST/TypeLoc.h>
#include <clang/Frontend/CompilerInstance.h>
#include <clang/Frontend/FrontendActions.h>
#include <clang/Index/USRGeneration.h>
#include <clang/Lex/Lexer.h>
#include <clang/Lex/MacroInfo.h>
#include <clang/Lex/PPCallbacks.h>
#include <clang/Lex/PreprocessorOptions.h>
#include <clang/Tooling/CompilationDatabase.h>
#include <clang/Tooling/Tooling.h>
#include <llvm/Support/VirtualFileSystem.h>
#include <algorithm>
#include <cctype>

using namespace clang;
namespace codegraph::clang_analyzer::detail {

// 文件系统只向 Clang 暴露批准的仓库源码和目标头文件。
class ReadBoundary : public llvm::vfs::ProxyFileSystem {
  Request &request; Platform &platform;
  // 对每次读取重新解析链接来源，禁止仓外头文件和业务依赖缓存。
  bool allowed(const llvm::Twine &name) {
    request.checkpoint(); std::error_code ec; fs::path input=fs::u8path(name.str()); if (input.is_relative()) input=request.root/input;
    auto path=fs::canonical(input,ec); if (ec) return false;
    if (under(path,request.root)) return !excluded(path.lexically_relative(request.root));
    if (under(path,platform.resource)) return true; for (auto &inc:platform.includes) if (under(path,inc)) return true;
    return false;
  }
public:
  // 使用独立真实文件系统，避免 Clang 修改进程全局 cwd。
  ReadBoundary(Request &r, Platform &p) : ProxyFileSystem(llvm::vfs::createPhysicalFileSystem()), request(r), platform(p) {}
  // 禁止对未授权路径探测状态。
  llvm::ErrorOr<llvm::vfs::Status> status(const llvm::Twine &p) override {
    if (!allowed(p)) return std::make_error_code(std::errc::no_such_file_or_directory); return ProxyFileSystem::status(p);
  }
  // 唯一源码打开入口始终执行来源校验。
  llvm::ErrorOr<std::unique_ptr<llvm::vfs::File>> openFileForRead(const llvm::Twine &p) override {
    if (!allowed(p)) return std::make_error_code(std::errc::no_such_file_or_directory); return ProxyFileSystem::openFileForRead(p);
  }
};
// 访问 Clang 的真实 AST，不经过 AST JSON 转储或项目编译命令。
class Collector : public RecursiveASTVisitor<Collector> {
  ASTContext &context; SourceManager &sm; Graph &graph; std::string config;
  std::map<const Decl *,std::string> known; std::vector<std::string> owners;
  // 得到物理文件位置；#line 与宏定义位置不取代真实声明来源。
  std::pair<std::string,unsigned> location(SourceLocation loc) {
    loc=sm.getExpansionLoc(loc); if (loc.isInvalid() || !loc.isFileID()) return {};
    auto file=sm.getFilename(loc); if (file.empty()) return {};
    std::error_code ec; auto p=fs::canonical(fs::u8path(file.str()),ec); if(ec) return {};
    if (!under(p,graph.request.root)) return {};
    if (graph.request.sourceReview) { auto selected=graph.selectedPhysical.find(p); if(selected!=graph.selectedPhysical.end()) return {selected->second,sm.getFileOffset(loc)}; }
    auto rel=p.lexically_relative(graph.request.root).generic_u8string(); return {rel,sm.getFileOffset(loc)};
  }
  // 仅显式承诺的对象成为节点，其余隐式或局部对象内联于 owner。
  std::string category(const Decl *d) {
    if (d->isImplicit() || isa<ParmVarDecl>(d)) return {};
    if(graph.request.sourceReview && d->isInvalidDecl()) {
      if(auto function=dyn_cast<FunctionDecl>(d)) {
        auto begin=location(function->getBeginLoc()),name=location(function->getLocation());
        if(begin==name && !isa<CXXConstructorDecl>(function) && !isa<CXXDestructorDecl>(function) && !isa<CXXConversionDecl>(function)) return {};
      } else if(isa<FieldDecl>(d) || isa<VarDecl>(d)) {
        auto begin=location(d->getBeginLoc()),name=location(d->getLocation()),last=location(d->getEndLoc());
        if(begin.first.empty() || begin.first!=name.first || begin.first!=last.first || begin.second>=name.second) return {};
        auto &source=graph.files.at(begin.first);
        auto prefix=rawTokens(source.substr(begin.second,name.second-begin.second));
        if(std::any_of(prefix.begin(),prefix.end(),[](const Token &token){return token.is(tok::l_paren);})) return {};
        auto end=last.second+Lexer::MeasureTokenLength(sm.getExpansionLoc(d->getEndLoc()),sm,context.getLangOpts());
        while(end<source.size() && isspace(static_cast<unsigned char>(source[end]))) ++end;
        if((last.second>=source.size() || source[last.second]!=';') && (end>=source.size() || source[end]!=';')) return {};
      } else if(!isa<NamespaceDecl>(d) && !isa<RecordDecl>(d)) return {};
    }
    if (auto f=dyn_cast<FunctionDecl>(d)) { if (f->getTemplateSpecializationKind()==TSK_ImplicitInstantiation) return {}; return "callable"; }
    if (auto r=dyn_cast<CXXRecordDecl>(d)) if (r->isInjectedClassName() || (isa<ClassTemplateSpecializationDecl>(r) && cast<ClassTemplateSpecializationDecl>(r)->getSpecializationKind()==TSK_ImplicitInstantiation)) return {};
    if (isa<NamespaceDecl>(d) || isa<NamespaceAliasDecl>(d)) return "container";
    if (isa<StaticAssertDecl>(d) || isa<UsingDecl>(d) || isa<UsingDirectiveDecl>(d) || isa<LinkageSpecDecl>(d)) return "extension";
    if (isa<FileScopeAsmDecl>(d)) return "execution_region";
    if (isa<ConceptDecl>(d)) return "type";
    if (isa<RecordDecl>(d) || isa<EnumDecl>(d) || isa<TypedefNameDecl>(d)) return "type";
    if (auto v=dyn_cast<VarDecl>(d)) { if(v->isLocalVarDecl()) return {}; return "value"; }
    if(isa<FieldDecl>(d) || isa<EnumConstantDecl>(d)) return "value";
    return {};
  }
  // 使用后端 USR 作为绑定证据，绝不直接当作源码节点身份。
  std::string usr(const Decl *d) {
    llvm::SmallString<128> text; if(index::generateUSRForDecl(d,text)) return {}; return text.str().str();
  }
  // 截取同一物理文件中的原始 token 范围，宏展开保持调用处文本。
  std::pair<unsigned,unsigned> range(SourceRange range, const std::string &path) {
    auto begin=location(range.getBegin()), end=location(range.getEnd());
    require(begin.first==path && end.first==path,"non-contiguous declaration source","object_extraction_failed");
    auto loc=sm.getExpansionRange(range.getEnd()).getEnd(); loc=sm.getExpansionLoc(loc);
    unsigned length=Lexer::MeasureTokenLength(loc,sm,context.getLangOpts());
    return {begin.second,end.second+length};
  }
  // 建立源位置内部键，同一位置换类别或父级会在并集阶段冲突。
  std::string keyFor(const Decl *d) {
    auto p=location(d->getLocation()); return p.first.empty()?std::string():p.first+":"+std::to_string(p.second);
  }
  // 添加一个声明，保留原始文本而不使用推导类型打印结果。
  std::string add(Decl *d) {
    auto cat=category(d); if(cat.empty()) return {};
    auto loc=location(d->getLocation()); if(loc.first.empty()) return {};
    require(!excluded(loc.first),"excluded dependency source","object_extraction_failed");
    auto *named=dyn_cast<NamedDecl>(d); std::string name=named?named->getNameAsString():""; if(name.empty()) name="<anonymous>";
    auto sourceRange=d->getSourceRange();
    if(auto f=dyn_cast<FunctionDecl>(d)) if(auto t=f->getDescribedFunctionTemplate()) sourceRange=t->getSourceRange();
    if(auto r=dyn_cast<CXXRecordDecl>(d)) if(auto t=r->getDescribedClassTemplate()) sourceRange=t->getSourceRange();
    auto span=range(sourceRange,loc.first); auto key=keyFor(d);
    auto &source=graph.files[loc.first]; if(source.empty()) source=read(graph.request.root/fs::u8path(loc.first));
    require(span.second<=source.size() && span.first<span.second,"invalid declaration range","object_extraction_failed");
    unsigned end=span.second; if(end<source.size() && source[end]==';') ++end;
    DeclInfo n; n.key=key; n.path=loc.first; n.name=name; n.category=cat; n.kind=d->getDeclKindName(); n.begin=span.first; n.end=end; n.body=end; n.nameOffset=loc.second;
    n.language=context.getLangOpts().CPlusPlus?"cpp":"c";
    if(auto tag=dyn_cast<RecordDecl>(d)) n.kind=tag->isUnion()?"UnionDecl":tag->isStruct()?"StructDecl":"ClassDecl";
    n.selected=graph.request.selected.count(loc.first); n.usr=graph.request.sourceReview?"":usr(d); if(!n.usr.empty()) graph.symbolKeys[n.usr].insert(key); n.parent=owners.empty()?(graph.request.sourceReview?sourceFileKey(n.path):""):owners.back();
    if(auto f=dyn_cast<FunctionDecl>(d)) {
      n.role=f->doesThisDeclarationHaveABody()?"definition":"declaration";
      if(f->doesThisDeclarationHaveABody()) { auto b=location(f->getBody()->getBeginLoc()); require(b.first==n.path,"unsupported function body source","object_extraction_failed"); n.body=b.second; }
      // 源参数类型组成重载槽；参数名不是类型的一部分。
      for(auto p:f->parameters()) {
        auto pr=range(p->getSourceRange(),n.path); std::string raw=source.substr(pr.first,pr.second-pr.first);
        auto pl=location(p->getLocation()); if(!p->getName().empty() && pl.first==n.path && pl.second>=pr.first && pl.second+p->getName().size()<=pr.second) raw.erase(pl.second-pr.first,p->getName().size());
        auto equal=raw.find('='); if(equal!=std::string::npos) raw.resize(equal);
        n.params+=normalize(raw)+";";
      }
      if(f->isVariadic()) n.params+="...";
      if(auto m=dyn_cast<CXXMethodDecl>(f)) { if(m->isConst()) n.params+=" const"; if(m->isVolatile()) n.params+=" volatile"; n.params+=" ref"+std::to_string(m->getRefQualifier()); }
    } else if(auto t=dyn_cast<TagDecl>(d)) { n.role=t->isThisDeclarationADefinition()?"definition":"declaration"; if(t->getBraceRange().getBegin().isValid()) n.body=location(t->getBraceRange().getBegin()).second; }
    if(auto linkage=dyn_cast<LinkageSpecDecl>(d)) {
      auto language=location(linkage->getLocation());
      n.body=language.second+Lexer::MeasureTokenLength(sm.getExpansionLoc(linkage->getLocation()),sm,context.getLangOpts());
    }
    if(isa<NamespaceDecl>(d)) {
      for(auto &token:rawTokens(source.substr(n.begin,n.end-n.begin))) if(token.is(tok::l_brace)) { n.body=n.begin+token.getLocation().getRawEncoding(); break; }
    }
    // 错误恢复可能合成没有源码位置的类型；此时保留原声明，不使用合成 TypeLoc 拆分范围。
    if((isa<VarDecl>(d) || isa<FieldDecl>(d)) && (!graph.request.sourceReview || !d->isInvalidDecl())) if(auto declarator=dyn_cast<DeclaratorDecl>(d)) if(auto info=declarator->getTypeSourceInfo()) {
      TypeLoc type=info->getTypeLoc(); std::vector<unsigned> declaratorStarts;
      // 每层原生 TypeLoc 提供自身范围；MemberPointerTypeLoc 的范围包含完整 C:: 限定名。
      while(!type.getNextTypeLoc().isNull()) {
        auto local=location(type.getLocalSourceRange().getBegin());
        if(local.first==n.path) declaratorStarts.push_back(local.second);
        type=type.getNextTypeLoc();
      }
      auto typeRange=range(type.getSourceRange(),n.path); n.typeEnd=typeRange.second; n.declaratorBegin=n.nameOffset;
      for(auto start:declaratorStarts) if(start>=n.typeEnd && start<n.declaratorBegin) n.declaratorBegin=start;
    }
    n.signature=normalize(source.substr(n.begin,n.body-n.begin));
    n.source=source.substr(n.begin,n.end-n.begin);
    auto found=graph.decls.find(key);
    if(found!=graph.decls.end()) {
      auto &o=found->second;
      if(n.language=="c") o.language="c";
      require(std::tie(o.name,o.category,o.kind,o.parent,o.begin,o.end,o.signature,o.params,o.role)==std::tie(n.name,n.category,n.kind,n.parent,n.begin,n.end,n.signature,n.params,n.role),"incompatible source declaration: "+n.path+" "+n.name,"union_conflict");
    } else graph.decls.emplace(key,std::move(n));
    known[d]=key; return key;
  }
public:
  // 绑定当前 TU 的 AST 与对应配置来源。
  Collector(ASTContext &c, Graph &g, std::string id):context(c),sm(c.getSourceManager()),graph(g),config(std::move(id)) {}
  // 只遍历显式源模板，不把编译器实例化产物复制成节点。
  bool shouldVisitTemplateInstantiations() const { return false; }
  // 自定义声明栈，精确保存词法包含关系及调用 owner。
  bool TraverseDecl(Decl *d) {
    if(!d) return true; graph.request.checkpoint();
    if(d->isImplicit()) return true;
    if(auto f=dyn_cast<FunctionDecl>(d)) if(f->getTemplateSpecializationKind()==TSK_ImplicitInstantiation) return true;
    if(auto r=dyn_cast<ClassTemplateSpecializationDecl>(d)) if(r->getSpecializationKind()==TSK_ImplicitInstantiation) return true;
    if((graph.request.sourceReview || graph.request.reviewContext) && d->getLocation().isMacroID()) return true;
    std::string key;
    try { key=add(d); } catch(const Failure &failure) {
      if(!graph.request.sourceReview || (failure.code!="object_extraction_failed" && failure.code!="union_conflict")) throw;
      return true;
    }
    if(!key.empty()) owners.push_back(key);
    bool ok=RecursiveASTVisitor<Collector>::TraverseDecl(d); if(!key.empty()) owners.pop_back(); return ok;
  }
  // 直接调用依据 Clang 绑定；函数指针和依赖调用保留未知观测。
  bool VisitCallExpr(CallExpr *call) {
    if(graph.request.reviewContext && call->getExprLoc().isMacroID()) return true;
    if(owners.empty()) return true; Edge e; e.owner=owners.back();
    if(graph.request.sourceReview) {
      auto loc=location(call->getExprLoc());
      if(loc.first.empty()) return true;
      try { auto span=range(call->getCallee()->getSourceRange(),loc.first); e.unknown=normalize(graph.files.at(loc.first).substr(span.first,span.second-span.first)); }
      catch(const Failure &) { e.unknown="<unresolved-call>"; }
      graph.edges.insert(std::move(e)); return true;
    }
    if(auto callee=call->getDirectCallee()) { e.target=keyFor(callee); e.usr=usr(callee); if(e.target.empty() || graph.request.reviewContext) e.unknown=callee->getQualifiedNameAsString(); }
    else { auto p=location(call->getExprLoc()); if(!p.first.empty()) { auto r=range(call->getCallee()->getSourceRange(),p.first); auto &source=graph.files[p.first]; if(source.empty()) source=read(graph.request.root/fs::u8path(p.first)); e.unknown=normalize(source.substr(r.first,r.second-r.first)); } }
    if(e.target.empty() && e.usr.empty() && e.unknown.empty()) e.unknown="<indirect-call>";
    graph.edges.insert(std::move(e)); return true;
  }
  // 构造表达式同样按真实构造函数绑定，不按类名猜测。
  bool VisitCXXConstructExpr(CXXConstructExpr *call) {
    if(graph.request.reviewContext && call->getExprLoc().isMacroID()) return true;
    if(owners.empty() || call->getConstructor()->isImplicit()) return true;
    if(graph.request.sourceReview) { graph.edges.insert({owners.back(),{},{},call->getConstructor()->getNameAsString()}); return true; }
    graph.edges.insert({owners.back(),keyFor(call->getConstructor()),usr(call->getConstructor()),{}}); return true;
  }
  // 恢复 AST 中可观测但尚无目标的名字保留为未解析引用，不按名字猜绑定。
  bool VisitUnresolvedLookupExpr(UnresolvedLookupExpr *expression) {
    if(graph.request.sourceReview && !owners.empty()) graph.edges.insert({owners.back(),{},{},expression->getName().getAsString(),"code.references"});
    return true;
  }
};
// 宏定义本身是显式源对象，避免仅修改宏正文却得到相同图。
class Macros : public PPCallbacks {
  SourceManager &sm; const LangOptions &options; Graph &graph;
  // 取得真实仓内位置，平台宏只作为语义上下文。
  std::pair<std::string,unsigned> where(SourceLocation loc) {
    loc=sm.getExpansionLoc(loc); if(loc.isInvalid() || !loc.isFileID()) return {};
    auto file=sm.getFilename(loc); if(file.empty()) return {};
    std::error_code ec; auto path=fs::canonical(fs::u8path(file.str()),ec); if(ec || !under(path,graph.request.root)) return {};
    return {path.lexically_relative(graph.request.root).generic_u8string(),sm.getFileOffset(loc)};
  }
public:
  // 使用当前 TU 的原生预处理器记录定义与展开。
  Macros(SourceManager &manager,const LangOptions &lang,Graph &g):sm(manager),options(lang),graph(g) {}
  // 原始定义正文参与内容比较，包括未生成独立 AST 节点的数字宏。
  void MacroDefined(const Token &name,const MacroDirective *directive) override {
    auto info=directive->getMacroInfo(); auto pos=where(name.getLocation()); if(pos.first.empty()) return;
    auto end=where(info->getDefinitionEndLoc()); require(end.first==pos.first,"nonlocal macro definition","object_extraction_failed");
    auto &source=graph.files[pos.first]; if(source.empty()) source=read(graph.request.root/fs::u8path(pos.first));
    unsigned begin=pos.second; while(begin>0 && source[begin-1]!='\n') --begin;
    unsigned finish=end.second+Lexer::MeasureTokenLength(sm.getExpansionLoc(info->getDefinitionEndLoc()),sm,options);
    DeclInfo n; n.key=pos.first+":"+std::to_string(pos.second); n.path=pos.first; n.begin=begin; n.end=finish; n.body=finish;
    n.name=name.getIdentifierInfo()->getName().str(); n.category="extension"; n.kind="MacroDefinition"; n.role="definition";
    n.language=options.CPlusPlus?"cpp":"c"; n.selected=graph.request.selected.count(pos.first); n.source=source.substr(begin,finish-begin); n.signature="#define "+n.name;
    auto found=graph.decls.find(n.key); if(found==graph.decls.end()) graph.decls.emplace(n.key,std::move(n)); else if(n.language=="c") found->second.language="c";
  }
  // 只记录 Clang 确认展开的宏，不按同名 token 猜引用。
  void MacroExpands(const Token &name,const MacroDefinition &definition,SourceRange,const MacroArgs *) override {
    auto source=where(name.getLocation()); auto info=definition.getMacroInfo(); if(source.first.empty() || !info) return;
    auto target=where(info->getDefinitionLoc()); if(target.first.empty()) return;
    graph.macroUses.push_back({source.first,target.first+":"+std::to_string(target.second),source.second});
  }
};

// 仅记录真正进入词法分析的仓内文件，探测、跳过和系统头不能扩大选集。
class Participation : public PPCallbacks {
  SourceManager &sm; Graph &graph;
  std::map<unsigned,std::pair<FileID,std::string>> entered;
public:
  // 与当前 TU 的 SourceManager 共用生命周期，最终分类在预处理完成后检查。
  Participation(SourceManager &manager,Graph &g):sm(manager),graph(g) {}
  // #line 不改变物理参与文件，只有真正进入词法分析的文件触发发现。
  void LexedFileChanged(FileID fid,LexedFileChangeReason reason,SrcMgr::CharacteristicKind type,
                        FileID,SourceLocation) override {
    graph.request.checkpoint();
    if(graph.request.sourceReview || !graph.request.discovery || reason!=LexedFileChangeReason::EnterFile || type!=SrcMgr::C_User) return;
    auto entry=sm.getFileEntryRefForID(fid); if(!entry) return;
    auto path=real(fs::u8path(entry->getName().str()));
    if(!under(path,graph.request.root)) return;
    auto relative=path.lexically_relative(graph.request.root);
    if(excluded(relative)) return;
    auto name=relative.generic_u8string(); relativePath(name);
    require(fs::is_regular_file(path),"participating source must be a regular repository file");
    entered.emplace(fid.getHashValue(),std::make_pair(fid,name));
    graph.discoveryCandidates.insert(name);
    require(graph.discoveryCandidates.size()<=MaxFiles,"Clang discovery exceeds file limit","resource_limit");
  }
  // pragma system_header 发生在进入文件之后，必须以最终的原生分类排除整份头文件。
  void complete(const std::string &config) {
    for(auto &entry:entered) {
      graph.request.checkpoint(); auto &file=entry.second;
      if(sm.isInSystemHeader(sm.getLocForEndOfFile(file.first))) continue;
      graph.request.selected.insert(file.second);
      require(graph.request.selected.size()<=MaxFiles,"Clang discovery exceeds file limit","resource_limit");
      graph.coverage[file.second].insert(config);
    }
  }
};

// 只在 AST 完整解析后执行声明提取，任何诊断错误都使整次请求失败。
class Consumer : public ASTConsumer {
  Graph &graph; std::string config; Participation &participation;
public:
  // 绑定本次配置。
  Consumer(Graph &g,std::string id,Participation &files):graph(g),config(std::move(id)),participation(files) {}
  // 消费原生 AST 并确认文件覆盖。
  void HandleTranslationUnit(ASTContext &context) override {
    if(!graph.request.sourceReview) require(!context.getDiagnostics().hasErrorOccurred(),"Clang cannot extract complete objects","object_extraction_failed");
    Collector c(context,graph,config); c.TraverseDecl(context.getTranslationUnitDecl());
    if(graph.request.sourceReview) return;
    if(graph.request.discovery) { participation.complete(config); return; }
    auto &sm=context.getSourceManager();
    for(auto &file:graph.request.selected) {
      auto entry=sm.getFileManager().getOptionalFileRef((graph.request.root/fs::u8path(file)).u8string());
      if(entry && sm.translateFile(*entry).isValid()) graph.coverage[file].insert(config);
    }
  }
};
// 纯语法前端 action 不执行代码生成、链接、插件或项目逻辑。
class Action : public ASTFrontendAction {
  Graph &graph; std::string config;
public:
  // 绑定单图并集与配置。
  Action(Graph &g,std::string id):graph(g),config(std::move(id)) {}
  // 源码结构阶段不进入 include，恢复 AST 只作为局部结构证据。
  bool BeginInvocation(CompilerInstance &instance) override {
    if(graph.request.sourceReview) {
      instance.getPreprocessorOpts().SingleFileParseMode=true;
      instance.getLangOpts().RecoveryAST=true;
      instance.getLangOpts().RecoveryASTType=true;
    }
    return true;
  }
  // 创建 AST consumer，不允许项目自定义 action。
  std::unique_ptr<ASTConsumer> CreateASTConsumer(CompilerInstance &instance,llvm::StringRef) override {
    auto participation=std::make_unique<Participation>(instance.getSourceManager(),graph);
    auto &files=*participation;
    instance.getPreprocessor().addPPCallbacks(std::move(participation));
    if(!graph.request.sourceReview && !graph.request.reviewContext) instance.getPreprocessor().addPPCallbacks(std::make_unique<Macros>(instance.getSourceManager(),instance.getLangOpts(),graph));
    return std::make_unique<Consumer>(graph,config,files);
  }
};
// ClangTool 为每个受控 TU 创建单一前端 action。
class Factory : public tooling::FrontendActionFactory {
  Graph &graph; std::string config;
public:
  // 初始化只读 action 工厂。
  Factory(Graph &g,std::string id):graph(g),config(std::move(id)) {}
  // 返回真实 LibTooling action。
  std::unique_ptr<FrontendAction> create() override { return std::make_unique<Action>(graph,config); }
};
// 有界收集诊断，避免 Clang 直接污染 stdout/stderr 或隐藏错误。
class Diagnostics : public DiagnosticConsumer {
  size_t limit,used=0; bool sourceOnly;
public:
  std::string firstError;
  // 固定整次 TU 的最大诊断字节数。
  explicit Diagnostics(size_t max,bool review=false):limit(max),sourceOnly(review) {}
  // 格式化诊断只用于错误信息，不改变成功标准。
  void HandleDiagnostic(DiagnosticsEngine::Level level,const Diagnostic &info) override {
    DiagnosticConsumer::HandleDiagnostic(level,info);
    if(sourceOnly) return;
    llvm::SmallString<256> text; info.FormatDiagnostic(text); used+=text.size();
    require(used<=limit,"diagnostic byte limit exceeded","resource_limit");
    if(level>=DiagnosticsEngine::Error && firstError.empty()) firstError=text.str().str();
  }
};
// 在同一请求中运行一个受控 TU，所有原生对象只在本调用内生存。
void collectTranslationUnit(Graph &graph, Config &c) {
  auto &r = graph.request;
    r.checkpoint(); tooling::FixedCompilationDatabase db(c.directory.u8string(),c.args);
    llvm::IntrusiveRefCntPtr<ReadBoundary> filesystem(new ReadBoundary(r,r.platforms[c.platform]));
    // ClangTool 借用路径与内容的 StringRef，路径必须存活到整个原生调用结束。
    std::vector<std::string> mappedPaths;
    if(r.sourceReview) mappedPaths={c.file.u8string(),real(c.file).u8string()};
    tooling::ClangTool tool(db,{c.file.u8string()},std::make_shared<PCHContainerOperations>(),filesystem);
    if(r.sourceReview) {
      // 单文件视图不进入 include，只映射本次源码，避免每个 TU 重复装入整个选集。
      auto &view=graph.documents.at(c.file.lexically_relative(r.root).generic_u8string()).view;
      for(auto &path:mappedPaths) tool.mapVirtualFile(path,view);
    }
    tool.setPrintErrorMessage(false); Diagnostics diagnostics(r.maxDiagnostic,r.sourceReview); tool.setDiagnosticConsumer(&diagnostics);
    Factory factory(graph,c.id); int status=0;
    try { status=tool.run(&factory); } catch(const Failure &f) { if(f.code=="object_extraction_failed" && !diagnostics.firstError.empty()) throw Failure(f.code,diagnostics.firstError); throw; }
    if(!r.sourceReview) require(status==0 && diagnostics.getNumErrors()==0,"Clang cannot extract complete objects: "+diagnostics.firstError,"object_extraction_failed");

}

} // namespace codegraph::clang_analyzer::detail
