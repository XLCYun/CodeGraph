import fs from 'node:fs/promises';
import path from 'node:path';
import os from 'node:os';
import { execFileSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { isDeepStrictEqual } from 'node:util';
import { repository, targets } from './distribution.mjs';
import { systemTar } from './system-tar.mjs';
import { safeFilePath } from './verify-distribution.mjs';

/** 固定实际 RID、版本、官方出处和完整 SHA512，禁止 NuGet 隐式选择补丁。 */
export function validateRuntimePack(entry, target) {
  const rid = { 'aarch64-apple-darwin': 'osx-arm64', 'x86_64-pc-windows-msvc': 'win-x64' }[target];
  if (!rid || entry?.rid !== rid || entry.id !== `Microsoft.NETCore.App.Runtime.${rid}` || entry.version !== '10.0.0' || entry.algorithm !== 'sha512' || !/^[0-9a-f]{128}$/.test(entry.digest ?? '') || entry.url !== `https://api.nuget.org/v3-flatcontainer/microsoft.netcore.app.runtime.${rid}/10.0.0/microsoft.netcore.app.runtime.${rid}.10.0.0.nupkg` || !entry.metadata_source?.startsWith('https://api.nuget.org/v3/catalog0/data/')) throw new Error('Invalid fixed .NET runtime pack');
  return entry;
}

/** 只识别 LF/CRLF 记录分隔符与一个末尾换行，路径空白和类型字段不能被 trim 掩盖。 */
export function validateRuntimeArchiveListing(namesOutput, listingOutput) {
  const names = namesOutput.replace(/\r?\n$/, '').split(/\r?\n/);
  const listing = listingOutput.replace(/\r?\n$/, '').split(/\r?\n/);
  if (names.some(name => !safeFilePath(name.replace(/\/$/, ''))) || listing.some(line => !['-', 'd'].includes(line[0]))) throw new Error('Runtime pack contains unsafe archive entries');
}

/** 优先复用既有 NuGet 缓存；只把核验过的固定 nupkg 暂存为离线恢复源。 */
export async function prepareRuntimePack(target, temporary, environment = process.env) {
  const manifest = JSON.parse(await fs.readFile(path.join(repository, 'resources/native-toolchains.json'), 'utf8'));
  for (const supported of targets) validateRuntimePack(manifest.runtime_packs?.[supported], supported);
  const entry = manifest.runtime_packs[target], filename = `${entry.id.toLowerCase()}.${entry.version}.nupkg`;
  const cache = path.join(environment.NUGET_PACKAGES || path.join(os.homedir(), '.nuget/packages'), entry.id.toLowerCase(), entry.version, filename);
  let bytes, origin = cache;
  try { bytes = await fs.readFile(cache); } catch (error) { if (error.code !== 'ENOENT') throw error; }
  if (!bytes) {
    const response = await fetch(entry.url, { signal: AbortSignal.timeout(180_000) });
    if (!response.ok) throw new Error(`Runtime pack download failed: ${response.status}`);
    bytes = Buffer.from(await response.arrayBuffer());
    origin = entry.url;
  }
  if (createHash('sha512').update(bytes).digest('hex') !== entry.digest) throw new Error('Runtime pack checksum mismatch');
  const source = path.join(temporary, 'runtime-source');
  await fs.mkdir(source);
  const archive = path.join(source, filename);
  await fs.writeFile(archive, bytes, { flag: 'wx' });
  const names = execFileSync(systemTar(), ['-tf', archive], { encoding: 'utf8' });
  const listing = execFileSync(systemTar(), ['-tvf', archive], { encoding: 'utf8' });
  validateRuntimeArchiveListing(names, listing);
  const notices = path.join(temporary, 'runtime-notices');
  await fs.mkdir(notices);
  execFileSync(systemTar(), ['-xf', archive, '-C', notices, 'LICENSE.TXT', 'THIRD-PARTY-NOTICES.TXT']);
  console.log(`Fixed runtime pack: ${JSON.stringify({ ...entry, sha256: createHash('sha256').update(bytes).digest('hex'), origin })}`);
  return { entry, source, notices, archive };
}

/** 自包含输出只说明随包的固定框架；SDK 省略无须参与解析的 rollForward 是合法形状。 */
export function validatePublishedRuntimeConfig(config) {
  if (config?.tfm !== 'net10.0' || !isDeepStrictEqual(config.includedFrameworks, [{ name: 'Microsoft.NETCore.App', version: '10.0.0' }]) || Object.hasOwn(config, 'framework') || Object.hasOwn(config, 'frameworks') || config.rollForward !== undefined && config.rollForward !== 'Disable') throw new Error('C# publish is not the fixed self-contained runtime');
}

/** 将完整发布的 CLR/BCL 与已核验 nupkg 逐字比较，缓存或全局运行时不能替换分发字节。 */
export async function verifyPublishedRuntime(publish, runtime, temporary) {
  const reference = path.join(temporary, 'runtime-byte-reference');
  await fs.mkdir(reference);
  const prefix = `runtimes/${runtime.entry.rid}`;
  execFileSync(systemTar(), ['-xf', runtime.archive, '-C', reference, `${prefix}/native`, `${prefix}/lib/net10.0`]);
  let checked = 0;
  for (const part of ['native', 'lib/net10.0']) for (const name of await fs.readdir(path.join(reference, prefix, part))) {
    if (name.endsWith('.json') || name.endsWith('.pdb')) continue;
    const source = path.join(reference, prefix, part, name);
    if (!(await fs.stat(source)).isFile()) throw new Error('Unexpected runtime reference directory');
    const expected = await fs.readFile(source), actual = await fs.readFile(path.join(publish, name));
    if (!actual.equals(expected)) throw new Error(`Published runtime byte mismatch: ${name}`);
    checked++;
  }
  const config = JSON.parse(await fs.readFile(path.join(publish, 'codegraph-csharp.runtimeconfig.json'), 'utf8')).runtimeOptions;
  validatePublishedRuntimeConfig(config);
  console.log(`Verified complete fixed CLR/BCL bytes: ${checked}`);
}
