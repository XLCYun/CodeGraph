import assert from 'node:assert/strict';
import fs from 'node:fs/promises';
import path from 'node:path';

/** 生产者验证真实解包产物的目标发现策略；消费者不读取这份私有控制文件。 */
export async function verifyNativePackPolicy(id, root) {
  const kind = { go: 'go', java: 'java', csharp: 'dotnet' }[id];
  if (!kind) return;
  const control = JSON.parse(await fs.readFile(path.join(root, 'control/native.json'), 'utf8'));
  assert.deepEqual(control.target_sdk, { kind }, `${id}: target SDK discovery policy changed`);
  assert.deepEqual(control.platforms, [], `${id}: target declarations must not be bundled`);
  await assert.rejects(fs.lstat(path.join(root, 'runtime/platform')), { code: 'ENOENT' }, `${id}: bundled target platform directory must be absent`);
}
