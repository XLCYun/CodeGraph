import { test } from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { execFile, execFileSync } from 'node:child_process';
import { createServer } from 'node:http';
import { createHash } from 'node:crypto';
import { promisify } from 'node:util';
import { packageDirectory } from '../language-packs/build-language-support-packs.mjs';
import { copyToolTree, smokeAnalyzer, csharpFixturePlatforms, javaFixturePlatforms, goFixturePlatforms } from '../language-packs/native-language-support-packs.mjs';
import { prepareDotnetFixtureReferences } from '../language-packs/provision-native-toolchains.mjs';
import { systemTar } from '../language-packs/system-tar.mjs';

/** 私有 runtime 的执行入口必须穿过打包与解压，两个平台都真实启动解包后的文件。 */
test('native runtime executable survives archive extraction', async () => {
  const root = await fs.mkdtemp(path.join(os.tmpdir(), 'narrant native permissions-'));
  try {
    const input = path.join(root, 'input');
    const windows = process.platform === 'win32';
    const entry = windows ? 'runtime/jdk/bin/java.exe' : 'runtime/jdk/bin/java';
    await fs.mkdir(path.join(input, 'runtime/jdk/bin'), { recursive: true });
    if (windows) await fs.copyFile(process.execPath, path.join(input, entry));
    else await fs.writeFile(path.join(input, entry), '#!/bin/sh\nexit 0\n', { mode: 0o755 });
    const target = windows ? 'x86_64-pc-windows-msvc' : 'aarch64-apple-darwin';
    const output = await packageDirectory({ id: 'java', version: '0.1.1', target }, input, path.join(root, 'output'));
    const manifest = JSON.parse(await fs.readFile(path.join(output, 'manifest.json')));
    assert.equal(manifest.files.find(file => file.path === entry)?.executable, true);
    const extracted = path.join(root, 'extracted');
    await fs.mkdir(extracted);
    execFileSync(systemTar(), ['-xzf', path.join(output, 'pack.tar.gz'), '-C', extracted]);
    const executable = path.join(extracted, entry);
    if (windows) assert.equal(execFileSync(executable, ['--version'], { encoding: 'utf8', timeout: 10_000 }).trim(), process.version);
    else {
      assert.ok((await fs.stat(executable)).mode & 0o111);
      execFileSync(executable, [], { timeout: 10_000 });
    }
  } finally { await fs.rm(root, { recursive: true, force: true }); }
});

/** Windows 的 .exe 入口不依赖 POSIX 位；同名非执行文件不会让 macOS 产物获得执行权限。 */
test('runtime executable metadata follows target platform', async () => {
  const root = await fs.mkdtemp(path.join(os.tmpdir(), 'narrant-runtime-target-'));
  try {
    const input = path.join(root, 'input');
    await fs.mkdir(path.join(input, 'runtime/jdk/bin'), { recursive: true });
    await fs.writeFile(path.join(input, 'runtime/jdk/bin/java.exe'), 'fixture', { mode: 0o644 });
    await fs.writeFile(path.join(input, 'runtime/jdk/bin/java.dll'), 'fixture', { mode: 0o644 });
    for (const target of ['x86_64-pc-windows-msvc', 'aarch64-apple-darwin']) {
      const output = await packageDirectory({ id: 'java', version: '0.1.1', target }, input, path.join(root, 'output'));
      const manifest = JSON.parse(await fs.readFile(path.join(output, 'manifest.json')));
      assert.equal(manifest.files.find(file => file.path.endsWith('java.exe')).executable, target === 'x86_64-pc-windows-msvc');
      assert.equal(manifest.files.find(file => file.path.endsWith('java.dll')).executable, false);
    }
  } finally { await fs.rm(root, { recursive: true, force: true }); }
});

/** SDK 内部链接转换为普通文件，包外链接必须在复制时失败。 */
test('SDK tree copy materializes internal files and rejects escape', { skip: process.platform === 'win32' }, async () => {
  const root = await fs.mkdtemp(path.join(os.tmpdir(), 'codegraph-engine-tree-'));
  try {
    const input = path.join(root, 'input');
    await fs.mkdir(input);
    await fs.writeFile(path.join(input, 'source'), 'declaration');
    await fs.symlink('source', path.join(input, 'link'));
    await copyToolTree(input, path.join(root, 'good'));
    assert.equal((await fs.lstat(path.join(root, 'good/link'))).isSymbolicLink(), false);
    await fs.writeFile(path.join(root, 'outside'), 'not approved');
    await fs.symlink('../outside', path.join(input, 'escape'));
    await assert.rejects(copyToolTree(input, path.join(root, 'bad')), /escapes approved root/);
  } finally { await fs.rm(root, { recursive: true, force: true }); }
});

