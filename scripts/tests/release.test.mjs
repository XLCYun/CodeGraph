import test from 'node:test';
import assert from 'node:assert/strict';
import { publishBundle, githubAdapter } from '../release/publish-release.mjs';
import { verifyEvidenceRun } from '../release/verify-evidence-run.mjs';
import { assetNames, packIds, releaseRepository, sha256, targets } from '../language-packs/distribution.mjs';

/** 完整双平台三十一资产和内存 GitHub，覆盖实际发布顺序与不可变规则。 */
function fixture() {
  const assets = [...targets.flatMap(target => ['codegraph', ...packIds].flatMap(id => assetNames(id, target))), 'SHA256SUMS'].map(name => ({ name, bytes: Buffer.from(name), size: Buffer.byteLength(name), sha256: sha256(Buffer.from(name)) }));
  const bundle = { version: '0.1.0', tag: 'v0.1.0', sourceCommit: 'a'.repeat(40), sourceDirty: false, assets };
  let release, tag;
  const objects = new Map(), events = [];
  const github = {
    tagCommit: async () => tag,
    findRelease: async () => release,
    createRelease: async body => { events.push('create'); release = { ...body, id: 1, assets: [] }; return release; },
    uploadAsset: async (_release, local) => { events.push('upload'); const asset = { id: objects.size + 1, name: local.name }; objects.set(local.name, local.bytes); release.assets.push(asset); return asset; },
    readAsset: async asset => objects.get(asset.name),
    publishRelease: async (id, body) => { events.push('publish'); assert.equal(id, release.id); assert.equal(objects.size, 31); Object.assign(release, body); tag ??= release.target_commitish; release.draft = false; return release; },
  };
  return { bundle, github, events, objects, release: () => release, setTag: value => { tag = value; } };
}

test('all seven packages and both CLIs are required before remote mutation', async () => {
  const f = fixture();
  f.bundle.assets.splice(3, 1);
  await assert.rejects(publishBundle(f.bundle, f.github), /Incomplete/);
  assert.deepEqual(f.events, []);
});

test('successful immutable prerelease verifies uploaded bytes and reuses exact versions', async () => {
  const f = fixture();
  await publishBundle(f.bundle, f.github);
  assert.equal(f.release().draft, false);
  assert.equal(f.release().prerelease, true);
  assert.equal(f.release().make_latest, 'false');
  assert.equal(f.events.filter(event => event === 'upload').length, 31);
  const original = [...f.events];
  await publishBundle(f.bundle, f.github);
  assert.deepEqual(f.events, original);
  f.objects.set(f.bundle.assets[0].name, Buffer.from('different'));
  await assert.rejects(publishBundle(f.bundle, f.github), /differs/);
  assert.deepEqual(f.events, original);
});

test('corrupt uploads leave drafts and changed tags fail before writes', async () => {
  const f = fixture();
  f.github.readAsset = async () => Buffer.from('corrupt');
  await assert.rejects(publishBundle(f.bundle, f.github), /differs/);
  assert.deepEqual(f.events, ['create', 'upload']);
  assert.equal(f.release().draft, true);
  const tagged = fixture();
  tagged.setTag('b'.repeat(40));
  await assert.rejects(publishBundle(tagged.bundle, tagged.github), /different source/);
  assert.deepEqual(tagged.events, []);
});

test('published missing assets and draft unexpected assets cannot be silently repaired', async () => {
  const f = fixture();
  await publishBundle(f.bundle, f.github);
  f.release().assets.pop();
  const original = [...f.events];
  await assert.rejects(publishBundle(f.bundle, f.github), /cannot be changed/);
  assert.deepEqual(f.events, original);
  f.release().draft = true;
  f.release().assets.push({ name: 'other' });
  await assert.rejects(publishBundle(f.bundle, f.github), /unexpected/);
  assert.deepEqual(f.events, original);
});

test('release evidence rejects failed, partial, PR, different-source and foreign runs', async () => {
  const source = 'a'.repeat(40);
  const goodRun = { repository: { full_name: releaseRepository }, head_repository: { full_name: releaseRepository }, path: '.github/workflows/ci.yml', event: 'push', head_branch: 'main', head_sha: source, status: 'completed', conclusion: 'success', run_attempt: 1 };
  const goodJobs = ['Complete CI', ...targets.map(target => `Native acceptance (${target})`)].map(name => ({ name, status: 'completed', conclusion: 'success' }));
  const api = (run = goodRun, jobs = goodJobs) => async endpoint => endpoint.includes('/jobs?') ? { total_count: jobs.length, jobs } : run;
  assert.equal(await verifyEvidenceRun('123', source, api()), goodRun);
  for (const mutation of [{ conclusion: 'failure' }, { status: 'in_progress' }, { event: 'pull_request' }, { head_branch: 'feature' }, { head_sha: 'b'.repeat(40) }, { path: '.github/workflows/release.yml' }, { head_repository: { full_name: 'other/repo' } }]) await assert.rejects(verifyEvidenceRun('123', source, api({ ...goodRun, ...mutation })), /exact source commit/);
  await assert.rejects(verifyEvidenceRun('123', source, api(goodRun, goodJobs.slice(0, 2))), /Missing complete/);
  await assert.rejects(verifyEvidenceRun('123', source, api(goodRun, goodJobs.map(job => ({ ...job, conclusion: 'skipped' })))), /skipped/);
  await assert.rejects(verifyEvidenceRun('../123', source, api()), /Invalid/);
});

