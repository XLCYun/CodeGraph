import assert from 'node:assert/strict';
import fs from 'node:fs/promises';
import path from 'node:path';

/** 生产者验证真实解包产物的目标发现策略；消费者不读取这份私有控制文件。 */
export function validateNativePackPolicy(id, control, files) {
  const kind = { go: 'go', java: 'java', csharp: 'dotnet' }[id];
  if (!kind) return;
  assert.deepEqual(control.target_sdk, { kind }, `${id}: target SDK discovery policy changed`);
  assert.deepEqual(control.platforms, [], `${id}: target declarations must not be bundled`);
  assert.ok(!files.some(name => name === 'runtime/platform' || name.startsWith('runtime/platform/')), `${id}: bundled target platform directory must be absent`);
}

/** 已落盘的包沿用相同 SDK 策略，并额外拒绝空的目标平台目录。 */
export async function verifyNativePackPolicy(id, root) {
  if (!['go', 'java', 'csharp'].includes(id)) return;
  validateNativePackPolicy(id, JSON.parse(await fs.readFile(path.join(root, 'control/native.json'), 'utf8')), []);
  await assert.rejects(fs.lstat(path.join(root, 'runtime/platform')), { code: 'ENOENT' }, `${id}: bundled target platform directory must be absent`);
}
