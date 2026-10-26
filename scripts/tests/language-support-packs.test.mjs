import { test } from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';
import { createHash } from 'node:crypto';
import { packageDirectory } from '../language-packs/build-language-support-packs.mjs';

test('pack identity cannot escape its output directory', async () => {
  await assert.rejects(packageDirectory({ id: 'rust', version: '../escape', target: 'aarch64-apple-darwin' }, path.join(os.tmpdir(), 'missing-payload'), path.join(os.tmpdir(), 'missing-output')), /identity/);
});

test('pack artifact binds identity, bytes and executable metadata to the complete payload', async () => {
  const root = await fs.mkdtemp(path.join(os.tmpdir(), 'narrant-pack-test-'));
  try {
    await fs.mkdir(path.join(root, 'input/bin'), { recursive: true });
    await fs.writeFile(path.join(root, 'input/bin/codegraph-rust'), 'analyzer fixture');
    const result = await packageDirectory({ id: 'rust', version: '0.1.2', target: 'aarch64-apple-darwin' }, path.join(root, 'input'), path.join(root, 'output'));
    const manifest = JSON.parse(await fs.readFile(path.join(result, 'manifest.json')));
    assert.equal(manifest.id, 'rust');
    assert.equal(manifest.version, '0.1.2');
    assert.deepEqual(manifest.files.map(file => [file.path, file.executable]), [['bin/codegraph-rust', true]]);
    const bytes = await fs.readFile(path.join(result, 'pack.tar.gz'));
    assert.equal(manifest.archiveSize, bytes.length);
    assert.equal(manifest.archiveSha256, createHash('sha256').update(bytes).digest('hex'));
    await fs.utimes(path.join(root, 'input/bin/codegraph-rust'), new Date(), new Date());
    const second = await packageDirectory({ id: 'rust', version: '0.1.2', target: 'aarch64-apple-darwin' }, path.join(root, 'input'), path.join(root, 'repeat'));
    assert.deepEqual(await fs.readFile(path.join(second, 'pack.tar.gz')), bytes);
  } finally { await fs.rm(root, { recursive: true, force: true }); }
});

test('Python pack preserves its native executable, fixed control file and standard-library license', async () => {
  const root = await fs.mkdtemp(path.join(os.tmpdir(), 'codegraph-python-engine-pack-test-'));
  try {
    for (const directory of ['bin', 'control', 'licenses']) await fs.mkdir(path.join(root, 'payload', directory), { recursive: true });
    await fs.writeFile(path.join(root, 'payload/bin/codegraph-python'), 'fixed native Python fixture');
    await fs.writeFile(path.join(root, 'payload/control/native.json'), '{}');
    await fs.writeFile(path.join(root, 'payload/licenses/typeshed-LICENSE'), 'typeshed license fixture');
    const result = await packageDirectory({ id: 'python', version: '0.1.2', target: 'aarch64-apple-darwin' }, path.join(root, 'payload'), path.join(root, 'output'));
    const manifest = JSON.parse(await fs.readFile(path.join(result, 'manifest.json')));
    assert.equal(manifest.id, 'python');
    assert.deepEqual(manifest.files.map(file => [file.path, file.executable]), [['bin/codegraph-python', true], ['control/native.json', false], ['licenses/typeshed-LICENSE', false]]);
    assert.equal(manifest.files.find(file => file.path === 'control/native.json').size, 2);
  } finally { await fs.rm(root, { recursive: true, force: true }); }
});

/** 实际归档必须通过完整验收，任何摘要、身份或执行位漂移都不能解包。 */
test('distribution verifies real bytes before extraction and rejects tampering and unsafe manifests', async () => {
  const { verifyAndExtract, verifyDistribution, parseManifest, safeFilePath } = await import('../language-packs/verify-distribution.mjs');
  const root = await fs.mkdtemp(path.join(os.tmpdir(), 'codegraph-distribution-'));
  try {
    await fs.mkdir(path.join(root, 'payload/bin'), { recursive: true });
    await fs.writeFile(path.join(root, 'payload/bin/codegraph-rust'), 'binary fixture');
    const directory = await packageDirectory({ id: 'rust', version: '0.1.2', target: 'aarch64-apple-darwin' }, path.join(root, 'payload'), path.join(root, 'artifacts'));
    const { manifest } = await verifyAndExtract(directory, path.join(root, 'extracted'));
    assert.equal(await fs.readFile(path.join(root, 'extracted/bin/codegraph-rust'), 'utf8'), 'binary fixture');
    assert.equal(manifest.engineVersion, '0.1.2');
    assert.equal(manifest.protocol, 'codegraph.analyzer/1');
    for (const mutation of [{ version: '1.1.0' }, { engineVersion: '9.0.0' }, { archiveSize: 256 * 1024 * 1024 + 1 }, { sourceCommit: 'main' }, { sourceDirty: null }, { files: [{ ...manifest.files[0], size: 1024 * 1024 * 1024 + 1 }] }, { files: [...manifest.files, manifest.files[0]] }]) {
      assert.throws(() => parseManifest(Buffer.from(`${JSON.stringify({ ...manifest, ...mutation })}\n`)));
    }
    for (const name of ['../escape', '/root', 'C:/root', 'bin\\run.exe', 'con.txt', 'bin/end.', 'a//b']) assert.equal(safeFilePath(name), false, name);
    const changed = { ...manifest, files: [{ ...manifest.files[0], executable: false }] };
    await fs.writeFile(path.join(directory, 'manifest.json'), `${JSON.stringify(changed)}\n`);
    await assert.rejects(verifyDistribution(directory), /Invalid archive entry/);
    await fs.writeFile(path.join(directory, 'manifest.json'), `${JSON.stringify(manifest)}\n`);
    await fs.appendFile(path.join(directory, 'pack.tar.gz'), 'corrupt');
    await assert.rejects(verifyAndExtract(directory, path.join(root, 'rejected')), /size mismatch/);
    await assert.rejects(fs.stat(path.join(root, 'rejected')), { code: 'ENOENT' });
  } finally { await fs.rm(root, { recursive: true, force: true }); }
});
