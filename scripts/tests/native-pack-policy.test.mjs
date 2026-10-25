import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';
import { packageDirectory } from '../language-packs/build-language-support-packs.mjs';
import { verifyAndExtract } from '../language-packs/verify-distribution.mjs';
import { verifyNativePackPolicy } from '../language-packs/verify-native-pack-policy.mjs';

/** 字节完整的真实归档也可能违反 SDK 策略；每项负例须在完整性校验之后独立失败。 */
test('verified native archives reject bundled targets and incorrect discovery policies', async t => {
  const root = await fs.mkdtemp(path.join(os.tmpdir(), 'codegraph native policy '));
  t.after(() => fs.rm(root, { recursive: true, force: true }));
  const target = process.platform === 'win32' ? 'x86_64-pc-windows-msvc' : 'aarch64-apple-darwin';
  for (const [id, kind] of [['go', 'go'], ['java', 'java'], ['csharp', 'dotnet']]) {
    for (const defect of ['none', 'wrong-kind', 'bundled-catalog', 'bundled-files']) {
      const payload = path.join(root, `${id}-${defect}`);
      await fs.mkdir(path.join(payload, 'control'), { recursive: true });
      const control = { schema_version: 1, helper_version: '0.1.1', target_sdk: { kind }, platforms: [] };
      if (defect === 'wrong-kind') control.target_sdk.kind = 'unrelated';
      if (defect === 'bundled-catalog') control.platforms.push({ id: 'fixed-build-target', root: 'runtime/platform' });
      if (defect === 'bundled-files') {
        await fs.mkdir(path.join(payload, 'runtime/platform'), { recursive: true });
        await fs.writeFile(path.join(payload, 'runtime/platform/declaration'), 'unintended target declarations');
      }
      await fs.writeFile(path.join(payload, 'control/native.json'), JSON.stringify(control));
      const archive = await packageDirectory({ id, version: '0.1.1', target }, payload, path.join(root, `output-${defect}`));
      const extracted = path.join(root, `${id}-${defect}-installed`);
      await verifyAndExtract(archive, extracted, { id, target, version: '0.1.1' });
      if (defect === 'none') await verifyNativePackPolicy(id, extracted);
      else await assert.rejects(verifyNativePackPolicy(id, extracted), {
        name: 'AssertionError',
        message: { 'wrong-kind': /discovery policy/, 'bundled-catalog': /must not be bundled/, 'bundled-files': /must be absent/ }[defect],
      });
    }
  }
});