/** 最终身份必须来自唯一公共结果，旧 ready 或错语言不能蒙混通过。 */
test('direct smoke rejects mismatched final identity and old ready transport', async () => {
  for (const expression of ["{protocol:'codegraph.analyzer/1',engine_version:'0.1.1',pack_id:'other',type:'completed',id:'pack-smoke',outcome:'ok',graph:{root_contains:[],nodes:[],external_symbols:[]}}", "{protocol:'codegraph.engine/1',type:'ready',helper_id:'other'}"]) {
    await assert.rejects(smokeAnalyzer({ executable: process.execPath, args: ['-e', `process.stdin.resume();process.stdin.on('end',()=>console.log(JSON.stringify(${expression})))`], id: 'go' }), /identity mismatch/);
  }
});

/** JSON 对象字段顺序不影响空图语义，任何多余节点仍须明确拒绝。 */
test('direct smoke accepts reordered empty fields and rejects nonempty graphs', async () => {
  const smoke = graph => smokeAnalyzer({ executable: process.execPath, args: ['-e', `process.stdin.resume();process.stdin.on('end',()=>console.log(JSON.stringify({protocol:'codegraph.analyzer/1',engine_version:'0.1.1',pack_id:'go',type:'completed',id:'pack-smoke',outcome:'ok',graph:${graph}})))`], id: 'go' });
  await smoke('{external_symbols:[],nodes:[],root_contains:[]}');
  await assert.rejects(smoke('{external_symbols:[],nodes:[{}],root_contains:[]}'), /Empty public graph mismatch/);
});

/** 真实 EOF 后仍不退出的入口须被调用方终止，close 后才能删除其 cwd。 */
test('smoke waits for killed child close before rejecting and removing cwd', async t => {
  const directory = await fs.mkdtemp(path.join(os.tmpdir(), 'codegraph-smoke-close-'));
  t.after(() => fs.rm(directory, { recursive: true, force: true }));
  const marker = path.join(directory, 'cwd');
  await assert.rejects(smokeAnalyzer({ executable: process.execPath, args: ['-e', `require('node:fs').writeFileSync(${JSON.stringify(marker)},JSON.stringify({cwd:process.cwd(),pid:process.pid}));process.stdin.resume();setInterval(()=>{},1000)`], id: 'go', timeoutMs: 300 }), /timed out/);
  const observed = JSON.parse(await fs.readFile(marker, 'utf8'));
  await assert.rejects(fs.stat(observed.cwd), { code: 'ENOENT' });
  assert.throws(() => process.kill(observed.pid, 0), { code: 'ESRCH' });
});

/** 清理异常保留原始最终身份故障，无主故障时报告真实清理错误。 */
test('smoke preserves primary failure and reports cleanup-only failure', async t => {
  const cleanupError = new Error('fixture cleanup failure'), remove = fs.rm;
  t.mock.method(fs, 'rm', async (root, options) => { await remove(root, options); if (path.basename(root).startsWith('codegraph-direct-pack-smoke-')) throw cleanupError; });
  const smoke = pack => smokeAnalyzer({ executable: process.execPath, id: 'go', args: ['-e', `process.stdin.resume();process.stdin.on('end',()=>console.log(JSON.stringify({protocol:'codegraph.analyzer/1',engine_version:'0.1.1',pack_id:'${pack}',type:'completed',id:'pack-smoke',outcome:'ok',graph:{root_contains:[],nodes:[],external_symbols:[]}})))`] });
  await assert.rejects(smoke('other'), error => /identity mismatch/.test(error.message) && /scratch cleanup failed: fixture cleanup failure/.test(error.message));
  await assert.rejects(smoke('go'), error => error === cleanupError);
});

/** 固定公开版本只声明已通过真实发布验证的七包与两个目标，不能漂移身份。 */
test('native catalog pins the published seven-pack dual-platform release', async () => {
  const catalog = JSON.parse(await fs.readFile(new URL('../../resources/language-support-packs.json', import.meta.url)));
  assert.equal(catalog.repository, 'XLCYun/CodeGraph');
  assert.equal(catalog.engineVersion, '0.1.1');
  assert.equal(catalog.protocol, 'codegraph.analyzer/1');
  assert.deepEqual(Object.keys(catalog.packs).sort(), ['rust', 'typescript-javascript', 'python', 'java', 'go', 'csharp', 'c-cpp'].sort());
  for (const pack of Object.values(catalog.packs)) {
    assert.equal(pack.version, '0.1.1');
    assert.equal(pack.published, true);
    assert.deepEqual(pack.verifiedTargets, ['aarch64-apple-darwin', 'x86_64-pc-windows-msvc']);
  }
});

