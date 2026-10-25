import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { spawnSync } from 'node:child_process';
import { after, test } from 'node:test';
import { directConfiguration } from './support.mjs';

const configuration = directConfiguration({ after });
const protocol = 'codegraph.analyzer/1';
const vectors = JSON.parse(fs.readFileSync(new URL('../../common/src/tests/public-conformance.json', import.meta.url)));

/** 实际包布局的唯一公开请求；不提供声明 catalog 或运行命令。 */
function fixture(t) {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'codegraph TS 协议-'));
  t.after(() => fs.rmSync(root, { recursive: true, force: true }));
  return { protocol, type: 'analyze', id: 'review', repository_root: root, files: [],
    options: { include_tests: false, rust_dependency_scope: 'repository-only' } };
}

/** 一次请求完整写入后 EOF，stdin、stdout、stderr 都由真实运行时处理。 */
function exchange(payload, args = configuration.args) {
  const result = spawnSync(configuration.executable, args, { input: payload, encoding: 'utf8', timeout: 15000, cwd: os.tmpdir() });
  assert.equal(result.error, undefined);
  return { ...result, message: result.stdout && JSON.parse(result.stdout) };
}

for (const vector of vectors) test(`shared public conformance: ${vector.name}`, t => {
  const request = { ...fixture(t), ...vector.patch };
  const result = exchange(JSON.stringify(request) + vector.suffix);
  assert.equal(result.status, vector.outcome === 'ok' ? 0 : 1, result.stderr);
  assert.equal(result.message.protocol, protocol);
  assert.equal(result.message.engine_version, '0.1.1');
  assert.equal(result.message.pack_id, 'typescript-javascript');
  assert.equal(result.message.type, 'completed');
  assert.equal(result.message.outcome, vector.outcome);
  if (vector.error_code) { assert.equal(result.message.error.code, vector.error_code); assert.ok(!Object.hasOwn(result.message, 'graph')); }
  assert.ok(!Object.hasOwn(result.message, 'evidence'));
});

for (const [name, replace] of [
  ['id', text => text.replace('"id":"review"', '"id":"discarded","id":"review"')],
  ['escaped id', text => text.replace('"id":"review"', '"i\\u0064":"discarded","id":"review"')],
  ['options', text => text.replace('"include_tests":false', '"include_tests":true,"include_tests":false')],
  ['nested unknown', text => text.replace('"files":[]', '"files":[],"extra":{"key":1,"key":2}')],
]) test(`duplicate JSON keys are rejected recursively: ${name}`, t => {
  const result = exchange(replace(JSON.stringify(fixture(t))));
  assert.equal(result.status, 1);
  assert.equal(result.message.id, null);
  assert.equal(result.message.error.code, 'invalid_request');
});

for (const value of ['', '[]', '{}', '{broken', Buffer.from([0xff]), '{"a":'.repeat(70) + '0' + '}'.repeat(70)]) test(`malformed whole input is a handled failure: ${String(value).slice(0, 24)}`, () => {
  const result = exchange(value);
  assert.equal(result.status, 1);
  assert.equal(result.message.id, null);
  assert.equal(result.message.error.code, 'invalid_request');
});

test('public discovery, exact selection, external context and test policy use the real compiler', t => {
  const request = fixture(t);
  const root = request.repository_root;
  const contents = {
    'tsconfig.json': JSON.stringify({ compilerOptions: { target: 'ES2022', module: 'Preserve', moduleResolution: 'Bundler', types: [] }, include: ['src'] }),
    'src/主程序.ts': "import {target} from './target'; export function caller(){target(); Promise.resolve(1)}",
    'src/target.ts': 'export function target(){return 1}',
    'src/main.test.ts': 'export function testOnly(){}', 'stray.ts': 'export function stray(){}',
  };
  for (const [name, text] of Object.entries(contents)) { fs.mkdirSync(path.dirname(path.join(root, name)), { recursive: true }); fs.writeFileSync(path.join(root, name), text); }
  delete request.files;
  const discovered = exchange(JSON.stringify(request));
  assert.equal(discovered.status, 0, discovered.stdout);
  assert.ok(discovered.message.graph.nodes.some(node => node.name === 'target'));
  assert.ok(!discovered.message.graph.nodes.some(node => ['stray', 'testOnly'].includes(node.name)));
  request.files = ['src/主程序.ts'];
  const selected = exchange(JSON.stringify(request));
  assert.equal(selected.status, 0, selected.stdout);
  assert.ok(!selected.message.graph.nodes.some(node => node.name === 'target'));
  assert.ok(selected.message.graph.external_symbols.some(symbol => symbol.name === 'target' && symbol.scope === 'repository'));
  assert.ok(selected.message.graph.external_symbols.some(symbol => symbol.scope === 'dependency'));
  request.files = ['src/main.test.ts'];
  request.options.include_tests = true;
  assert.ok(exchange(JSON.stringify(request)).message.graph.nodes.some(node => node.name === 'testOnly'));
  for (const [name, text] of Object.entries(contents)) assert.equal(fs.readFileSync(path.join(root, name), 'utf8'), text);
});

