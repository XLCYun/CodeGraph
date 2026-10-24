import fs from 'node:fs';
import path from 'node:path';
import { relativePath } from './request.mjs';
import { within } from './workspace.mjs';

/** 集合必须唯一且每一项均为非空字符串。 */
function unique(values, label) {
  if (!Array.isArray(values) || values.some(value => typeof value !== 'string' || !value || value.includes('\0'))
    || new Set(values).size !== values.length) throw new Error(`无效或重复${label}`);
  return new Set(values);
}

/** 语义和扩展字段使用有名称的命名空间。 */
function namespaced(value) { return typeof value === 'string' && /^[^.\0]+\.[^\0]+$/.test(value); }

/** 在实际构图输出边界检查来源、成员与图连接，不修改或自动修补图。 */
export function validateProjection(result, input, platforms) {
  const { graph, evidence } = result;
  const root = fs.realpathSync.native(input.repository_root);
  /** 来源路径必须仍属于仓库中的普通文件。 */
  function source(relative) {
    relativePath(relative);
    const file = fs.realpathSync.native(path.join(root, relative));
    if (!within(root, file) || !fs.statSync(file).isFile()) throw new Error('构图来源越过源码边界');
  }
  const configurations = unique(evidence.configurations.map(config => config.id), '配置');
  if (input.files === undefined && !configurations.size) throw new Error('配置发现缺少实际配置');
  const approved = new Set(platforms.map(platform => platform.id));
  for (const config of evidence.configurations) {
    unique(config.source_paths, '配置来源').forEach(source);
    if ([...unique(config.platform_ids, '配置平台')].some(id => !approved.has(id))) throw new Error('配置引用未知声明');
  }
  if (evidence.files.length > 100000) throw new Error('文件覆盖超过上限');
  const selection = input.files === undefined ? null : new Set(input.files);
  const files = new Map();
  for (const file of evidence.files) {
    source(file.path);
    if (files.has(file.path) || selection && !selection.has(file.path)) throw new Error('重复或未选择的文件覆盖');
    if (!['analyzed', 'excluded_test'].includes(file.disposition) || input.include_tests && file.disposition === 'excluded_test') throw new Error('无效测试排除');
    const ids = unique(file.configuration_ids, '文件配置');
    if (!ids.size || [...ids].some(id => !configurations.has(id))) throw new Error('文件缺少实际配置');
    files.set(file.path, file.disposition);
  }
  if (input.files !== undefined && files.size !== input.files.length) throw new Error('文件覆盖遗漏选集');
  const ids = unique(graph.nodes.map(node => node.id), '节点身份');
  const origins = unique(evidence.origins.map(origin => origin.node_id), '节点来源');
  if (ids.size !== origins.size || [...ids].some(id => !origins.has(id))) throw new Error('节点与来源不一一对应');
  for (const origin of evidence.origins) {
    const selected = unique(origin.selected_paths, '节点来源路径');
    if (!selected.size || [...selected].some(file => files.get(file) !== 'analyzed')) throw new Error('完整节点来源不属于已分析选集');
  }
  const nodes = new Map(graph.nodes.map(node => [node.id, node]));
  const parents = new Set(), external = new Set();
  /** 单一父节点和有效端点同时约束根与所有 contains。 */
  function children(values) {
    for (const id of values) {
      if (!ids.has(id) || parents.has(id)) throw new Error('包含关系重复或目标不存在');
      parents.add(id);
    }
  }
  children(graph.root_contains);
  for (const node of graph.nodes) {
    if (!['container', 'type', 'callable', 'value', 'execution_region', 'extension'].includes(node.category)
      || !['typescript', 'javascript'].includes(node.language) || !node.native_kind
      || !node.id.startsWith(`${node.language}:`) || node.id.length <= node.language.length + 1) throw new Error('无效图节点类别或身份');
    for (const key of ['name', 'signature', 'code', 'source']) if (node[key] !== null && typeof node[key] !== 'string') throw new Error('节点自身内容字段缺失');
    if (Object.keys(node.extensions).some(key => !namespaced(key))) throw new Error('无效扩展命名空间');
    children(node.contains);
    const references = new Set();
    for (const reference of node.references) {
      const fact = JSON.stringify(reference);
      if (!namespaced(reference.semantic) || references.has(fact)) throw new Error('无效或重复关系');
      references.add(fact);
      const target = reference.target;
      if (target.kind === 'node' && typeof target.id === 'string' && ids.has(target.id) && Object.keys(target).length === 2) continue;
      if (target.kind === 'external' && typeof target.symbol === 'string' && /^[^:\0]+:[^\0]+$/.test(target.symbol)
        && !ids.has(target.symbol) && Object.keys(target).length === 2) { external.add(target.symbol); continue; }
      if (target.kind === 'unresolved' && typeof target.name === 'string' && target.name && !target.name.includes('\0') && Object.keys(target).length === 2) continue;
      throw new Error('无效关系目标');
    }
  }
  const reachable = new Set(), pending = [...graph.root_contains];
  while (pending.length) {
    const id = pending.pop();
    if (reachable.has(id)) throw new Error('包含关系形成环');
    reachable.add(id);
    // 逐项入栈，避免合法的大扇出触发运行时调用参数上限。
    for (const child of nodes.get(id).contains) pending.push(child);
  }
  if (reachable.size !== ids.size) throw new Error('图包含未连接到根的节点');
  const descriptors = unique(graph.external_symbols.map(symbol => symbol.symbol), '图外描述');
  if (descriptors.size !== external.size || [...external].some(id => !descriptors.has(id))) throw new Error('图外引用缺少完整描述');
  for (const symbol of graph.external_symbols) if (!symbol.name || !['repository', 'dependency'].includes(symbol.scope)) throw new Error('无效图外来源描述');
  return result;
}
