import fs from 'node:fs/promises';
import path from 'node:path';
import { nativeTarget, packIds, releaseIdentity, repository, sha256 } from './distribution.mjs';
import { verifyAnalyzerDistribution } from './verify-analyzer-package.mjs';

/** 只在七包实际公开验收之后保存本机候选；它明确不替代双平台完整 CI 或发布证据。 */
export async function writeLocalCandidates(artifacts) {
  const identity = await releaseIdentity(), target = nativeTarget();
  if (identity.sourceDirty) throw new Error('Local candidates require committed clean source');
  const candidates = [], assets = [];
  for (const id of packIds) {
    const directory = path.join(artifacts, id, identity.version, target);
    const { manifest, body, archive } = await verifyAnalyzerDistribution(directory, { id, version: identity.version, target, sourceCommit: identity.sourceCommit, sourceDirty: false });
    const root = path.join(artifacts, 'smoke-installed', id, identity.version, target);
    // 交接的实际安装文件仍须与不可变归档清单逐字一致。
    for (const file of manifest.files) if (sha256(await fs.readFile(path.join(root, file.path))) !== file.sha256) throw new Error(`Installed candidate changed: ${id}/${file.path}`);
    candidates.push({ id, version: identity.version, engineVersion: identity.engineVersion, protocol: identity.protocol, target, root });
    assets.push({ id, archive, archiveSha256: manifest.archiveSha256, archiveSize: manifest.archiveSize, manifest: path.join(directory, 'manifest.json'), manifestSha256: sha256(body), installed: root, fileCount: manifest.files.length, unpackedSize: manifest.files.reduce((sum, file) => sum + file.size, 0) });
  }
  const pins = {};
  for (const name of ['resources/language-support-packs.json', 'resources/native-toolchains.json', 'analyzers/java/dependencies.lock.json', 'analyzers/rust/Cargo.lock', 'analyzers/python/Cargo.lock', 'Cargo.lock']) pins[name] = sha256(await fs.readFile(path.join(repository, name)));
  const receipt = { schemaVersion: 1, kind: 'local-direct-public-acceptance', releaseEvidence: false, ...identity, target, assets, pins, acceptanceLogs: path.join(artifacts, 'public-acceptance') };
  await fs.writeFile(path.join(artifacts, 'candidate-receipt.json'), `${JSON.stringify(receipt, null, 2)}\n`, { flag: 'wx' });
  await fs.writeFile(path.join(artifacts, 'candidates.json'), `${JSON.stringify(candidates, null, 2)}\n`, { flag: 'wx' });
}
