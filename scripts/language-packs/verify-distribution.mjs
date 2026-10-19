import fs from 'node:fs/promises';
import path from 'node:path';
import { createReadStream } from 'node:fs';
import { createHash } from 'node:crypto';
import { list, extract } from 'tar';
import { limits, packIds, targets, sha256 } from './distribution.mjs';

/** 路径必须在两平台都表示普通相对文件，拒绝设备名、歧义和目录穿越。 */
export function safeFilePath(file) {
  return typeof file === 'string' && file.length > 0 && !/[\\:\x00-\x1f\x7f]/.test(file) && file.split('/').every(part => part && part !== '.' && part !== '..' && !/[. ]$/.test(part) && !/^(con|prn|aux|nul|com[1-9]|lpt[1-9])(?:\.|$)/i.test(part));
}

/** 清单身份、精确字段、大小与权限必须先于解包验证；原字节禁止重复 JSON key。 */
export function parseManifest(bytes, expected = {}) {
  if (bytes.length > limits.manifest) throw new Error('Manifest exceeds size limit');
  const manifest = JSON.parse(bytes.toString('utf8'));
  if (`${JSON.stringify(manifest)}\n` !== bytes.toString('utf8')) throw new Error('Manifest must use canonical JSON without duplicate keys');
  const fields = ['schemaVersion', 'id', 'version', 'target', 'engineVersion', 'protocol', 'sourceCommit', 'sourceDirty', 'archiveSize', 'archiveSha256', 'files'];
  if (JSON.stringify(Object.keys(manifest).sort()) !== JSON.stringify(fields.sort()) || manifest.schemaVersion !== 1 || ![...packIds, 'codegraph'].includes(manifest.id) || !targets.includes(manifest.target) || !/^\d+\.\d+\.\d+$/.test(manifest.version) || manifest.version !== manifest.engineVersion || manifest.protocol !== 'codegraph.analyzer/1' || !/^[0-9a-f]{40}$/.test(manifest.sourceCommit) || typeof manifest.sourceDirty !== 'boolean' || !Number.isSafeInteger(manifest.archiveSize) || manifest.archiveSize <= 0 || manifest.archiveSize > limits.archive || !/^[0-9a-f]{64}$/.test(manifest.archiveSha256) || !Array.isArray(manifest.files) || !manifest.files.length) throw new Error('Invalid distribution manifest');
  for (const [key, value] of Object.entries(expected)) if (manifest[key] !== value) throw new Error(`Manifest ${key} mismatch`);
  let size = 0;
  const names = new Set();
  for (const file of manifest.files) {
    if (JSON.stringify(Object.keys(file).sort()) !== JSON.stringify(['path', 'size', 'sha256', 'executable'].sort()) || !safeFilePath(file.path) || names.has(file.path.toLowerCase()) || !Number.isSafeInteger(file.size) || file.size < 0 || !/^[0-9a-f]{64}$/.test(file.sha256) || typeof file.executable !== 'boolean') throw new Error('Invalid manifest file');
    names.add(file.path.toLowerCase());
    size += file.size;
    if (size > limits.unpacked) throw new Error('Unpacked size exceeds limit');
  }
  const sorted = [...names].sort();
  if (sorted.some((name, index) => index && name.startsWith(`${sorted[index - 1]}/`))) throw new Error('Manifest file/directory collision');
  return manifest;
}

/** 流式复核归档摘要后逐项读取 tar；链接、重复、额外文件及权限漂移全部拒绝。 */
export async function verifyDistribution(directory, expected = {}) {
  const manifestPath = path.join(directory, 'manifest.json');
  if ((await fs.stat(manifestPath)).size > limits.manifest) throw new Error('Manifest exceeds size limit');
  const body = await fs.readFile(manifestPath);
  const manifest = parseManifest(body, expected);
  const archive = path.join(directory, 'pack.tar.gz');
  if ((await fs.stat(archive)).size !== manifest.archiveSize) throw new Error('Archive size mismatch');
  const hash = createHash('sha256');
  for await (const chunk of createReadStream(archive)) hash.update(chunk);
  if (hash.digest('hex') !== manifest.archiveSha256) throw new Error('Archive hash mismatch');
  const files = new Map(manifest.files.map(file => [file.path, file]));
  const seen = new Set();
  const failures = [];
  await list({ file: archive, strict: true, onReadEntry(entry) {
    const file = files.get(entry.path);
    if (!file || seen.has(entry.path) || entry.type !== 'File' || entry.size !== file.size || (entry.mode & 0o7777) !== (file.executable ? 0o755 : 0o644)) failures.push(`Invalid archive entry: ${entry.path}`);
    seen.add(entry.path);
    const hash = createHash('sha256');
    let size = 0;
    entry.on('data', chunk => { hash.update(chunk); size += chunk.length; });
    entry.on('end', () => { if (size !== file?.size || hash.digest('hex') !== file?.sha256) failures.push(`File hash mismatch: ${entry.path}`); });
  } });
  if (seen.size !== files.size || failures.length) throw new Error(`Archive file verification failed: ${failures.join('; ') || 'missing files'}`);
  return { manifest, body, archive };
}

/** 只向新目录解包已验证字节，并重新验证落盘内容及 POSIX 执行权限。 */
export async function verifyAndExtract(directory, destination, expected = {}) {
  const result = await verifyDistribution(directory, expected);
  await fs.mkdir(destination, { recursive: false });
  try {
    await extract({ file: result.archive, cwd: destination, strict: true, noChmod: false, preserveOwner: false });
    for (const file of result.manifest.files) {
      const name = path.join(destination, file.path);
      const stat = await fs.lstat(name);
      if (!stat.isFile() || stat.size !== file.size || sha256(await fs.readFile(name)) !== file.sha256 || (process.platform !== 'win32' && (stat.mode & 0o777) !== (file.executable ? 0o755 : 0o644))) throw new Error(`Extracted file mismatch: ${file.path}`);
    }
    return result;
  } catch (error) { await fs.rm(destination, { recursive: true, force: true }); throw error; }
}
