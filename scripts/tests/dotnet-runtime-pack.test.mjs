import test from 'node:test';
import assert from 'node:assert/strict';
import { validatePublishedRuntimeConfig } from '../language-packs/dotnet-runtime-pack.mjs';

/** 固定 SDK 的真实 SCD 形状省略 rollForward，框架对象键顺序不参与协议语义。 */
test('self-contained runtime config accepts the actual fixed SDK shape', () => {
  const config = { tfm: 'net10.0', includedFrameworks: [{ version: '10.0.0', name: 'Microsoft.NETCore.App' }], configProperties: { 'System.Reflection.Metadata.MetadataUpdater.IsSupported': false } };
  validatePublishedRuntimeConfig(config);
  validatePublishedRuntimeConfig({ ...config, rollForward: 'Disable' });
});

/** 不得把补丁漂移、第二框架或依赖安装运行时的配置误当作固定自包含发布。 */
test('self-contained runtime config rejects version drift and external frameworks', () => {
  const config = { tfm: 'net10.0', includedFrameworks: [{ name: 'Microsoft.NETCore.App', version: '10.0.0' }] };
  for (const invalid of [undefined, {}, { ...config, tfm: 'net9.0' }, { ...config, includedFrameworks: [{ name: 'Microsoft.NETCore.App', version: '10.0.1' }] }, { ...config, includedFrameworks: [...config.includedFrameworks, ...config.includedFrameworks] }, { ...config, framework: { name: 'Microsoft.NETCore.App', version: '10.0.0' } }, { ...config, frameworks: [] }, { ...config, rollForward: 'LatestPatch' }, { ...config, rollForward: null }]) assert.throws(() => validatePublishedRuntimeConfig(invalid), /fixed self-contained runtime/);
});