/** 标准引用只出现在本机测试 SDK，生成的目录描述允许新 TFM/补丁且不复制构建资产。 */
test('C# fixture discovery uses installed SDK layout without bundling target references', async () => {
  const root = await fs.mkdtemp(path.join(os.tmpdir(), 'narrant-reference-discovery-'));
  try {
    const sdk = path.join(root, 'sdk'), source = path.join(root, 'reference9');
    await fs.mkdir(path.join(source, 'ref/net9.0'), { recursive: true });
    await fs.writeFile(path.join(source, 'ref/net9.0/System.Runtime.dll'), 'net9 reference');
    await fs.writeFile(path.join(source, 'Microsoft.NETCore.App.Ref.nuspec'), '<id>Microsoft.NETCore.App.Ref</id><version>9.0.20</version>');
    await fs.mkdir(path.join(source, 'build'));
    await fs.writeFile(path.join(source, 'build/untrusted.targets'), 'must not copy');
    await prepareDotnetFixtureReferences({ CODEGRAPH_DOTNET_SDK: sdk, CODEGRAPH_DOTNET_REF9_ROOT: source });
    const platforms = await csharpFixturePlatforms(sdk);
    assert.equal(platforms.length, 1);
    assert.equal(platforms[0].version, '9.0.20');
    assert.equal(platforms[0].metadata.tfm, 'net9.0');
    assert.equal(await fs.readFile(path.join(platforms[0].root, 'System.Runtime.dll'), 'utf8'), 'net9 reference');
    assert.equal(platforms[0].root, path.join(sdk, 'packs/Microsoft.NETCore.App.Ref/9.0.20/ref/net9.0'));
    await assert.rejects(fs.stat(path.join(sdk, 'packs/Microsoft.NETCore.App.Ref/9.0.20/build')), { code: 'ENOENT' });
    const catalog = JSON.parse(await fs.readFile(new URL('../../resources/language-support-packs.json', import.meta.url)));
    assert.equal(catalog.packs.csharp.referenceTargets, undefined);
    assert.equal(catalog.packs.csharp.targetStandardLibrary, 'local-dotnet-reference-packs');
  } finally { await fs.rm(root, { recursive: true, force: true }); }
});

/** Java 与 Go 的目标版本来自准备好的 SDK 元数据，产品支持包只保留发现策略。 */
test('Java and Go fixtures describe actual SDKs without a bundled target list', async () => {
  const root = await fs.mkdtemp(path.join(os.tmpdir(), 'codegraph-java-engine-go-targets-'));
  try {
    const java = path.join(root, 'jdk'), go = path.join(root, 'go');
    await fs.mkdir(java); await fs.mkdir(go);
    await fs.writeFile(path.join(java, 'release'), 'JAVA_VERSION="21.0.12"\n');
    await fs.writeFile(path.join(go, 'VERSION'), 'go1.26.7\ntime fixture\n');
    assert.equal((await javaFixturePlatforms([java]))[0].metadata.release, '21');
    assert.equal((await goFixturePlatforms([go]))[0].version, '1.26.7');
    const catalog = JSON.parse(await fs.readFile(new URL('../../resources/language-support-packs.json', import.meta.url)));
    assert.equal(catalog.packs.java.targetStandardLibrary, 'local-java-jdks');
    assert.equal(catalog.packs.java.targetJava, undefined);
    assert.equal(catalog.packs.java.targetJdk, undefined);
    assert.equal(catalog.packs.go.targetStandardLibrary, 'local-go-sdks');
  } finally { await fs.rm(root, { recursive: true, force: true }); }
});

/** 生产 TS 模块完整暂存，新 runtime 和 graph 不能遗漏；测试文件不进入包。 */
test('TypeScript staging preserves every production module and no test driver', async () => {
  const { stageTypeScript } = await import('../language-packs/stage-typescript.mjs');
  const root = await fs.mkdtemp(path.join(os.tmpdir(), 'direct-typescript-staging-'));
  try {
    await stageTypeScript(root);
    const expected = (await fs.readdir(new URL('../../analyzers/typescript', import.meta.url))).filter(name => name.endsWith('.mjs')).sort();
    assert.deepEqual((await fs.readdir(path.join(root, 'runtime/analyzer'))).sort(), expected);
    for (const name of expected) assert.deepEqual(await fs.readFile(path.join(root, 'runtime/analyzer', name)), await fs.readFile(new URL(`../../analyzers/typescript/${name}`, import.meta.url)));
    assert.ok(expected.includes('graph.mjs') && expected.includes('runtime.mjs'));
    await assert.rejects(fs.stat(path.join(root, 'runtime/helper')), { code: 'ENOENT' });
  } finally { await fs.rm(root, { recursive: true, force: true }); }
});

