import fs from 'node:fs/promises';
import path from 'node:path';
import { execFileSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import { assetNames, packIds, releaseRepository, sha256, targets } from '../language-packs/distribution.mjs';
import { prepareRelease } from './release-artifacts.mjs';
import { githubJson, verifyEvidenceRun } from './verify-evidence-run.mjs';

/** 完整资产集合在任何远端写入之前固定；现有不同内容版本绝不删除或覆盖。 */
export async function publishBundle(bundle, github) {
  const expected = [...targets.flatMap(target => packIds.flatMap(id => assetNames(id, target))), 'SHA256SUMS'].sort();
  if (!/^\d+\.\d+\.\d+$/.test(bundle.version) || bundle.tag !== `v${bundle.version}` || !/^[0-9a-f]{40}$/.test(bundle.sourceCommit) || bundle.sourceDirty !== false || JSON.stringify(bundle.assets.map(asset => asset.name).sort()) !== JSON.stringify(expected)) throw new Error('Incomplete immutable release bundle');
  const tagCommit = await github.tagCommit(bundle.tag);
  if (tagCommit && tagCommit !== bundle.sourceCommit) throw new Error('Existing tag points at a different source commit');
  let release = await github.findRelease(bundle.tag);
  if (release && (release.tag_name !== bundle.tag || (!release.draft && tagCommit !== bundle.sourceCommit) || release.target_commitish !== bundle.sourceCommit || !release.prerelease)) throw new Error('Existing release identity differs');
  if (release && (!Array.isArray(release.assets) || new Set(release.assets.map(asset => asset.name)).size !== release.assets.length || release.assets.some(asset => !expected.includes(asset.name)))) throw new Error('Existing release has unexpected assets');
  // 先检查所有已存在资产，再补齐 draft；差异不会留下部分新写入。
  for (const asset of release?.assets ?? []) {
    const local = bundle.assets.find(candidate => candidate.name === asset.name);
    const bytes = await github.readAsset(asset);
    if (bytes.length !== local.size || sha256(bytes) !== local.sha256) throw new Error(`Existing asset differs: ${asset.name}`);
  }
  if (release && !release.draft) {
    if (release.assets.length !== expected.length) throw new Error('Published release cannot be changed');
    return release;
  }
  release ??= await github.createRelease({ tag_name: bundle.tag, target_commitish: bundle.sourceCommit, name: `CodeGraph ${bundle.version}`, body: `Immutable CodeGraph prerelease. Source commit: ${bundle.sourceCommit}. Both native CI targets passed all seven language packages and public CLI acceptance. Verify SHA256SUMS before installation.`, draft: true, prerelease: true, make_latest: 'false' });
  const releaseId = release.id;
  if (!Number.isSafeInteger(releaseId) || releaseId <= 0) throw new Error('Invalid draft release ID');
  for (const local of bundle.assets) {
    if (release.assets.some(asset => asset.name === local.name)) continue;
    const asset = await github.uploadAsset(release, local);
    const bytes = await github.readAsset(asset);
    if (bytes.length !== local.size || sha256(bytes) !== local.sha256) throw new Error(`Uploaded asset differs: ${local.name}`);
  }
  const current = await github.findRelease(bundle.tag);
  if (!current || current.id !== releaseId || current.draft !== true || current.tag_name !== bundle.tag || current.target_commitish !== bundle.sourceCommit || current.prerelease !== true) throw new Error('Draft identity changed during publication');
  if (!Array.isArray(current.assets) || JSON.stringify(current.assets.map(asset => asset.name).sort()) !== JSON.stringify(expected)) throw new Error('Draft assets changed during publication');
  for (const asset of current.assets) {
    const local = bundle.assets.find(candidate => candidate.name === asset.name);
    const bytes = await github.readAsset(asset);
    if (bytes.length !== local.size || sha256(bytes) !== local.sha256) throw new Error(`Draft asset changed: ${asset.name}`);
  }
  const finalTagCommit = await github.tagCommit(bundle.tag);
  if (finalTagCommit && finalTagCommit !== bundle.sourceCommit) throw new Error('Tag changed during publication');
  // 即使最后读回之后 draft 被改目标，缺失 tag 的创建也只能使用已验证的完整提交。
  const published = await github.publishRelease(releaseId, { tag_name: bundle.tag, target_commitish: bundle.sourceCommit, draft: false, prerelease: true, make_latest: 'false' });
  if (published.id !== releaseId || published.draft || !published.prerelease || published.tag_name !== bundle.tag || published.target_commitish !== bundle.sourceCommit || await github.tagCommit(bundle.tag) !== bundle.sourceCommit) throw new Error('Published release identity differs');
  return published;
}

/** 官方 gh 处理认证和资产跳转；读写固定仓库，不接受任意下载主机或 clobber。 */
export function githubAdapter(run = execFileSync) {
  /** 只把明确 404 当成不存在，其余 API 失败仍关闭发布。 */
  function optional(endpoint) {
    try { return githubJson(endpoint); } catch (error) { if (String(error.stderr).includes('(HTTP 404)')) return undefined; throw error; }
  }
  return {
    tagCommit(tag) {
      let object = optional(`/repos/${releaseRepository}/git/ref/tags/${tag}`)?.object;
      for (let depth = 0; object?.type === 'tag' && depth < 8; depth++) object = githubJson(`/repos/${releaseRepository}/git/tags/${object.sha}`).object;
      if (object && object.type !== 'commit') throw new Error('Tag does not resolve to a commit');
      return object?.sha;
    },
    findRelease: tag => optional(`/repos/${releaseRepository}/releases/tags/${tag}`),
    createRelease: body => githubJson(`/repos/${releaseRepository}/releases`, body),
    uploadAsset(release, asset) {
      run('gh', ['release', 'upload', release.tag_name, asset.file, '--repo', releaseRepository], { stdio: 'inherit' });
      const uploaded = githubJson(`/repos/${releaseRepository}/releases/${release.id}`).assets.find(item => item.name === asset.name);
      if (!uploaded) throw new Error('Uploaded asset missing');
      return uploaded;
    },
    readAsset(asset) {
      if (!Number.isSafeInteger(asset.id) || asset.id <= 0 || asset.state !== 'uploaded' || asset.size <= 0 || asset.size > 256 * 1024 * 1024) throw new Error('Invalid GitHub asset identity');
      return run('gh', ['api', `/repos/${releaseRepository}/releases/assets/${asset.id}`, '-H', 'Accept: application/octet-stream'], { maxBuffer: 256 * 1024 * 1024 + 1024 });
    },
    publishRelease(id, body) {
      return JSON.parse(run('gh', ['api', `/repos/${releaseRepository}/releases/${id}`, '--method', 'PATCH', '--input', '-'], { encoding: 'utf8', input: JSON.stringify(body) }));
    },
  };
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const [artifacts, sourceCommit, runId, output] = process.argv.slice(2);
  if (!artifacts || !sourceCommit || !runId || !output) throw new Error('Usage: publish-release.mjs <artifacts> <full-source-commit> <successful-main-ci-run> <empty-output>');
  await verifyEvidenceRun(runId, sourceCommit);
  const bundle = await prepareRelease(path.resolve(artifacts), sourceCommit, path.resolve(output));
  await publishBundle(bundle, githubAdapter());
  console.log(`Published immutable prerelease ${bundle.tag} from ${sourceCommit}`);
}
