import test from 'node:test';
import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';

/** 在真实 Python 示例和管道上强制非 UTF-8 默认值，同时检查正常转发和子进程失败语义。 */
test('directory example preserves UTF-8 bytes through cp1252 boundaries', () => {
  const result = spawnSync(process.env.CODEGRAPH_PYTHON ?? (process.platform === 'win32' ? 'python' : 'python3'),
    [fileURLToPath(new URL('./fixtures/python-example-transport.py', import.meta.url)), '-v'],
    { encoding: 'utf8', timeout: 20_000 });
  assert.equal(result.status, 0, result.error?.message ?? result.stderr);
  assert.match(result.stderr, /Ran 2 tests/);
  assert.match(result.stderr, /\bOK\b/);
});
