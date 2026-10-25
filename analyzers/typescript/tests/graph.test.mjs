import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { test } from 'node:test';
import { analyze } from '../analyze.mjs';
import { project } from '../projection.mjs';
import { validateProjection } from '../graph.mjs';
import { analyzerRuntime, ts } from './support.mjs';

/** 使用真实编译器的有效图作为输入，再逐项破坏公共不变量。 */
test('natural output boundary rejects broken identities, coverage and graph endpoints without repair', t => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'codegraph TS 图边界-'));
  t.after(() => fs.rmSync(root, { recursive: true, force: true }));
  fs.writeFileSync(path.join(root, 'main.ts'), 'export function caller(){target(); Promise.resolve(1)} export function target(){}');
  const { platforms } = analyzerRuntime;
  const input = { repository_root: root, files: ['main.ts'], include_tests: false, profile: 'typescript-static-v1' };
  const result = analyze(analyzerRuntime, { id: 'graph', input });
  const mutations = [
    value => value.graph.nodes.push(structuredClone(value.graph.nodes[0])),
    value => { value.evidence.origins = []; },
    value => { value.evidence.files = []; },
    value => { value.evidence.origins[0].selected_paths = ['outside.ts']; },
    value => { value.graph.root_contains = []; },
    value => value.graph.root_contains.push(value.graph.root_contains[0]),
    value => { value.graph.nodes[0].category = 'unknown'; },
    value => { value.graph.nodes[0].id = 'go:wrong'; },
    value => value.graph.nodes[0].references.push({ semantic: 'code.calls', target: { kind: 'node', id: 'typescript:missing' } }),
    value => { value.graph.external_symbols = []; },
    value => { value.graph.nodes[0].extensions.unqualified = true; },
  ];
  for (const mutate of mutations) {
    const value = structuredClone(result);
    mutate(value);
    const before = JSON.stringify(value);
    assert.throws(() => validateProjection(value, input, platforms));
    assert.equal(JSON.stringify(value), before, 'validation cannot silently repair graph data');
  }
});

/** 合法源码可产生超过 V8 调用参数上限的子节点，图校验不能因此截断或失败。 */
test('a valid native graph with 140000 children remains complete and reachable', { timeout: 60000 }, t => {
  const count = 140000;
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'codegraph TS 大扇出-'));
  t.after(() => fs.rmSync(root, { recursive: true, force: true }));
  const source = `export interface Big {${Array.from({ length: count }, (_, index) => `p${index}:number;`).join('')}}`;
  const file = path.join(root, 'main.ts');
  fs.writeFileSync(file, source);
  const sf = ts.createSourceFile(file, source, ts.ScriptTarget.Latest, true, ts.ScriptKind.TS);
  assert.equal(sf.parseDiagnostics.length, 0);
  const nodes = project(ts, sf, root).declarations.map(declaration => declaration.node);
  const rootIds = nodes.filter(node => node.native_kind === 'file').map(node => node.id);
  const children = nodes.find(node => node.name === 'Big').contains;
  assert.equal(nodes.length, count + 2);
  assert.equal(new Set(nodes.map(node => node.id)).size, count + 2);
  assert.equal(children.length, count);
  assert.equal(new Set(children).size, count);
  const result = {
    graph: { root_contains: rootIds, nodes, external_symbols: [] },
    evidence: {
      configurations: [{ id: 'large-interface', source_paths: [], platform_ids: [] }],
      files: [{ path: 'main.ts', disposition: 'analyzed', configuration_ids: ['large-interface'] }],
      origins: nodes.map(node => ({ node_id: node.id, selected_paths: ['main.ts'] })),
    },
  };
  const output = { protocol: 'codegraph.analyzer/1', engine_version: '0.1.1', pack_id: 'typescript-javascript',
    type: 'completed', id: 'review-high-fanout', outcome: 'ok', graph: result.graph };
  const bytes = Buffer.byteLength(JSON.stringify(output)) + 1;
  assert.ok(bytes < 128 * 1024 * 1024);
  t.diagnostic(JSON.stringify({ node: process.version, typescript: ts.version, source_bytes: Buffer.byteLength(source),
    node_count: nodes.length, children: children.length, public_result_bytes: bytes }));
  const childIds = [...children], nodeIds = nodes.map(node => node.id);
  const input = { repository_root: root, files: ['main.ts'], include_tests: false, profile: 'typescript-static-v1' };
  assert.equal(validateProjection(result, input, []), result);
  assert.equal(result.graph.nodes.length, count + 2);
  assert.equal(children.length, count);
  assert.deepEqual(children, childIds);
  assert.deepEqual(result.graph.nodes.map(node => node.id), nodeIds);
  assert.equal(fs.readFileSync(file, 'utf8'), source);
});
