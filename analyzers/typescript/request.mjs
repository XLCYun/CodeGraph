import fs from 'node:fs';
import path from 'node:path';
import { within } from './workspace.mjs';

export const MAX_BYTES = 128 * 1024 * 1024;

/** 保留公共错误类别，语言内部异常统一由入口归为分析失败。 */
export class AnalysisFailure extends Error {
  /** 建立不携带私有路径清单的公开错误。 */
  constructor(code, message) { super(message); this.code = code; }
}

/** 在整体 JSON.parse 之前逐层检查键；转义拼写不同的同名键也不能被覆盖。 */
function rejectDuplicateKeys(text) {
  const token = /\s*("(?:[^"\\]|\\(?:["\\/bfnrt]|u[0-9a-fA-F]{4}))*"|-?(?:0|[1-9]\d*)(?:\.\d+)?(?:[eE][+-]?\d+)?|true|false|null|[{}[\],:])/y;
  let offset = 0;
  /** 读取一个严格 JSON 词法单元，拒绝未识别内容。 */
  function next() {
    token.lastIndex = offset;
    const match = token.exec(text);
    if (!match) throw new SyntaxError('无效 JSON');
    offset = token.lastIndex;
    return match[1];
  }
  /** 有限递归只保留当前对象的键集合，不构造另一份协议对象。 */
  function value(first, depth = 0) {
    if (depth > 64) throw new SyntaxError('JSON 嵌套过深');
    if (first === '{') {
      const keys = new Set();
      let key = next();
      if (key === '}') return;
      while (true) {
        if (!key.startsWith('"')) throw new SyntaxError('无效 JSON 对象键');
        const decoded = JSON.parse(key);
        if (keys.has(decoded)) throw new SyntaxError(`重复 JSON 键：${decoded}`);
        keys.add(decoded);
        if (next() !== ':') throw new SyntaxError('无效 JSON 对象');
        value(next(), depth + 1);
        const end = next();
        if (end === '}') return;
        if (end !== ',') throw new SyntaxError('无效 JSON 对象');
        key = next();
      }
    }
    if (first === '[') {
      let item = next();
      if (item === ']') return;
      while (true) {
        value(item, depth + 1);
        const end = next();
        if (end === ']') return;
        if (end !== ',') throw new SyntaxError('无效 JSON 数组');
        item = next();
      }
    }
    if (/^[}\],:]$/.test(first)) throw new SyntaxError('无效 JSON 值');
  }
  value(next());
  if (text.slice(offset).trim()) throw new SyntaxError('JSON 尾部内容');
}

/** 对象采用闭合字段集；可选选集只允许省略，不接受 null。 */
function fields(object, names) {
  if (!object || typeof object !== 'object' || Array.isArray(object)
    || Object.keys(object).length !== names.length || !names.every(name => Object.hasOwn(object, name))) throw new Error('无效协议字段');
}

/** 字符串不能含 NUL 或无法无损编码为 UTF-8 的代理项。 */
function string(value) {
  if (typeof value !== 'string' || !value || value.includes('\0') || !value.isWellFormed()) throw new Error('无效协议字符串');
}

/** 所有选集及来源路径共用规范 POSIX 相对路径规则。 */
export function relativePath(value) {
  string(value);
  if (value.includes('\\') || value.includes(':') || value.startsWith('/')
    || value.split('/').some(part => !part || part === '.' || part === '..')) throw new Error('无效仓库相对路径');
}

/** 直接解码一次公共请求，不接受内部目录、SDK、命令或控制消息。 */
export function decodeRequest(text) {
  try {
    rejectDuplicateKeys(text);
    const message = JSON.parse(text);
    if (message && typeof message === 'object' && typeof message.protocol === 'string'
      && message.protocol !== 'codegraph.analyzer/1') throw new AnalysisFailure('incompatible_protocol', '不兼容的分析协议');
    fields(message, ['protocol', 'type', 'id', 'repository_root', 'options', ...(Object.hasOwn(message ?? {}, 'files') ? ['files'] : [])]);
    if (message.protocol !== 'codegraph.analyzer/1' || message.type !== 'analyze') throw new Error('无效分析请求');
    string(message.id);
    if (!message.id.trim() || Buffer.byteLength(message.id) > 1024) throw new Error('无效请求 ID');
    string(message.repository_root);
    if (!path.isAbsolute(message.repository_root)) throw new Error('项目根必须为绝对路径');
    fields(message.options, ['include_tests', 'rust_dependency_scope']);
    if (typeof message.options.include_tests !== 'boolean'
      || !['repository-only', 'standard-library', 'all-dependencies'].includes(message.options.rust_dependency_scope)) throw new Error('无效分析选项');
    if (Object.hasOwn(message, 'files')) {
      if (!Array.isArray(message.files) || message.files.length > 100000) throw new Error('无效文件选集');
      message.files.forEach(relativePath);
      if (new Set(message.files).size !== message.files.length) throw new Error('重复选集路径');
    }
    return message;
  } catch (error) {
    if (error instanceof AnalysisFailure) throw error;
    throw new AnalysisFailure('invalid_request', error.message);
  }
}

/** 根和每个选中文件必须真实存在；安装与源码不能相互包含。 */
export function validateInput(request, installation) {
  try {
    const root = fs.realpathSync.native(request.repository_root);
    const installed = fs.realpathSync.native(installation);
    if (!fs.statSync(root).isDirectory() || within(root, installed) || within(installed, root)) throw new Error('源码与分析器安装重叠或根不是目录');
    for (const relative of request.files ?? []) {
      const file = fs.realpathSync.native(path.join(root, relative));
      if (!within(root, file) || !fs.statSync(file).isFile()) throw new Error('选中文件越界或不是普通文件');
    }
    return { ...request, repository_root: root };
  } catch (error) { throw new AnalysisFailure('invalid_input', `无效源码输入：${error.code ?? error.message}`); }
}
