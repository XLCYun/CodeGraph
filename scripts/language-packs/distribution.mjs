import fs from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { execFileSync } from 'node:child_process';
import { createHash } from 'node:crypto';

export const repository = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../..');
export const targets = ['aarch64-apple-darwin', 'x86_64-pc-windows-msvc'];
export const packIds = ['rust', 'typescript-javascript', 'python', 'java', 'go', 'csharp', 'c-cpp'];
export const limits = Object.freeze({ archive: 256 * 1024 * 1024, unpacked: 1024 * 1024 * 1024, manifest: 2 * 1024 * 1024 });
export const releaseRepository = 'XLCYun/CodeGraph';

/** 统一读取版本；Cargo、Node 工程和七包必须与同一公共协议绑定。 */
export async function releaseIdentity() {
  const catalog = JSON.parse(await fs.readFile(path.join(repository, 'resources/language-support-packs.json'), 'utf8'));
  const cargo = await fs.readFile(path.join(repository, 'Cargo.toml'), 'utf8');
  const node = JSON.parse(await fs.readFile(path.join(repository, 'package.json'), 'utf8'));
  const version = cargo.match(/^version = "(\d+\.\d+\.\d+)"$/m)?.[1];
  if (!version || node.version !== version || catalog.engineVersion !== version || catalog.protocol !== 'codegraph.analyzer/1' || catalog.repository !== releaseRepository || JSON.stringify(Object.keys(catalog.packs).sort()) !== JSON.stringify([...packIds].sort()) || Object.values(catalog.packs).some(pack => pack.version !== version)) throw new Error('Engine/package version or protocol mismatch');
  const sourceCommit = execFileSync('git', ['rev-parse', 'HEAD'], { cwd: repository, encoding: 'utf8' }).trim();
  const sourceDirty = execFileSync('git', ['status', '--porcelain', '--untracked-files=all'], { cwd: repository, encoding: 'utf8' }).trim().length !== 0;
  return { version, engineVersion: version, protocol: catalog.protocol, sourceCommit, sourceDirty };
}

/** 目标限定在真实宿主；不允许交叉构建成功冒充目标平台运行验收。 */
export function nativeTarget() {
  const target = { 'darwin-arm64': targets[0], 'win32-x64': targets[1] }[`${process.platform}-${process.arch}`];
  if (!target) throw new Error('CodeGraph requires macOS ARM64 or Windows x64');
  return target;
}

/** 使用完整字节计算统一 SHA-256。 */
export function sha256(bytes) { return createHash('sha256').update(bytes).digest('hex'); }

/** 固定资产名；一个 engine tag 同时包含双平台和七种语言。 */
export function assetNames(id, target) {
  if (!packIds.includes(id) || !targets.includes(target)) throw new Error('Invalid distribution identity');
  return [`${id}-${target}-pack.tar.gz`, `${id}-${target}-manifest.json`];
}