/** 语言筛选依据已迁移集合，不为不相关语言下载工具或接受未实现的核心。 */
test('migration toolchain selection is explicit and complete', async () => {
  const { selectedNativePacks, selectedToolchainNames } = await import('../language-packs/native-migration.mjs');
  assert.deepEqual(selectedToolchainNames('go'), ['go', 'go-target']);
  assert.deepEqual(selectedToolchainNames('java'), ['jdk25', 'jdk17', 'jdk8']);
  assert.deepEqual(selectedToolchainNames('csharp'), ['dotnet', 'dotnet-ref8', 'dotnet-ref9']);
  assert.deepEqual(selectedToolchainNames('c-cpp'), ['llvm', 'cmake']);
  assert.equal(selectedToolchainNames().length, 10);
  assert.deepEqual(selectedNativePacks(), ['rust', 'python', 'typescript-javascript', 'java', 'go', 'csharp', 'c-cpp']);
  assert.deepEqual(selectedToolchainNames('typescript-javascript'), []);
  for (const id of ['none']) assert.throws(() => selectedToolchainNames(id), /not enabled/);
});

/** Cargo 许可步骤只保留目标生产闭包，开发依赖不能冒充实际分发依赖。 */
test('Cargo license packaging excludes unreachable development dependencies and binds real license bytes', async () => {
  const { packageCargoLicenses } = await import('../language-packs/native-language-support-packs.mjs');
  const repository = fileURLToPath(new URL('../../', import.meta.url));
  const root = await fs.mkdtemp(path.join(repository, '.license-test-'));
  try {
    for (const name of ['source', 'development']) {
      await fs.mkdir(path.join(root, name));
      await fs.writeFile(path.join(root, name, 'lib.rs'), '');
      await fs.writeFile(path.join(root, name, 'Cargo.toml'), `[package]\nname="license_${name}"\nversion="0.1.1"\nedition="2021"\nlicense="MIT"\n[workspace]\n[lib]\npath="lib.rs"\n${name === 'source' ? '[dev-dependencies]\nlicense_development={path="../development"}\n' : ''}`);
      await fs.writeFile(path.join(root, name, 'LICENSE'), `actual ${name} license bytes`);
    }
    await fs.writeFile(path.join(root, 'source/Cargo.lock'), 'version = 4\n[[package]]\nname = "license_source"\nversion = "0.1.1"\ndependencies = ["license_development"]\n[[package]]\nname = "license_development"\nversion = "0.1.1"\n');
    const payload = path.join(root, 'payload');
    await fs.mkdir(path.join(payload, 'licenses'), { recursive: true });
    await packageCargoLicenses(path.join(root, 'source'), process.platform === 'win32' ? 'x86_64-pc-windows-msvc' : 'aarch64-apple-darwin', payload, 'Fixture');
    const notices = JSON.parse(await fs.readFile(path.join(payload, 'licenses/Fixture-dependencies.json')));
    assert.deepEqual(notices.map(item => item.name), ['license_source']);
    assert.equal(await fs.readFile(path.join(payload, 'licenses/license_source-0.1.1/LICENSE'), 'utf8'), 'actual source license bytes');
    await assert.rejects(fs.stat(path.join(payload, 'licenses/license_development-0.1.1')), { code: 'ENOENT' });
  } finally { await fs.rm(root, { recursive: true, force: true }); }
});