for (const args of [[], ['analyze', 'analyze'], ['--version', '--version'], ['__worker'], ['analyze', '--pack-id', 'java']]) test(`invalid CLI exits two: ${JSON.stringify(args)}`, () => {
  const result = exchange('', [...configuration.args.slice(0, -1), ...args]);
  assert.equal(result.status, 2);
  assert.equal(result.stdout, '');
});
for (const args of [['--help'], ['analyze', '--help'], ['--version']]) test(`CLI text is forwarded by the program: ${args.join(' ')}`, () => {
  const result = spawnSync(configuration.executable, [...configuration.args.slice(0, -1), ...args], { encoding: 'utf8', timeout: 10000 });
  assert.equal(result.status, 0);
  assert.match(result.stdout, /codegraph-typescript-javascript/);
  assert.equal(result.stderr, '');
});

for (const [name, mutate] of [
  ['private envelope', request => ({ protocol, type: 'analyze', id: request.id, input: {}, environment: {}, limits: {} })],
  ['oversized UTF-8 id', request => ({ ...request, id: '源'.repeat(400) })],
  ['unpaired surrogate', request => ({ ...request, id: '\uD800' })],
  ['too many files', request => ({ ...request, files: Array.from({ length: 100001 }, (_, index) => `f${index}.ts`) })],
]) test(`public boundary rejects ${name}`, t => {
  const result = exchange(JSON.stringify(mutate(fixture(t))));
  assert.equal(result.status, 1);
  assert.equal(result.message.id, null);
  assert.equal(result.message.error.code, 'invalid_request');
});

test('installation cannot overlap selected source even for explicit empty selection', t => {
  const request = fixture(t);
  request.repository_root = configuration.root;
  const result = exchange(JSON.stringify(request));
  assert.equal(result.status, 1);
  assert.equal(result.message.id, request.id);
  assert.equal(result.message.error.code, 'invalid_input');
});

/** 损坏的固定运行时仍在公开入口失败；显式空选集不触碰声明或无关配置。 */
for (const [name, mutate, code, message] of [
  ['malformed manifest', root => fs.writeFileSync(path.join(root, 'runtime/typescript/package.json'), '{broken'), 'invalid_pack', /随包 TypeScript 声明不可用/],
  ['mismatched declaration version', root => fs.writeFileSync(path.join(root, 'runtime/@types/node/package.json'), '{"version":"0.0.0"}'), 'invalid_pack', /固定声明版本不符/],
  ['missing compiler', root => fs.unlinkSync(path.join(root, 'runtime/typescript/lib/typescript.js')), 'invalid_pack', /随包 TypeScript 声明不可用/],
  ['mismatched compiler version', root => fs.writeFileSync(path.join(root, 'runtime/typescript/lib/typescript.js'), 'module.exports = { version: "0.0.0" };'), 'analysis_failed', /需要固定 TypeScript/],
  ['mismatched canonical catalog', (root, t) => {
    const other = directConfiguration(t);
    const library = path.join(root, 'runtime/typescript/lib');
    fs.rmSync(library, { recursive: true });
    fs.symlinkSync(path.join(other.root, 'runtime/typescript/lib'), library, 'junction');
  }, 'analysis_failed', /声明 catalog 与固定运行时目录不一致/],
]) test(`fixed runtime rejects ${name} while empty selection still short circuits`, t => {
  const pack = directConfiguration(t);
  mutate(pack.root, t);
  const request = fixture(t);
  fs.writeFileSync(path.join(request.repository_root, 'tsconfig.json'), '{broken');
  assert.equal(exchange(JSON.stringify(request), pack.args).status, 0);
  request.files = ['main.ts'];
  fs.writeFileSync(path.join(request.repository_root, 'main.ts'), 'export function source(){}');
  const result = exchange(JSON.stringify(request), pack.args);
  assert.equal(result.status, 1);
  assert.equal(result.message.id, request.id);
  assert.equal(result.message.error.code, code);
  assert.match(result.message.error.message, message);
});