/** draft 尚未创建 tag 时可恢复上传，但必须继续验证已有字节和精确 source commit。 */
test('a verified partial draft can resume without a pre-created tag', async () => {
  const f = fixture();
  const upload = f.github.uploadAsset;
  let attempts = 0;
  f.github.uploadAsset = async (release, local) => { if (++attempts === 2) throw new Error('network stopped'); return upload(release, local); };
  await assert.rejects(publishBundle(f.bundle, f.github), /network stopped/);
  assert.equal(f.release().draft, true);
  f.github.uploadAsset = upload;
  await publishBundle(f.bundle, f.github);
  assert.equal(f.release().draft, false);
  assert.equal(f.events.filter(event => event === 'create').length, 1);
});

/** 首发没有 tag 时，上传期间变更 draft 身份也必须在公开发布之前失败。 */
test('draft identity drift during upload is rejected before publication without a tag', async () => {
  for (const mutation of [{ target_commitish: 'b'.repeat(40) }, { id: 2 }, { tag_name: 'v9.0.0' }, { prerelease: false }]) {
    const f = fixture(), find = f.github.findRelease;
    let reads = 0;
    f.github.findRelease = async tag => {
      const release = await find(tag);
      if (++reads === 2) Object.assign(release, mutation);
      return release;
    };
    await assert.rejects(publishBundle(f.bundle, f.github), /Draft identity changed/);
    assert.equal(f.events.includes('publish'), false, 'identity drift must fail before publication');
    assert.equal(f.release().draft, true);
    assert.equal(await f.github.tagCommit(f.bundle.tag), undefined);
  }
});

/** 最终读回后仍可能被改目标，发布写入必须显式绑定已验证 tag 与完整 SHA。 */
test('publication pins the exact commit even if the draft target changes after the final read', async () => {
  const f = fixture(), publish = f.github.publishRelease;
  f.github.publishRelease = async (id, body) => {
    Object.assign(f.release(), { target_commitish: 'b'.repeat(40), tag_name: 'v9.0.0' });
    await publish(id, body);
    assert.deepEqual(body, { tag_name: f.bundle.tag, target_commitish: f.bundle.sourceCommit, draft: false, prerelease: true, make_latest: 'false' });
    return f.release();
  };
  await publishBundle(f.bundle, f.github);
  assert.equal(await f.github.tagCommit(f.bundle.tag), f.bundle.sourceCommit);
  assert.equal(f.release().tag_name, f.bundle.tag);
});

/** target_commitish 对已存在 tag 无效，所以最终 tag 指向漂移也必须在 publish 前拒绝。 */
test('a tag created at a different commit during upload prevents publication', async () => {
  const f = fixture();
  let probes = 0;
  f.github.tagCommit = async () => ++probes === 1 ? undefined : 'b'.repeat(40);
  await assert.rejects(publishBundle(f.bundle, f.github), /Tag changed/);
  assert.equal(f.events.includes('publish'), false);
  assert.equal(f.release().draft, true);
});

/** 直接检验实际 gh adapter 的 PATCH 字节，内存服务不会掩盖字段未发送的错误。 */
test('GitHub publish adapter sends the exact target commit and tag in its PATCH body', async () => {
  const calls = [], body = { tag_name: 'v0.1.0', target_commitish: 'a'.repeat(40), draft: false, prerelease: true, make_latest: 'false' };
  const github = githubAdapter((command, args, options) => {
    calls.push({ command, args, options });
    return JSON.stringify({ id: 17, ...body });
  });
  assert.deepEqual(await github.publishRelease(17, body), { id: 17, ...body });
  assert.equal(calls.length, 1);
  assert.equal(calls[0].command, 'gh');
  assert.deepEqual(calls[0].args, ['api', `/repos/${releaseRepository}/releases/17`, '--method', 'PATCH', '--input', '-']);
  assert.deepEqual(JSON.parse(calls[0].options.input), body);
});

/** artifact 存在不能冒充测试成功；证据必须逐项匹配身份、语义清单与归档摘要。 */
test('partial or stale local evidence is rejected before preparing release assets', async () => {
  const { validateVerificationEvidence } = await import('../release/release-artifacts.mjs');
  const identity = { version: '0.1.0', engineVersion: '0.1.0', protocol: 'codegraph.analyzer/1', sourceCommit: 'a'.repeat(40) };
  const target = targets[0], receipts = [{ id: 'rust', archiveSha256: 'b'.repeat(64) }];
  const inventory = [{ name: 'required::semantic', platforms: ['darwin', 'win32'] }];
  const evidence = { schemaVersion: 1, status: 'passed', ...identity, sourceDirty: false, target, languages: packIds, semanticTests: ['required::semantic'], commands: Array.from({ length: 10 }, () => ({ args: ['scripts/language-packs/verify-public-cli.mjs'], outcome: 'passed' })), artifacts: receipts };
  assert.equal(validateVerificationEvidence(evidence, identity, target, receipts, inventory), evidence);
  for (const mutation of [{ status: 'failed' }, { sourceDirty: true }, { sourceCommit: 'c'.repeat(40) }, { target: targets[1] }, { engineVersion: '9.0.0' }, { protocol: 'other/1' }, { semanticTests: [] }, { languages: packIds.slice(0, 3) }, { commands: [] }, { artifacts: [] }, { commands: evidence.commands.map(command => ({ ...command, outcome: 'skipped' })) }]) assert.throws(() => validateVerificationEvidence({ ...evidence, ...mutation }, identity, target, receipts, inventory), /complete successful/);
});
