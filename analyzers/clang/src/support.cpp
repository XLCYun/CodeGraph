#include "detail/analysis.hpp"
#include <llvm/Support/raw_ostream.h>
#include <cctype>
#include <cstdlib>
#include <fstream>
#include <iterator>
namespace codegraph::clang_analyzer::detail {

// 检查协议条件，失败时停止整个请求。
void require(bool ok, const std::string &message, const std::string &code) {
  if (!ok) throw Failure(code, message);
}
// JSON 序列化只用于协议数据，不构造命令。
std::string jsonText(const j::Value &v) {
  std::string out; llvm::raw_string_ostream stream(out); stream << v; return out;
}
// 提取必填字符串并校验类型。
std::string str(const j::Object &o, llvm::StringRef key) {
  auto value = o.getString(key); require(value.has_value(), "missing/string field: " + key.str());
  return value->str();
}
// 精确校验字段集合，拒绝静默扩展协议。
void keys(const j::Object &o, std::initializer_list<const char *> names) {
  std::set<std::string> expected; for (auto n : names) expected.insert(n);
  require(o.size() == expected.size(), "unexpected/missing object fields");
  for (auto &kv : o) require(expected.count(kv.first.str()), "unknown field: " + kv.first.str());
}
// 严格扫描 JSON 容器，额外拒绝 LLVM JSON 默认允许的重复键。
class UniqueKeys {
  const std::string &s; size_t p = 0;
  // 跳过 JSON 空白。
  void ws() { while (p < s.size() && isspace(static_cast<unsigned char>(s[p]))) ++p; }
  // 读取并解码字符串键，转义后的相同键也会被拒绝。
  std::string string() {
    size_t start = p++; bool closed = false;
    // LLVM 默认替换孤立代理项；严格输入必须在任何身份解码前拒绝这种有损转换。
    auto codeUnit=[&]() {
      require(p+4<=s.size(),"incomplete Unicode escape"); unsigned value=0;
      for(unsigned count=0;count<4;++count) { char c=s[p++]; unsigned digit=c>='0' && c<='9'?c-'0':c>='a' && c<='f'?c-'a'+10:c>='A' && c<='F'?c-'A'+10:16; require(digit<16,"invalid Unicode escape"); value=value*16+digit; }
      return value;
    };
    while (p < s.size()) {
      char c=s[p++];
      if(c=='\\') {
        require(p<s.size(),"incomplete JSON escape"); char escaped=s[p++];
        if(escaped=='u') {
          auto value=codeUnit(); require(value<0xdc00 || value>0xdfff,"unpaired Unicode surrogate");
          if(value>=0xd800 && value<=0xdbff) { require(p+2<=s.size() && s[p]=='\\' && s[p+1]=='u',"unpaired Unicode surrogate"); p+=2; auto low=codeUnit(); require(low>=0xdc00 && low<=0xdfff,"unpaired Unicode surrogate"); }
        }
      } else if(c=='"') { closed=true; break; }
    }
    require(closed, "unterminated JSON string");
    auto value = j::parse(s.substr(start,p-start)); if(!value) throw Failure("invalid_input",llvm::toString(value.takeError()));
    return value->getAsString()->str();
  }
  // 递归检查每个对象的键，容器深度有界。
  void value(unsigned depth) {
    require(depth < 128, "JSON nesting limit"); ws(); require(p < s.size(), "incomplete JSON");
    if (s[p] == '"') { string(); return; }
    if (s[p] == '{' || s[p] == '[') {
      bool object = s[p++] == '{'; char end = object ? '}' : ']'; std::set<std::string> seen; ws();
      if (p < s.size() && s[p] == end) { ++p; return; }
      while (true) {
        ws(); if (object) { require(p < s.size() && s[p] == '"', "invalid JSON key"); auto key = string(); require(seen.insert(key).second, "duplicate JSON key: " + key); ws(); require(p < s.size() && s[p++] == ':', "invalid JSON colon"); }
        value(depth+1); ws(); require(p < s.size(), "incomplete JSON container");
        if (s[p] == end) { ++p; return; } require(s[p++] == ',', "invalid JSON separator");
      }
    }
    size_t start=p; while (p<s.size() && s[p]!=',' && s[p]!=']' && s[p]!='}' && !isspace(static_cast<unsigned char>(s[p]))) ++p;
    require(p>start, "invalid JSON value");
  }
public:
  // 扫描完整消息，拒绝尾随数据。
  explicit UniqueKeys(const std::string &text) : s(text) { value(0); ws(); require(p==s.size(), "trailing JSON"); }
};
// 解析有界 JSON 文档，协议和 CDB 共用严格键规则。
j::Value parse(const std::string &text) {
  UniqueKeys check(text); auto v=j::parse(text); if(!v) throw Failure("invalid_input",llvm::toString(v.takeError())); return std::move(*v);
}
// 从磁盘读取小型配置；不执行文件、不写入项目。
std::string read(const fs::path &path) {
  require(fs::is_regular_file(path), "missing file: " + path.u8string());
  require(fs::file_size(path)<=32*1024*1024, "input file too large", "resource_limit");
  std::ifstream in(path, std::ios::binary); require(bool(in), "cannot read file");
  return std::string(std::istreambuf_iterator<char>(in), {});
}
// 对路径做真实文件系统归一化，消除符号链接越界。
fs::path real(const fs::path &path) {
  std::error_code ec; auto p=fs::canonical(path,ec); require(!ec,"unavailable path: "+path.u8string()); return p;
}
// 按路径组件判断包含关系，避免字符串前缀误判。
bool under(const fs::path &path, const fs::path &root) {
  auto a=path.begin(), b=root.begin(); for (; b!=root.end(); ++b,++a) if (a==path.end() || *a!=*b) return false; return true;
}
// 排除已安装的业务依赖与缓存，允许普通仓内提交源码。
bool excluded(const fs::path &relative) {
  for (const auto &part : relative) if (part=="node_modules" || part=="vendor" || part==".git" || part==".cache" || part==".venv" || part=="target") return true;
  return false;
}
// 验证协议相对路径，不接受平台相关的替代分隔符或父级跳转。
fs::path relativePath(const std::string &s) {
  fs::path p=fs::u8path(s); require(!s.empty() && !p.is_absolute() && s.front()!='/' && s.back()!='/' && s.find("//")==std::string::npos && s.find('\\')==std::string::npos && s.find(':')==std::string::npos && s.find('\0')==std::string::npos, "invalid relative path");
  for (const auto &part : p) require(part!=".." && part!=".","invalid path component"); return p;
}
// 清除可能注入 include、驱动器或配置路径的宿主变量。
void clearEnvironment(const char *name) {
#ifdef _WIN32
  _putenv_s(name,"");
#else
  unsetenv(name);
#endif
}

} // namespace codegraph::clang_analyzer::detail
