import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';
import { execFileSync } from 'node:child_process';
import { systemTar } from '../language-packs/system-tar.mjs';
import { validateArchive } from '../language-packs/archive-validation.mjs';

/** 模拟 Git Bash 优先出现在 PATH，系统工具仍使用包含盘符和空格的明确路径。 */
test('system tar selection ignores PATH and preserves Windows drive and space paths', () => {
  for (const root of ['C:\\Windows', 'D:\\Windows system']) {
    assert.equal(systemTar({ platform: 'win32', env: { SystemRoot: root, PATH: 'C:\\Program Files\\Git\\usr\\bin' } }), `${root}\\System32\\tar.exe`);
  }
  assert.equal(systemTar({ platform: 'darwin', env: { PATH: '/untrusted/bin' } }), '/usr/bin/tar');
  for (const root of [undefined, '', 'Windows', 'D:Windows', '\\Windows']) {
    assert.throws(() => systemTar({ platform: 'win32', env: { SystemRoot: root } }), /absolute drive path/);
  }
});

/** 用真实 ZIP/tgz 和绝对空格路径覆盖 Node runtime 与 npm 包的生产解包参数。 */
test('system tar extracts ZIP and validated tgz archives from absolute space paths', async t => {
  const root = await fs.mkdtemp(path.join(os.tmpdir(), 'codegraph tar space-'));
  t.after(() => fs.rm(root, { recursive: true, force: true }));
  const tar = systemTar(), options = { encoding: 'utf8', timeout: 10_000, env: { ...process.env, COPYFILE_DISABLE: '1' } };
  for (const directory of ['node-win-x64', 'package']) await fs.mkdir(path.join(root, directory));
  await fs.writeFile(path.join(root, 'node-win-x64/node.exe'), 'runtime fixture');
  await fs.writeFile(path.join(root, 'node-win-x64/LICENSE'), 'runtime license');
  await fs.writeFile(path.join(root, 'package/index.js'), 'module fixture');
  const zip = path.join(root, 'node runtime.zip'), zipped = path.join(root, 'zip output');
  await fs.mkdir(zipped);
  execFileSync(tar, ['--format=zip', '-cf', zip, '-C', root, 'node-win-x64'], options);
  execFileSync(tar, ['-xf', zip, '-C', zipped, 'node-win-x64/node.exe', 'node-win-x64/LICENSE'], options);
  assert.equal(await fs.readFile(path.join(zipped, 'node-win-x64/node.exe'), 'utf8'), 'runtime fixture');
  assert.equal(await fs.readFile(path.join(zipped, 'node-win-x64/LICENSE'), 'utf8'), 'runtime license');
  const tgz = path.join(root, 'npm package.tgz'), unpacked = path.join(root, 'npm output');
  await fs.mkdir(unpacked);
  execFileSync(tar, ['-czf', tgz, '-C', root, 'package'], options);
  validateArchive(execFileSync(tar, ['-tzf', tgz], options), execFileSync(tar, ['-tvzf', tgz], options));
  execFileSync(tar, ['-xzf', tgz, '-C', unpacked, '--strip-components=1'], options);
  assert.equal(await fs.readFile(path.join(unpacked, 'index.js'), 'utf8'), 'module fixture');
});
