import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { test } from 'node:test';
import { analyze } from '../analyze.mjs';
import { validateProjection } from '../graph.mjs';
import { runtimeContext } from '../runtime.mjs';
import { runtime, ts } from './support.mjs';

/** 使用真实编译器的有效图作为输入，再逐项破坏公共不变量。 */
test('natural output boundary rejects broken identities, coverage and graph endpoints without repair', t => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'codegraph TS 图边界-'));
  t.after(() => fs.rmSync(root, { recursive: true, force: true }));
  fs.writeFileSync(path.join(root, 'main.ts'), 'export function caller(){target(); Promise.resolve(1)} export function target(){}');
  const { platforms } = runtimeContext(runtime);
  const input = { repository_root: root, files: ['main.ts'], include_tests: false, profile: 'typescript-static-v1' };
  const result = analyze(ts, { id: 'graph', input, environment: { platforms } });
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
