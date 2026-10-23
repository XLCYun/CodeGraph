#include "detail/analysis.hpp"
#include <clang/Lex/Lexer.h>

using namespace clang;
namespace codegraph::clang_analyzer::detail {

// 从原始缓冲区取得全部 token；最后一个有效 token 与 EOF 必须分别处理。
std::vector<Token> rawTokens(const std::string &source) {
  LangOptions opts; opts.CPlusPlus=true; opts.CPlusPlus11=true; opts.CPlusPlus20=true; opts.LineComment=true;
  Lexer lexer(SourceLocation(),opts,source.data(),source.data(),source.data()+source.size());
  lexer.SetCommentRetentionState(false); std::vector<Token> tokens;
  while(true) {
    Token token; bool atEnd=lexer.LexFromRawLexer(token);
    if(token.is(tok::eof)) break;
    tokens.push_back(token);
    if(atEnd) break;
  }
  return tokens;
}
// 在原始 token 上去注释和排版，保留字面量及预处理指令边界。
std::string normalize(const std::string &source) {
  std::string out; bool directive=false;
  for(auto &t:rawTokens(source)) {
    if (t.isAtStartOfLine() && directive) { out+="\n"; directive=false; }
    if (t.isAtStartOfLine() && t.is(tok::hash)) directive=true;
    if (!out.empty() && out.back()!='\n') out+=' ';
    out.append(source.data()+t.getLocation().getRawEncoding(),t.getLength());
  }
  return out;
}

} // namespace codegraph::clang_analyzer::detail
