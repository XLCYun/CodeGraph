import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';
import { execFile } from 'node:child_process';
import { promisify } from 'node:util';
import { preparePublicProject, assertPublicProjectUnchanged, runPublicPackAcceptances } from '../language-packs/verify-public-cli.mjs';
import { fixtures, prepareCancellationProject } from '../language-packs/public-cli-projects.mjs';
import { publicEnvironment } from '../language-packs/public-cli-session.mjs';
import { packIds } from '../language-packs/distribution.mjs';

/** 前一包真实退出失败后仍验证下一包，最终保留每个原始错误且不能转为成功。 */
test('public pack failures preserve diagnostics and do not prevent later real children', async t => {
  const diagnostics = t.mock.method(console, 'error', () => {});
  const root = await fs.mkdtemp(path.join(os.tmpdir(), 'codegraph public groups-'));
  t.after(() => fs.rm(root, { recursive: true, force: true }));
  const marker = path.join(root, 'executed'), original = [];
  let failure;
  try {
    await runPublicPackAcceptances(['csharp', 'c-cpp', 'python'], async id => {
      const status = { csharp: 1, 'c-cpp': 0, python: 2 }[id];
      try {
        await promisify(execFile)(process.execPath, ['-e', "require('node:fs').appendFileSync(process.argv[1], process.argv[2] + '\\n'); console.error('original-' + process.argv[2]); process.exit(Number(process.argv[3]));", marker, id, String(status)]);
      } catch (error) { original.push(error); throw error; }
    });
  } catch (error) { failure = error; }
  assert.equal(await fs.readFile(marker, 'utf8'), 'csharp\nc-cpp\npython\n');
  assert.ok(failure instanceof AggregateError);
  assert.equal(failure.errors.length, 2);
  assert.deepEqual(failure.errors.map(error => error.cause), original);
  assert.match(failure.errors[0].message, /csharp/);
  assert.match(failure.errors[1].message, /python/);
  assert.match(original[0].stderr, /original-csharp/);
  assert.match(original[1].stderr, /original-python/);
  assert.deepEqual(diagnostics.mock.calls.map(call => call.arguments[0]), failure.errors);
});

/** 全部验收成功才可让上层继续写成功 receipt，且始终串行完成。 */
test('public pack success requires every complete callback without overlapping', async () => {
  const visited = [];
  let running = false;
  await runPublicPackAcceptances(['csharp', 'c-cpp'], async id => {
    assert.equal(running, false);
    running = true;
    visited.push(`start:${id}`);
    await new Promise(resolve => setImmediate(resolve));
    visited.push(`end:${id}`);
    running = false;
  });
  assert.deepEqual(visited, ['start:csharp', 'end:csharp', 'start:c-cpp', 'end:c-cpp']);
});

/** Java 的公开验收必须提供 helper 支持的声明式源码根与确切目标 release。 */
test('public Java project declares its source root and target release', async t => {
  const root = await fs.mkdtemp(path.join(os.tmpdir(), 'codegraph java fixture-'));
  t.after(() => fs.rm(root, { recursive: true, force: true }));
  const project = await preparePublicProject('java', root);
  const metadata = JSON.parse(await fs.readFile(path.join(root, 'narrant-java.json'), 'utf8'));
  assert.deepEqual(metadata, { schema: 'java-static-v1', release: '17', source_roots: [{ path: 'src', role: 'main' }, { path: 'checks', role: 'test' }] });
  assert.deepEqual(project.files, ['src/Sample.java', 'src/Api.java', 'checks/Checks.java']);
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
  assert.deepEqual(project.files, ['main.cpp', 'api.hpp']);
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

/** 每语言的公开夹具都必须包含独立调用目标、配置外成员和真实配置取消负载。 */
test('all seven public projects retain configuration boundaries and omitted-files cancellation workloads', async t => {
  const root = await fs.mkdtemp(path.join(os.tmpdir(), 'codegraph discovery fixtures-'));
  t.after(() => fs.rm(root, { recursive: true, force: true }));
  assert.deepEqual(Object.keys(fixtures).sort(), [...packIds].sort());
  for (const id of packIds) {
    const project = await preparePublicProject(id, path.join(root, id));
    assert.ok(project.files.length >= 2);
    assert.ok(project.selected.length < project.files.length);
    assert.ok(Object.values(project.content).some(source => source.includes('excluded_member')));
    assert.ok(project.content[project.prerequisite]);
    await assertPublicProjectUnchanged(project);
    const cancellation = await prepareCancellationProject(id, path.join(root, `${id}-cancel`));
    assert.ok(Object.keys(cancellation.content).length >= 600);
    assert.ok(Object.values(cancellation.content).some(source => source.includes('value_599_99')));
    await assertPublicProjectUnchanged(cancellation);
  }
});

/** 标准 SDK 定位可以保留，但私有 helper 配置绝不能令公开包验收虚假通过。 */
test('public entry acceptance removes producer-only environment', () => {
  assert.deepEqual(publicEnvironment({ PATH: '/standard/tools', GOROOT: '/go', DOTNET_ROOT: '/dotnet', JAVA_HOME: '/jdk8', JDK_HOME: '/jdk17', CODEGRAPH_NATIVE_HELPERS_CONFIG: '/private.json', CODEGRAPH_GO_SDK: '/build/sdk', CODEGRAPH_GO_TARGET_SDK: '/go', CODEGRAPH_JAVA_TARGET_JDK: '/jdk17', CODEGRAPH_TEST_DISCOVERY_PROBE: '/unexpected/probe' }), { PATH: '/standard/tools', GOROOT: '/go', DOTNET_ROOT: '/dotnet', JAVA_HOME: '/jdk8', JDK_HOME: '/jdk17' });
});
