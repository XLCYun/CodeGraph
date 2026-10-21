import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { spawnSync } from 'node:child_process';
import { test } from 'node:test';

/** 公开请求只包含仓库、选集和既定分析选项。 */
function request(root) {
  return { protocol: 'codegraph.analyzer/1', type: 'analyze', id: 'direct', repository_root: root,
    files: [], options: { include_tests: false, rust_dependency_scope: 'repository-only' } };
}

/** 直接启动语言入口，stdin 在唯一 JSON 后关闭。 */
function exchange(payload, args = ['analyze']) {
  return spawnSync(process.execPath, [path.resolve(import.meta.dirname, '../main.mjs'), ...args],
    { input: payload, encoding: 'utf8', timeout: 10000, cwd: os.tmpdir() });
}

test('direct entry emits one fixed public empty result after EOF without loading declarations', t => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'codegraph-public-ts-'));
  t.after(() => fs.rmSync(root, { recursive: true, force: true }));
  fs.writeFileSync(path.join(root, 'tsconfig.json'), '{ broken');
  const result = exchange(JSON.stringify(request(root)));
  assert.equal(result.status, 0, result.stderr);
  assert.deepEqual(JSON.parse(result.stdout), { protocol: 'codegraph.analyzer/1', engine_version: '0.1.0',
    pack_id: 'typescript-javascript', type: 'completed', id: 'direct', outcome: 'ok',
    graph: { root_contains: [], nodes: [], external_symbols: [] } });
  assert.equal(result.stderr, '');
});
