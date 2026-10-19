import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';
import { preparePublicProject, assertPublicProjectUnchanged } from '../language-packs/verify-public-cli.mjs';

/** Java 的公开验收必须提供 helper 支持的声明式源码根与确切目标 release。 */
test('public Java project declares its source root and target release', async t => {
  const root = await fs.mkdtemp(path.join(os.tmpdir(), 'codegraph java fixture-'));
  t.after(() => fs.rm(root, { recursive: true, force: true }));
  const project = await preparePublicProject('java', root);
  const metadata = JSON.parse(await fs.readFile(path.join(root, 'narrant-java.json'), 'utf8'));
  assert.deepEqual(metadata, { schema: 'java-static-v1', release: '17', source_roots: [{ path: 'src', role: 'main' }] });
  assert.deepEqual(project.files, ['src/Sample.java']);
  assert.ok((await fs.readFile(path.join(root, project.files[0]), 'utf8')).includes('target()'));
  assert.equal(project.prerequisite, 'narrant-java.json');
  await assertPublicProjectUnchanged(project);
});

/** CDB 绑定实际临时仓库，声明驱动、标准和输入；工厂提供唯一批准的宿主 SDK。 */
test('public C++ project has a concrete static compilation database', async t => {
  const root = await fs.mkdtemp(path.join(os.tmpdir(), 'codegraph cpp fixture-'));
  t.after(() => fs.rm(root, { recursive: true, force: true }));
  const project = await preparePublicProject('c-cpp', root);
  const database = JSON.parse(await fs.readFile(path.join(root, 'compile_commands.json'), 'utf8'));
  assert.deepEqual(database, [{ directory: await fs.realpath(root), file: 'main.cpp', arguments: ['clang++', '-std=c++20', '-c', 'main.cpp'] }]);
  assert.deepEqual(project.files, ['main.cpp']);
  assert.equal(project.prerequisite, 'compile_commands.json');
  assert.ok((await fs.stat(path.join(database[0].directory, database[0].file))).isFile());
  await assertPublicProjectUnchanged(project);
});

/** 元数据缺失或修改都必须破坏只读验收，不能只检查选中的源码文件。 */
test('public project snapshots detect missing or changed required metadata', async t => {
  const root = await fs.mkdtemp(path.join(os.tmpdir(), 'codegraph metadata snapshot-'));
  t.after(() => fs.rm(root, { recursive: true, force: true }));
  for (const id of ['java', 'c-cpp']) {
    const project = await preparePublicProject(id, path.join(root, id));
    const file = path.join(project.root, project.prerequisite);
    await fs.rm(file);
    await assert.rejects(assertPublicProjectUnchanged(project), { code: 'ENOENT' });
    await fs.writeFile(file, '{}\n');
    await assert.rejects(assertPublicProjectUnchanged(project), /Analysis changed/);
    await fs.writeFile(file, project.content[project.prerequisite]);
    await assertPublicProjectUnchanged(project);
  }
});
