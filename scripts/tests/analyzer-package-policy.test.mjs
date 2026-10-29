import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';
import { create as createTar } from 'tar';
import { packIds, sha256, targets } from '../language-packs/distribution.mjs';
import { analyzerPackageFiles, verifyAnalyzerDistribution } from '../language-packs/verify-analyzer-package.mjs';
import { verifyDistribution } from '../language-packs/verify-distribution.mjs';

/** 控制内容与完整运行时文件由固定包身份生成，供所有平台政策用例复用。 */
function fixtureContent(id, target) {
  const content = Object.fromEntries(analyzerPackageFiles(id, target).map(name => [name, 'fixture bytes']));
  content['licenses/fixture-LICENSE'] = 'fixture license';
  content['licenses/codegraph/CodeGraph-dependencies.json'] = '[]';
  content['control/codegraph.json'] = JSON.stringify({ schema_version: 1, pack_id: id, pack_version: '0.1.2', engine_version: '0.1.2', protocol: 'codegraph.analyzer/1' });
  content['control/native.json'] = JSON.stringify({ schema_version: 1, helper_version: '0.1.2', platforms: [], ...({ go: 'go', java: 'java', csharp: 'dotnet' }[id] ? { target_sdk: { kind: { go: 'go', java: 'java', csharp: 'dotnet' }[id] } } : {}) });
  return content;
}

/** 故意不赋予宿主执行位；跨目标归档的权限应由夹具显式表示。 */
async function writePayload(payload, content) {
  for (const [name, bytes] of Object.entries(content)) {
    await fs.mkdir(path.dirname(path.join(payload, name)), { recursive: true });
    await fs.writeFile(path.join(payload, name), bytes, { mode: 0o644 });
  }
}

/** 生成供策略验收使用的真实归档；可执行路径由测试目标显式传入。 */
async function archiveFixture(identity, payload, output, executables) {
  const directory = path.join(output, identity.id, identity.version, identity.target);
  await fs.mkdir(directory, { recursive: true });
  const files = [];
  /** 读取实际夹具字节，但不从宿主 mode 推导另一个目标的执行权限。 */
  async function visit(relative = '') {
    for (const name of (await fs.readdir(path.join(payload, relative))).sort()) {
      const file = path.posix.join(relative, name), absolute = path.join(payload, file);
      const stat = await fs.lstat(absolute);
      if (stat.isDirectory()) await visit(file);
      else {
        assert.ok(stat.isFile());
        const bytes = await fs.readFile(absolute);
        files.push({ path: file, size: bytes.length, sha256: sha256(bytes), executable: executables.has(file) });
      }
    }
  }
  await visit();
  await createTar({ file: path.join(directory, 'pack.tar.gz'), cwd: payload, gzip: true, portable: true, mtime: new Date(0),
    onWriteEntry: entry => { entry.stat.mode = executables.has(entry.path) ? 0o755 : 0o644; },
  }, files.map(file => file.path));
  const archive = await fs.readFile(path.join(directory, 'pack.tar.gz'));
  const manifest = { schemaVersion: 1, ...identity, engineVersion: identity.version, protocol: 'codegraph.analyzer/1', sourceCommit: 'a'.repeat(40), sourceDirty: false, archiveSize: archive.length, archiveSha256: sha256(archive), files };
  await fs.writeFile(path.join(directory, 'manifest.json'), `${JSON.stringify(manifest)}\n`);
  return directory;
}

/** 即使宿主没有 POSIX 执行位，双目标也必须得到有效归档并拒绝缺执行权限的 runtime。 */
test('target archives validate runtime permissions without host POSIX executable bits', async t => {
  const root = await fs.mkdtemp(path.join(os.tmpdir(), 'codegraph cross-target modes-'));
  t.after(() => fs.rm(root, { recursive: true, force: true }));
  for (const target of targets) {
    const suffix = target.includes('windows') ? '.exe' : '';
    const runtime = `runtime/jdk/bin/java${suffix}`;
    const payload = path.join(root, target);
    await writePayload(payload, fixtureContent('java', target));
    assert.equal((await fs.stat(path.join(payload, runtime))).mode & 0o111, 0);
    const executables = new Set([`bin/codegraph-java${suffix}`, runtime]);
    const build = () => archiveFixture({ id: 'java', version: '0.1.2', target }, payload, path.join(root, 'output'), executables);
    const { manifest } = await verifyAnalyzerDistribution(await build());
    assert.equal(manifest.files.find(file => file.path === runtime).executable, true);
    executables.delete(runtime);
    const directory = await build();
    await verifyDistribution(directory);
    await assert.rejects(verifyAnalyzerDistribution(directory), { message: `Missing package dependency or executable: ${runtime}` });
  }
});

/** 字节有效的产物仍须具有唯一固定入口、准确描述及该语言的运行时。 */
test('all fourteen package identities reject generic, foreign and incomplete payloads', async t => {
  const root = await fs.mkdtemp(path.join(os.tmpdir(), 'codegraph package policy-'));
  t.after(() => fs.rm(root, { recursive: true, force: true }));
  const verifiedIdentities = new Set();
  for (const target of targets) for (const id of packIds) {
    const suffix = target.includes('windows') ? '.exe' : '';
    const payload = path.join(root, `${id}-${target}`);
    const files = analyzerPackageFiles(id, target);
    const content = fixtureContent(id, target);
    await writePayload(payload, content);
    const output = path.join(root, 'output');
    const executables = new Set(files.filter(name => name === `bin/codegraph-${id}${suffix}` || name === `bin/node${suffix}` || name === `runtime/jdk/bin/java${suffix}`));
    const build = () => archiveFixture({ id, version: '0.1.2', target }, payload, output, executables);
    await verifyAnalyzerDistribution(await build());
    verifiedIdentities.add(`${id}-${target}`);
    for (const unwanted of [`bin/codegraph${suffix}`, `bin/codegraph-${id === 'go' ? 'rust' : 'go'}${suffix}`, `runtime/codegraph-python${suffix}`, `bin/codegraph-process-owner${suffix}`, `runtime/analyzer/codegraph-process-owner${suffix}`, 'bin/unrelated-tool']) {
      await fs.mkdir(path.dirname(path.join(payload, unwanted)), { recursive: true });
      await fs.writeFile(path.join(payload, unwanted), 'unwanted executable');
      executables.add(unwanted);
      await assert.rejects(verifyAnalyzerDistribution(await build()), /Unexpected package file/);
      await fs.rm(path.join(payload, unwanted));
      executables.delete(unwanted);
    }
    for (const required of files) {
      await fs.rm(path.join(payload, required));
      await assert.rejects(verifyAnalyzerDistribution(await build()), /Missing package dependency/);
      await fs.writeFile(path.join(payload, required), content[required], { mode: 0o644 });
    }
    const descriptor = path.join(payload, 'control/codegraph.json');
    for (const bad of [content['control/codegraph.json'].replace('0.1.2', '9.9.9'), content['control/codegraph.json'].replace('{', '{"extra":true,'), content['control/codegraph.json'].replace('{', '{"pack_id":"wrong",')]) {
      await fs.writeFile(descriptor, bad);
      await assert.rejects(verifyAnalyzerDistribution(await build()), /descriptor/);
    }
  }
  assert.equal(verifiedIdentities.size, 14);
});