/** 用本机 registry 重现 release 缓存没有开发 crate 的情况，收集仍需完整 metadata。 */
test('Cargo license packaging fetches uncached locked metadata without distributing dev licenses', async () => {
  const repository = fileURLToPath(new URL('../../', import.meta.url));
  const root = await fs.mkdtemp(path.join(repository, '.license cache test-'));
  const run = promisify(execFile);
  let server;
  try {
    const source = path.join(root, 'source'), cargoHome = path.join(root, 'cargo-home');
    const crate = path.join(root, 'license_development-0.1.1');
    for (const directory of [source, cargoHome, crate]) await fs.mkdir(directory);
    await fs.writeFile(path.join(source, 'lib.rs'), '');
    await fs.writeFile(path.join(source, 'LICENSE'), 'actual distributed source license');
    await fs.writeFile(path.join(source, 'Cargo.toml'), '[package]\nname="license_source"\nversion="0.1.1"\nedition="2021"\nlicense="MIT"\n[workspace]\n[lib]\npath="lib.rs"\n[dev-dependencies]\nlicense_development="=0.1.1"\n');
    await fs.writeFile(path.join(crate, 'lib.rs'), '');
    await fs.writeFile(path.join(crate, 'LICENSE'), 'development license must not be distributed');
    await fs.writeFile(path.join(crate, 'Cargo.toml'), '[package]\nname="license_development"\nversion="0.1.1"\nedition="2021"\nlicense="MIT"\n[workspace]\n[lib]\npath="lib.rs"\n');
    const archive = path.join(root, 'development.crate');
    execFileSync(systemTar(), ['-czf', archive, '-C', root, 'license_development-0.1.1'], { env: { ...process.env, COPYFILE_DISABLE: '1' } });
    const bytes = await fs.readFile(archive), checksum = createHash('sha256').update(bytes).digest('hex');
    const lock = `version = 4\n[[package]]\nname = "license_source"\nversion = "0.1.1"\ndependencies = ["license_development"]\n[[package]]\nname = "license_development"\nversion = "0.1.1"\nsource = "registry+https://github.com/rust-lang/crates.io-index"\nchecksum = "${checksum}"\n`;
    await fs.writeFile(path.join(source, 'Cargo.lock'), lock);
    let downloads = 0, address;
    // 本机稀疏索引和真实 crate 归档让测试不依赖公网或预热的用户缓存。
    server = createServer((request, response) => {
      if (request.url === '/index/config.json') response.end(JSON.stringify({ dl: `${address}/api/v1/crates`, api: address }));
      else if (request.url === '/index/li/ce/license_development') response.end(`${JSON.stringify({ name: 'license_development', vers: '0.1.1', deps: [], cksum: checksum, features: {}, yanked: false })}\n`);
      else if (request.url === '/api/v1/crates/license_development/0.1.1/download') { downloads++; response.end(bytes); }
      else { response.statusCode = 404; response.end(); }
    });
    await new Promise(resolve => server.listen(0, '127.0.0.1', resolve));
    address = `http://127.0.0.1:${server.address().port}`;
    await fs.writeFile(path.join(cargoHome, 'config.toml'), `[source.crates-io]\nreplace-with="fixture"\n[source.fixture]\nregistry="sparse+${address}/index/"\n`);
    const target = process.platform === 'win32' ? 'x86_64-pc-windows-msvc' : 'aarch64-apple-darwin';
    const environment = { ...process.env, CARGO_HOME: cargoHome };
    // 只预热 registry metadata；然后仅移除本夹具的 dev crate，保留索引与锁。
    await run('bash', [path.join(repository, 'scripts/cargo-agent.sh'), 'metadata', '--locked', '--filter-platform', target, '--format-version', '1'], { cwd: source, env: environment });
    assert.equal(downloads, 1);
    for (const directory of ['cache', 'src']) {
      for (const registry of await fs.readdir(path.join(cargoHome, 'registry', directory))) {
        await fs.rm(path.join(cargoHome, 'registry', directory, registry, `license_development-0.1.1${directory === 'cache' ? '.crate' : ''}`), { recursive: true });
      }
    }
    const payload = path.join(root, 'payload');
    await fs.mkdir(path.join(payload, 'licenses'), { recursive: true });
    const module = new URL('../language-packs/native-language-support-packs.mjs', import.meta.url).href;
    // 独立子进程传环境，避免并发测试改写全局 CARGO_HOME；异步调用允许 registry 响应。
    await run(process.execPath, ['--input-type=module', '-e', `import { packageCargoLicenses } from ${JSON.stringify(module)}; await packageCargoLicenses(...JSON.parse(process.argv[1]));`, JSON.stringify([source, target, payload, 'Fixture'])], { cwd: repository, env: environment });
    assert.equal(downloads, 2, 'collector must fetch the uncached locked development crate');
    assert.equal(await fs.readFile(path.join(source, 'Cargo.lock'), 'utf8'), lock);
    const notices = JSON.parse(await fs.readFile(path.join(payload, 'licenses/Fixture-dependencies.json')));
    assert.deepEqual(notices.map(item => item.name), ['license_source']);
    assert.equal(await fs.readFile(path.join(payload, 'licenses/license_source-0.1.1/LICENSE'), 'utf8'), 'actual distributed source license');
    await assert.rejects(fs.stat(path.join(payload, 'licenses/license_development-0.1.1')), { code: 'ENOENT' });
  } finally {
    if (server) await new Promise(resolve => server.close(resolve));
    await fs.rm(root, { recursive: true, force: true });
  }
});
