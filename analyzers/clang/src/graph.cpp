#include "detail/analysis.hpp"
#include <clang/Lex/Token.h>
#include <algorithm>
#include <cctype>
namespace codegraph::clang_analyzer::detail {

// 对身份字段进行无歧义转义，保持相对路径和源声明槽位可复查。
static std::string escape(const std::string &s) {
  static const char *hex="0123456789ABCDEF"; std::string out;
  for(unsigned char c:s) { if(isalnum(c) || c=='_' || c=='-' || c=='.' || c=='/') out+=c; else { out+='%'; out+=hex[c>>4]; out+=hex[c&15]; } } return out;
}
// 按 AST 基础类型范围拆分共用声明前缀，保持每个 declarator 的初始化内容独立。
void prepareDeclarators(Graph &graph) {
  std::map<std::tuple<std::string,std::string,unsigned>,std::vector<DeclInfo *>> groups;
  for(auto &entry:graph.decls) { auto &n=entry.second; if(n.typeEnd) groups[{n.path,n.parent,n.begin}].push_back(&n); }
  for(auto &group:groups) {
    auto &items=group.second; if(items.size()<2) continue;
    std::sort(items.begin(),items.end(),[](auto a,auto b){return a->nameOffset<b->nameOffset;});
    auto &first=*items.front(); auto &source=graph.files.at(first.path);
    require(first.typeEnd<=first.nameOffset,"unsupported shared declarator type range","object_extraction_failed");
    require(first.declaratorBegin>=first.typeEnd && first.declaratorBegin<=first.nameOffset,"invalid native declarator range","object_extraction_failed");
    std::string prefix=source.substr(first.begin,first.declaratorBegin-first.begin);
    unsigned previousEnd=first.end;
    for(size_t i=1;i<items.size();++i) {
      auto &n=*items[i]; require(previousEnd<=n.nameOffset,"overlapping declarator initializer","object_extraction_failed");
      auto tokens=rawTokens(source.substr(previousEnd,n.nameOffset-previousEnd));
      require(!tokens.empty() && tokens.front().is(clang::tok::comma),"missing declarator separator","object_extraction_failed");
      unsigned begin=tokens.size()>1?previousEnd+tokens[1].getLocation().getRawEncoding():n.nameOffset;
      n.begin=begin; n.commonPrefix=prefix;
      n.signature=normalize(prefix+source.substr(begin,n.body-begin));
      previousEnd=n.end;
    }
  }
}
// 构造完成后的单图 DTO；任何未覆盖文件或不兼容归并都硬失败。
j::Object finish(Graph &g) {
  if(g.request.discovery) for(auto &entry:g.decls) entry.second.selected=g.request.selected.count(entry.second.path);
  if(!g.request.sourceReview) prepareDeclarators(g);
  auto &r=g.request; std::map<std::string,std::vector<DeclInfo *>> slots;
  for(auto &use:g.macroUses) {
    DeclInfo *owner=nullptr;
    for(auto &entry:g.decls) { auto &n=entry.second; if(n.path==use.path && n.kind!="MacroDefinition" && n.begin<=use.offset && use.offset<n.end && (!owner || n.end-n.begin<owner->end-owner->begin)) owner=&n; }
    if(owner && g.decls.count(use.target)) g.edges.insert({owner->key,use.target,{}, {},"code.references"});
  }
  size_t assigned=0;
  while(assigned<g.decls.size()) {
    slots.clear();
    for(auto &entry:g.decls) {
      auto &n=entry.second; if(!n.id.empty()) continue;
      if(!n.parent.empty() && g.decls.at(n.parent).id.empty()) continue;
      auto owner=n.parent.empty()?n.path:g.decls.at(n.parent).id;
      auto slot=owner+"#"+n.name+"("+n.params+")#"+n.role; slots[slot].push_back(&n);
    }
    require(!slots.empty(),"cyclic declaration containment","union_conflict");
    for(auto &slot:slots) { auto &items=slot.second; std::sort(items.begin(),items.end(),[](auto a,auto b){return a->begin<b->begin;}); for(size_t i=0;i<items.size();++i) { items[i]->id=items[i]->language+":"+escape(slot.first)+"#"+std::to_string(i); ++assigned; } }
  }
  std::map<std::string,std::vector<DeclInfo *>> children; std::map<std::string,std::vector<DeclInfo *>> symbols;
  for(auto &entry:g.decls) { auto &n=entry.second; if(!n.parent.empty()) children[n.parent].push_back(&n);  }
  for(auto &symbol:g.symbolKeys) for(auto &key:symbol.second) symbols[symbol.first].push_back(&g.decls.at(key));
  j::Array nodes,origins,roots,externals; std::map<std::string,j::Object> externalMap;
  for(auto &entry:g.decls) {
    r.checkpoint(); auto &n=entry.second; if(!n.selected) continue; j::Array contains,refs; std::set<std::string> refKeys;
    auto raw=g.files[n.path].substr(n.begin,n.end-n.begin); auto child=children[n.key]; std::sort(child.begin(),child.end(),[](auto a,auto b){return a->begin>b->begin;}); unsigned boundary=n.end;
    for(auto c:child) {
      require(c->path==n.path && c->begin>=n.begin && c->end<=boundary,"overlapping/nonlocal child declaration","object_extraction_failed");
      require(c->begin>n.begin || c->end<n.end || n.kind=="SourceFile","ambiguous equal-span declarations","object_extraction_failed"); boundary=c->begin;
      raw.replace(c->begin-n.begin,c->end-c->begin," __narrant_child_"+escape(c->id)+" "); if(c->selected) contains.push_back(c->id);
    }
    n.code=normalize(n.commonPrefix+raw);
    if(n.parent.empty() || !g.decls.at(n.parent).selected) roots.push_back(n.id);
    for(auto &edge:g.edges) {
      if(edge.owner!=n.key) continue; j::Object target; DeclInfo *destination=nullptr;
      auto exact=g.decls.find(edge.target); if(exact!=g.decls.end()) destination=&exact->second;
      auto sym=symbols.find(edge.usr);
      if(!edge.usr.empty() && sym!=symbols.end()) {
        std::vector<DeclInfo *> definitions; for(auto d:sym->second) if(d->role=="definition") definitions.push_back(d);
        if(definitions.size()==1) destination=definitions.front();
      }
      if(!edge.externalSymbol.empty()) { target=j::Object{{"kind","external"},{"symbol",edge.externalSymbol}}; externalMap.try_emplace(edge.externalSymbol,j::Object{{"symbol",edge.externalSymbol},{"name",edge.externalName},{"scope",edge.externalScope}}); }
      else if(destination && destination->selected) target=j::Object{{"kind","node"},{"id",destination->id}};
      else if(destination) { target=j::Object{{"kind","external"},{"symbol",destination->id}}; externalMap.try_emplace(destination->id,j::Object{{"symbol",destination->id},{"name",destination->name},{"scope","repository"}}); }
      else if(!edge.usr.empty() && edge.target.empty()) {
        auto symbol=n.language+":dependency:"+escape(edge.usr); target=j::Object{{"kind","external"},{"symbol",symbol}};
        externalMap.try_emplace(symbol,j::Object{{"symbol",symbol},{"name",edge.unknown.empty()?edge.usr:edge.unknown},{"scope","dependency"}});
      } else target=j::Object{{"kind","unresolved"},{"name",edge.unknown.empty()?"<unmapped-call>":edge.unknown}};
      j::Object ref{{"semantic",edge.semantic},{"target",std::move(target)}}; if(refKeys.insert(jsonText(j::Value(j::Object(ref)))).second) refs.push_back(std::move(ref));
    }
    // 容器签名只包含声明头，独立子对象正文不能污染父节点。
    auto signature=n.signature;
    if(r.sourceReview && n.kind!="SourceFile") {
      auto header=g.files.at(n.path).substr(n.begin,n.body-n.begin);
      for(auto item:child) if(item->begin>=n.begin && item->end<=n.body)
        header.replace(item->begin-n.begin,item->end-item->begin," __narrant_child_"+escape(item->id)+" ");
      signature=normalize(n.commonPrefix+header);
    }
    j::Object extensions;
    nodes.push_back(j::Object{{"category",n.category},{"id",n.id},{"language",n.language},{"native_kind",n.kind},{"name",n.name},{"signature",signature},{"code",n.code},{"source",n.source},{"contains",std::move(contains)},{"references",std::move(refs)},{"extensions",std::move(extensions)}});
    origins.push_back(j::Object{{"node_id",n.id},{"selected_paths",j::Array{n.path}}});
  }
  for(auto &entry:externalMap) externals.push_back(std::move(entry.second));
  j::Array configs,files;
  for(auto &c:r.configs) { auto &p=r.platforms[c.platform]; j::Array ids{p.id,p.resourceId},sources; for(auto &id:p.additionalIds) ids.push_back(id); for(auto &path:c.sourcePaths) sources.push_back(path); configs.push_back(j::Object{{"id",c.id},{"source_paths",std::move(sources)},{"platform_ids",std::move(ids)}}); }
  for(auto &file:r.selected) { auto &coverage=g.coverage[file]; require(!coverage.empty(),"selected file has no complete TU context: "+file,"object_extraction_failed"); j::Array ids; for(auto &id:coverage) ids.push_back(id); files.push_back(j::Object{{"path",file},{"disposition","analyzed"},{"configuration_ids",std::move(ids)}}); }
  return j::Object{{"protocol",Protocol},{"engine_version","0.1.1"},{"pack_id","c-cpp"},{"type","completed"},{"id",r.id},{"outcome","ok"},{"graph",j::Object{{"root_contains",std::move(roots)},{"nodes",std::move(nodes)},{"external_symbols",std::move(externals)}}},{"evidence",j::Object{{"configurations",std::move(configs)},{"files",std::move(files)},{"origins",std::move(origins)}}}};
}

} // namespace codegraph::clang_analyzer::detail
