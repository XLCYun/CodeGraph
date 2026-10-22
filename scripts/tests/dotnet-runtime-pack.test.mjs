import test from 'node:test';
import assert from 'node:assert/strict';
import { validatePublishedRuntimeConfig, validateRuntimeArchiveListing } from '../language-packs/dotnet-runtime-pack.mjs';

/** Windows 的 CRLF 是清单记录分隔符，不能变成合法固定归档路径尾部的控制字符。 */
test('runtime archive inspection accepts both LF and CRLF file and directory records', () => {
  const names = ['LICENSE.TXT', 'runtimes/', 'runtimes/win-x64/native/coreclr.dll', 'THIRD-PARTY-NOTICES.TXT'];
  const listing = ['-rw-r--r-- file', 'drwxr-xr-x directory', '-rw-r--r-- runtime', '-rw-r--r-- notice'];
  for (const eol of ['\n', '\r\n']) for (const suffix of ['', eol]) validateRuntimeArchiveListing(names.join(eol) + suffix, listing.join(eol) + suffix);
});

/** 只识别记录换行；路径中的控制字符、穿越、盘符和歧义名称仍须原样拒绝。 */
test('runtime archive inspection retains unsafe path rejection with either line ending', () => {
  const listing = '-rw-r--r-- file';
  for (const eol of ['\n', '\r\n']) for (const name of ['../escape', '/absolute', 'C:/escape', 'bin\\escape', 'CON.txt', 'a//b', 'a/./b', 'a/../b', 'end.', 'end ', 'end\t', 'bin/a\rb', 'bin/a\0b', 'bin/a\x7fb']) assert.throws(() => validateRuntimeArchiveListing(name + eol, listing + eol), /unsafe archive entries/, JSON.stringify({ eol, name }));
  assert.throws(() => validateRuntimeArchiveListing('end\r\r\n', listing + '\r\n'), /unsafe archive entries/);
  assert.throws(() => validateRuntimeArchiveListing('end\r', listing), /unsafe archive entries/);
});

/** 空记录、非普通类型和被 trim 掩盖的尾空白不能因兼容 CRLF 而放行。 */
test('runtime archive inspection rejects blank records and unsafe types without trimming content', () => {
  for (const eol of ['\n', '\r\n']) {
    const names = 'LICENSE.TXT' + eol, listing = '-rw-r--r-- file' + eol;
    for (const unsafe of ['', eol, eol + names, names + eol]) assert.throws(() => validateRuntimeArchiveListing(unsafe, listing), /unsafe archive entries/);
    for (const type of ['l', 'h', 'b', 'c', 'p', 's', '?', ' ']) assert.throws(() => validateRuntimeArchiveListing(names, type + 'rw-r--r-- file' + eol), /unsafe archive entries/);
    assert.throws(() => validateRuntimeArchiveListing(names, ' ' + listing), /unsafe archive entries/);
    assert.throws(() => validateRuntimeArchiveListing(names, listing + eol), /unsafe archive entries/);
  }
});

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
