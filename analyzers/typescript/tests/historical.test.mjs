import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { test } from 'node:test';
import { analyze } from '../analyze.mjs';
import { runtimeContext } from '../runtime.mjs';
import { runtime, ts } from './support.mjs';

/** 继续使用退役前冻结的身份、类别、原文与包含树，不重新生成期望值。 */
test('historical public identity and source fixture remains exact in the direct compiler', t => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'codegraph TS 历史身份-'));
  t.after(() => fs.rmSync(root, { recursive: true, force: true }));
  const sources = {
    "中文 path/声明!~'().ts": '/** docs */\nexport class Model { field = 1; constructor(public prop: string) {} method(){} static { run(); } } namespace N { export const arrow = () => 1; } const handlers = { nested: { save(){} } }; function overloaded(x:string):string; function overloaded(x:any){return x;} interface I { property: string; method(): void; } interface I { another: number; }',
    'main.js': 'export function caller() {} const expression = function hidden(){}; class C { #field = 1; get value(){return 1} set value(v){} };',
    'default.ts': 'export default function() {}',
  };
  for (const [file, source] of Object.entries(sources)) { fs.mkdirSync(path.dirname(path.join(root, file)), { recursive: true }); fs.writeFileSync(path.join(root, file), source); }
  const { platforms } = runtimeContext(runtime);
  const { graph } = analyze(ts, { id: 'history', input: { repository_root: root, files: Object.keys(sources), include_tests: false, profile: 'typescript-static-v1' }, environment: { platforms } });
  const expected = JSON.parse(fs.readFileSync(new URL('../../../src/adapters/tests/fixtures/typescript-historical-identity.json', import.meta.url)));
  const category = { container: 'Container', type: 'Type', callable: 'Callable', value: 'Value', execution_region: 'ExecutionRegion', extension: 'Extension' };
  /** 历史 fixture 保持 POSIX 路径；只沿用既有 Windows 分隔符转义差异。 */
  const platformId = id => process.platform === 'win32' ? id.replaceAll('%2F', '%5C') : id;
  assert.equal(graph.nodes.length, expected.length);
  for (const frozen of expected) {
    const node = graph.nodes.find(node => node.id === platformId(frozen.id));
    assert.ok(node, frozen.id);
    for (const key of ['native_kind', 'name', 'source']) assert.equal(node[key], frozen[key], `${frozen.id}: ${key}`);
    assert.equal(category[node.category], frozen.category);
    assert.deepEqual([...node.contains].sort(), frozen.contains.map(platformId).sort());
  }
  for (const [file, source] of Object.entries(sources)) assert.equal(fs.readFileSync(path.join(root, file), 'utf8'), source);
});
