#include "detail/analysis.hpp"
#include "detail/source_document.hpp"
#include <clang/Lex/Token.h>
#include <algorithm>

namespace codegraph::clang_analyzer::detail {
namespace {

// 保存一个原始逻辑指令及其 token 边界，续行仍属于当前指令。
struct Directive {
  unsigned begin, end;
  size_t first, next;
  std::vector<std::string> words;
};

// 只检查指令名称，不解释条件表达式或宏正文。
bool opening(const std::string &word) {
  return word == "if" || word == "ifdef" || word == "ifndef";
}

// 标准保护只认可明确的否定定义检查，未知条件仍整块不透明。
std::string guardName(const Directive &directive) {
  const auto &words = directive.words;
  if (words.size() == 2 && words[0] == "ifndef") return words[1];
  if (words.size() == 4 && words[0] == "if" && words[1] == "!" && words[2] == "defined") return words[3];
  if (words.size() == 6 && words[0] == "if" && words[1] == "!" && words[2] == "defined" && words[3] == "(" && words[5] == ")") return words[4];
  return {};
}

} // namespace

// 原始 lexer 保证字符串、注释和续行中的井号不会被误当作新的指令。
SourceDocument sourceDocument(const std::string &source) {
  auto tokens = rawTokens(source);
  std::vector<Directive> directives;
  for (size_t i = 0; i < tokens.size(); ++i) {
    if (!tokens[i].is(clang::tok::hash) || !tokens[i].isAtStartOfLine()) continue;
    size_t next = i + 1;
    while (next < tokens.size() && !tokens[next].isAtStartOfLine()) ++next;
    Directive directive{tokens[i].getLocation().getRawEncoding(),
                        next < tokens.size() ? tokens[next].getLocation().getRawEncoding() : static_cast<unsigned>(source.size()),
                        i, next, {}};
    for (size_t word = i + 1; word < next; ++word) {
      auto &token = tokens[word];
      directive.words.push_back(source.substr(token.getLocation().getRawEncoding(), token.getLength()));
    }
    if (directive.words.empty()) directive.words.push_back("");
    directives.push_back(std::move(directive));
    i = next - 1;
  }

  std::map<size_t, size_t> closing;
  std::set<size_t> alternatives;
  std::vector<size_t> stack;
  for (size_t i = 0; i < directives.size(); ++i) {
    auto &word = directives[i].words[0];
    if (opening(word)) stack.push_back(i);
    else if ((word == "else" || word == "elif") && !stack.empty()) alternatives.insert(stack.back());
    else if (word == "endif" && !stack.empty()) {
      closing[stack.back()] = i;
      stack.pop_back();
    }
  }

  size_t guard = directives.size(), candidate = 0;
  while (candidate < directives.size() && directives[candidate].words == std::vector<std::string>{"pragma", "once"}) ++candidate;
  if (candidate + 1 < directives.size()) {
    auto name = guardName(directives[candidate]);
    auto end = closing.find(candidate);
    auto &definition = directives[candidate + 1];
    bool first = directives.front().first == 0;
    for(size_t i=1;i<=candidate;++i) first=first && directives[i-1].next==directives[i].first;
    if (!name.empty() && first && end != closing.end() && !alternatives.count(candidate) &&
        end->second + 1 == directives.size() && directives[end->second].next == tokens.size() &&
        directives[candidate].next == definition.first && definition.words.size() >= 2 &&
        definition.words[0] == "define" && definition.words[1] == name) {
      guard = candidate;
    }
  }

  SourceDocument document{source, {}};
  for (size_t i = 0; i < directives.size(); ++i) {
    auto &directive = directives[i];
    SourceRegion region{directive.begin, directive.end, "PreprocessorDirective", "#" + directive.words[0]};
    if (opening(directive.words[0]) && i != guard) {
      auto end = closing.find(i);
      region.kind = "ConditionalBlock";
      region.end = end == closing.end() ? static_cast<unsigned>(source.size()) : directives[end->second].end;
      document.regions.push_back(std::move(region));
      if (end == closing.end()) break;
      i = end->second;
      continue;
    }
    if (directive.words[0] == "define" && directive.words.size() > 1) region.name += " " + directive.words[1];
    // 保护结束行也单独保留；普通孤立 endif 同样不应从原文中消失。
    document.regions.push_back(std::move(region));
  }
  for (auto &region : document.regions) {
    for (unsigned i = region.begin; i < region.end; ++i) {
      if (document.view[i] != '\n' && document.view[i] != '\r') document.view[i] = ' ';
    }
  }
  return document;
}

} // namespace codegraph::clang_analyzer::detail
