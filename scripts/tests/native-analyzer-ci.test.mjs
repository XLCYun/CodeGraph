import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';
import { createHash } from 'node:crypto';
import { execFileSync, spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import { finalizeToolchainEnvironment, validateToolchainManifest, verifyArchive } from '../language-packs/provision-native-toolchains.mjs';
import { runVerificationCommands, verificationCommands } from '../language-packs/verify-native-analyzers.mjs';
import { publicEnvironment } from '../language-packs/public-cli-session.mjs';

const manifest = JSON.parse(await fs.readFile(new URL('../../resources/native-toolchains.json', import.meta.url), 'utf8'));
const workflowSource = (await fs.readFile(new URL('../../.github/workflows/ci.yml', import.meta.url), 'utf8')).replace(/\r\n/g, '\n');
const publishingWorkflow = (await fs.readFile(new URL('../../.github/workflows/release.yml', import.meta.url), 'utf8')).replace(/\r\n/g, '\n');

test('provisioned target SDKs survive public sanitization on both path conventions', () => {
  for (const [platform, paths, output] of [['darwin', path.posix, '/isolated tools'], ['win32', path.win32, 'D:\\isolated tools']]) {
    const installed = Object.fromEntries(['GO_SDK', 'GO_TARGET_SDK', 'JAVA_RUNTIME', 'JAVA_TARGET_JDK', 'JAVA_TARGET_8', 'DOTNET_SDK', 'CMAKE_ROOT']
      .map(name => [`CODEGRAPH_${name}`, paths.join(output, name)]));
    const environment = JSON.parse(JSON.stringify(finalizeToolchainEnvironment({ ...installed }, output, platform)));
    const publicEnv = publicEnvironment(environment);
    assert.equal(publicEnv.GOROOT, installed.CODEGRAPH_GO_TARGET_SDK);
    assert.equal(publicEnv.JAVA_HOME, installed.CODEGRAPH_JAVA_TARGET_8);
    assert.equal(publicEnv.JDK_HOME, installed.CODEGRAPH_JAVA_TARGET_JDK);
    assert.equal(publicEnv.DOTNET_ROOT, installed.CODEGRAPH_DOTNET_SDK);
    assert.equal(publicEnv.DOTNET_CLI_HOME, paths.join(output, 'dotnet-home'));
    assert.ok(Object.keys(publicEnv).every(name => !name.startsWith('CODEGRAPH_')));
    const goTest = verificationCommands(environment, platform, 'go', { paths })[0];
    assert.equal(goTest.environment.GOROOT, installed.CODEGRAPH_GO_SDK);
    assert.equal(goTest.executable, paths.join(installed.CODEGRAPH_GO_SDK, `bin/go${platform === 'win32' ? '.exe' : ''}`));
  }
});

/** 没有安装目标时不能把构建 SDK 当成目标，也不伪造标准发现路径。 */

test('environment finalization does not invent missing target SDKs', () => {
  const environment = finalizeToolchainEnvironment({ CODEGRAPH_GO_SDK: '/build/go', CODEGRAPH_JAVA_RUNTIME: '/build/jdk' }, '/tools');
  assert.equal(environment.GOROOT, undefined);
  assert.equal(environment.JAVA_HOME, undefined);
  assert.equal(environment.JDK_HOME, undefined);
  assert.equal(environment.CODEGRAPH_GO_SDK, '/build/go');
  assert.equal(environment.CODEGRAPH_JAVA_RUNTIME, '/build/jdk');
  const existing = finalizeToolchainEnvironment({ GOROOT: '/existing/go', JAVA_HOME: '/existing/jdk8', JDK_HOME: '/existing/jdk17' }, '/tools');
  assert.equal(existing.GOROOT, '/existing/go');
  assert.equal(existing.JAVA_HOME, '/existing/jdk8');
  assert.equal(existing.JDK_HOME, '/existing/jdk17');
});

// 用真实 Cargo 解析生产命令生成的 runner 配置，覆盖 Node → Git Bash → 缓存入口边界。

test('Rust helper runner configuration survives the complete Cargo agent chain', async () => {
  const directory = await fs.mkdtemp(path.join(os.tmpdir(), 'narrant cargo config 空格-'));
  try {
    const manifestPath = path.join(directory, 'Cargo.toml');
    await fs.writeFile(manifestPath, '[package]\nname = "runner_config_regression"\nversion = "0.1.0"\nedition = "2021"\n[lib]\npath = "lib.rs"\n');
    await fs.writeFile(path.join(directory, 'lib.rs'), '');
    const command = verificationCommands({ CODEGRAPH_PROJECT_RUST_TOOLCHAIN: 'stable', CODEGRAPH_TEST_CARGO_TOOLCHAINS: 'fixture', CODEGRAPH_TEST_CARGO_194: 'fixture' }, process.platform, 'rust')[0];
    const runner = command.args[command.args.indexOf('--config') + 1];
    // metadata 不编译、不下载；沿用已安装工具链，保留导致 Windows 转义丢失的完整调用链。
    const result = spawnSync(command.executable, [command.args[0], 'metadata', '--offline', '--no-deps', '--format-version', '1', '--manifest-path', manifestPath, '--config', runner], {
      cwd: fileURLToPath(new URL('../../', import.meta.url)),
      encoding: 'utf8',
      timeout: 20_000,
    });
    assert.equal(result.status, 0, result.error?.message ?? result.stderr);
    assert.equal(JSON.parse(result.stdout).packages[0].name, 'runner_config_regression');
  } finally { await fs.rm(directory, { recursive: true, force: true }); }
});

// 两个平台必须拥有全部固定构建工具和引用夹具，清单不能遗漏语言运行时。

test('official pinned manifest covers every required tool on both platforms', () => {
  assert.equal(validateToolchainManifest(manifest), manifest);
  for (const entries of Object.values(manifest.targets)) assert.deepEqual(entries.map(entry => entry.name).sort(), ['cmake', 'dotnet', 'dotnet-ref8', 'dotnet-ref9', 'go', 'go-target', 'jdk17', 'jdk25', 'jdk8', 'llvm']);
});

// 恶意或不完整清单必须在任何下载和解包发生前被拒绝。

test('untrusted downloads, missing digests and escaping homes fail before installation', () => {
  for (const mutation of [{ url: 'https://example.org/LLVM-latest.tar.xz' }, { digest: 'deadbeef' }, { home: '../outside' }, { home: 'C:/outside' }]) {
    const invalid = structuredClone(manifest);
    Object.assign(invalid.targets['x86_64-pc-windows-msvc'][0], mutation);
    assert.throws(() => validateToolchainManifest(invalid));
  }
});

// 篡改下载文件必须触发摘要失败，而不是继续安装。

test('download verification rejects a modified archive with a fixed SHA256', async () => {
  const directory = await fs.mkdtemp(path.join(os.tmpdir(), 'native-checksum-'));
  try {
    const file = path.join(directory, 'tool.tar.gz');
    await fs.writeFile(file, 'original');
    const entry = { name: 'fixture', algorithm: 'sha256', digest: createHash('sha256').update('original').digest('hex') };
    await verifyArchive(file, entry);
    await fs.writeFile(file, 'tampered');
    await assert.rejects(verifyArchive(file, entry), /Checksum mismatch/);
  } finally { await fs.rm(directory, { recursive: true, force: true }); }
});

// Windows 空格路径可用，缺任意 helper 或相对 SDK 根都要失败。

test('failed validation child does not prevent later child execution or make the result successful', async t => {
  const diagnostics = t.mock.method(console, 'error', () => {});
  const launches = t.mock.method(console, 'log', () => {});
  const directory = await fs.mkdtemp(path.join(os.tmpdir(), 'native validation groups '));
  const marker = path.join(directory, 'executed');
  const commands = [7, 0].map(code => ({
    executable: process.execPath,
    args: ['-e', `require('node:fs').appendFileSync(process.argv[1], '${code}\\n'); process.exit(${code})`, marker],
  }));
  try {
    await assert.rejects(runVerificationCommands(commands, process.env), error => {
      assert.ok(error instanceof AggregateError);
      assert.equal(error.errors.length, 1);
      assert.ok(error.errors[0].message.includes(commands[0].args[1]));
      assert.match(error.errors[0].cause.message, /Native validation failed \(7\)/);
      return true;
    });
    assert.equal(await fs.readFile(marker, 'utf8'), '7\n0\n');
    assert.equal(diagnostics.mock.callCount(), 1);
    assert.match(diagnostics.mock.calls[0].arguments[0].cause.message, /failed \(7\)/);
    assert.deepEqual(launches.mock.calls.map(call => call.arguments), commands.map(command => [`Native validation: ${command.executable} ${command.args.join(' ')}`]));
  } finally { await fs.rm(directory, { recursive: true, force: true }); }
});

// 多组失败保留各自命令和原始错误，异步执行始终串行且次序不变。

test('validation collects multiple original failures in order without overlapping groups', async t => {
  const diagnostics = t.mock.method(console, 'error', () => {});
  const commands = ['first', 'middle', 'last'].map(name => ({ executable: name, args: ['--fixture'] }));
  const failures = [new Error('first original failure'), new Error('last original failure')];
  const environment = { FIXTURE: 'same environment' };
  const events = [];
  let running = false;
  await assert.rejects(runVerificationCommands(commands, environment, async (command, passedEnvironment) => {
    assert.equal(running, false);
    assert.equal(passedEnvironment, environment);
    running = true;
    events.push(`start:${command.executable}`);
    await new Promise(resolve => setImmediate(resolve));
    events.push(`end:${command.executable}`);
    running = false;
    if (command === commands[0]) throw failures[0];
    if (command === commands[2]) throw failures[1];
  }), error => {
    assert.ok(error instanceof AggregateError);
    assert.equal(error.errors.length, 2);
    assert.equal(error.errors[0].cause, failures[0]);
    assert.equal(error.errors[1].cause, failures[1]);
    assert.match(error.errors[0].message, /first --fixture/);
    assert.match(error.errors[1].message, /last --fixture/);
    return true;
  });
  assert.deepEqual(events, ['start:first', 'end:first', 'start:middle', 'end:middle', 'start:last', 'end:last']);
  assert.deepEqual(diagnostics.mock.calls.map(call => call.arguments[0].cause), failures);
  assert.match(diagnostics.mock.calls[0].arguments[0].message, /first --fixture/);
  assert.match(diagnostics.mock.calls[1].arguments[0].message, /last --fixture/);
});

// 全部验收成功时正常完成，不能因汇总机制产生伪失败。

test('successful validation groups all complete normally', async () => {
  const commands = ['one', 'two'].map(executable => ({ executable, args: [] }));
  const visited = [];
  await runVerificationCommands(commands, {}, async command => { visited.push(command); });
  assert.deepEqual(visited, commands);
});

// 引用专用 NuGet 包跨架构使用同一来源摘要，不能偷换为浮动版本或任意 NuGet 工具。

test('net8 standard references have one official architecture-independent SHA512 pin', () => {
  const references = Object.values(manifest.targets).map(entries => entries.find(entry => entry.name === 'dotnet-ref8'));
  assert.deepEqual(references[0], references[1]);
  assert.equal(references[0].algorithm, 'sha512');
  assert.equal(references[0].environment, 'CODEGRAPH_DOTNET_REF8_ROOT');
  for (const mutation of [{ version: '8.0.1' }, { url: references[0].url.replace('8.0.0.nupkg', '8.0.1.nupkg') }, { algorithm: 'sha256', digest: 'a'.repeat(64) }]) {
    const invalid = structuredClone(manifest);
    Object.assign(invalid.targets['aarch64-apple-darwin'].find(entry => entry.name === 'dotnet-ref8'), mutation);
    assert.throws(() => validateToolchainManifest(invalid), /reference fixture/);
  }
});

// 官方引用包使用 SHA512，字节篡改必须在解包前被发现。

test('reference package verification checks SHA512 content', async () => {
  const directory = await fs.mkdtemp(path.join(os.tmpdir(), 'native-reference-checksum-'));
  try {
    const archive = path.join(directory, 'reference.nupkg');
    await fs.writeFile(archive, 'official reference declarations');
    const entry = { name: 'dotnet-ref8', algorithm: 'sha512', digest: createHash('sha512').update('official reference declarations').digest('hex') };
    await verifyArchive(archive, entry);
    await fs.appendFile(archive, 'changed');
    await assert.rejects(verifyArchive(archive, entry), /Checksum mismatch/);
  } finally { await fs.rm(directory, { recursive: true, force: true }); }
});


/** 每个平台编译出的原有语义断言都必须逐名接入。 */

test('CI gates both native targets, exact prerequisites, all packs and immutable release evidence', () => {
  assert.match(workflowSource, /runner: macos-15/);
  assert.match(workflowSource, /runner: windows-2022/);
  assert.ok(workflowSource.indexOf('run-windows-process-probe.mjs') < workflowSource.indexOf('provision-native-toolchains.mjs'));
  assert.ok(workflowSource.indexOf('git config --global core.longpaths true') < workflowSource.indexOf('toolchain install 1.98.0'));
  assert.match(workflowSource, /rustup component add rust-src --toolchain stable/);
  assert.match(workflowSource, /rustup toolchain install 1\.81\.0 1\.94\.0 1\.96\.0/);
  assert.match(workflowSource, /CARGO_HOME=' \+ process.env.CODEGRAPH_RUST_TEST_CARGO_HOME/);
  assert.match(workflowSource, /verify-native-analyzers.mjs artifacts --build --language all/);
  assert.doesNotMatch(workflowSource, /continue-on-error|if-no-files-found: warn|CARGO_TARGET_DIR|skip-ci/);
  const upload = workflowSource.split('uses: actions/upload-artifact@')[1].split('\n  complete:')[0];
  assert.doesNotMatch(upload, /always\(\)|if:/);
  assert.match(upload, /artifacts\/\*\/\*\/\*\/pack.tar.gz/);
  assert.match(upload, /verification-/);
  assert.match(publishingWorkflow, /needs: prepare/);
  assert.match(publishingWorkflow, /ref: \$\{\{ needs.prepare.outputs.source_commit \}\}/);
  assert.match(publishingWorkflow, /run-id: \$\{\{ inputs.ci_run_id \}\}/);
  assert.match(publishingWorkflow, /pattern: codegraph-\*/);
  assert.doesNotMatch(publishingWorkflow, /clobber|workflow_run|push:/);
});

/** helper 固定编译器与项目 Rust 矩阵是两个独立前置，缺少任何一项都失败。 */

test('Rust runner and project prerequisites preserve pinned compiler and isolated metadata', async () => {
  const commands = verificationCommands({ CODEGRAPH_PROJECT_RUST_TOOLCHAIN: 'stable', CODEGRAPH_TEST_CARGO_TOOLCHAINS: '/81:/94:/96', CODEGRAPH_TEST_CARGO_194: '/94' }, 'darwin', 'rust');
  assert.deepEqual(commands[0].args.slice(0, 6), ['scripts/cargo-agent.sh', '+1.98.0', 'test', '--manifest-path', 'analyzers/rust/Cargo.toml', '--config']);
  assert.match(commands[0].args[6], /run-rust-helper-test\.mjs/);
  for (const item of ['--locked', '--no-fail-fast', 'initial_protocol', 'lifecycle', '--include-ignored', '--test-threads=1']) assert.ok(commands[0].args.includes(item));
  const { prepareRustNativeTests } = await import('../language-packs/prepare-rust-native-tests.mjs');
  await assert.rejects(prepareRustNativeTests({}), /explicit.*CARGO_HOME/);
  await assert.rejects(prepareRustNativeTests({ CODEGRAPH_PROJECT_RUST_TOOLCHAIN: 'stable', CARGO_HOME: '/cargo', CODEGRAPH_TEST_CARGO_TOOLCHAINS: '/only-one' }), /exactly/);
});

/** 提供空目录或拼错 SDK 不会被 helper suite 的 skip 分支隐藏。 */

test('missing named semantic success rejects an otherwise successful subprocess', async t => {
  const diagnostics = t.mock.method(console, 'error', () => {});
  const launches = t.mock.method(console, 'log', () => {});
  const command = { executable: process.execPath, args: ['-e', 'process.exit(0)'], requiredTests: ['required::real_semantic_case'] };
  await assert.rejects(runVerificationCommands([command], process.env), error => {
    assert.match(error.errors[0].cause.message, /Required semantic test did not pass/);
    return true;
  });
  assert.deepEqual(launches.mock.calls.map(call => call.arguments), [[`Native validation: ${command.executable} ${command.args.join(' ')}`]]);
  assert.equal(diagnostics.mock.callCount(), 1);
  assert.match(diagnostics.mock.calls[0].arguments[0].cause.message, /Required semantic test did not pass: required::real_semantic_case/);
});

/** 新命令绑定非交付驱动和公共入口，不能借旧根模块或静默子集产生完整 receipt。 */
test('verification binds every retained semantic name to new public integration commands', async () => {
  const environment = Object.fromEntries(['GO_SDK', 'GO_TARGET_SDK', 'DOTNET_SDK', 'DOTNET_REF8_ROOT', 'DOTNET_REF9_ROOT', 'JAVA_RUNTIME', 'JAVA_TARGET_JDK', 'JAVA_TARGET_8', 'LLVM_SDK', 'PROJECT_RUST_TOOLCHAIN', 'TEST_CARGO_TOOLCHAINS', 'TEST_CARGO_194'].map(name => [`CODEGRAPH_${name}`, '/isolated/tools']));
  const inventory = JSON.parse(await fs.readFile(new URL('../../resources/semantic-acceptance.json', import.meta.url)));
  assert.equal(inventory.flatMap(item => item.historicalNames).length, 78);
  for (const platform of ['darwin', 'win32']) {
    const commands = verificationCommands(environment, platform);
    assert.deepEqual(commands.flatMap(command => command.requiredTests ?? []).sort(), inventory.filter(item => item.platforms.includes(platform)).map(item => item.name).sort());
    assert.ok(commands.some(command => command.args.includes('analyzers/go/tests')));
    assert.ok(commands.some(command => command.args.includes('analyzers/common/Cargo.toml')));
    assert.ok(commands.some(command => command.args.includes('analyzers/windows-process-probe/Cargo.toml')));
    assert.ok(commands.some(command => command.args.includes('--workspace')));
    assert.ok(commands.some(command => command.environment?.CODEGRAPH_CLANG_HELPER?.includes('codegraph-clang-components')));
    assert.ok(commands.some(command => command.args.includes('analyzers/typescript/tests/historical.test.mjs')));
    assert.doesNotMatch(JSON.stringify(commands), /adapters::|native_analyzer::|codegraph-.*-engine(?:\.exe)?["/]/);
    for (const command of commands.filter(command => command.executable === 'bash')) assert.equal(command.args[0], 'scripts/cargo-agent.sh');
  }
  for (const file of new Set(inventory.map(item => item.source))) {
    const source = (await fs.readFile(new URL(`../../${file}`, import.meta.url), 'utf8')).replace(/\r\n/g, '\n');
    const functions = [...source.matchAll(/^((?:#\[[^\n]+\]\n)+)fn (\w+)\(/gm)].filter(match => match[1].includes('#[ignore'));
    assert.equal(functions.length, inventory.filter(item => item.source === file).length, file);
    for (const [, attributes, name] of functions) {
      const cfg = [...attributes.matchAll(/#\[cfg\(([^\n]+)\)\]/g)].map(match => match[1]);
      assert.ok(cfg.length === 0 || cfg.length === 1 && ['unix', 'windows'].includes(cfg[0]));
      const platforms = cfg[0] === 'unix' ? ['darwin'] : cfg[0] === 'windows' ? ['win32'] : ['darwin', 'win32'];
      assert.deepEqual(inventory.find(item => item.source === file && item.name.endsWith(`::${name}`))?.platforms, platforms);
    }
  }
  assert.throws(() => verificationCommands({}, 'darwin', 'go'), /Missing fixed toolchain/);
  assert.throws(() => verificationCommands({}, 'darwin', 'typo'), /not enabled/);
});

/** 不随包组件由源码构建，公共测试必须指向真实包内 apphost/native 程序。 */
test('each selected language requires its package and real test drivers', async () => {
  const { validateNativeFixtures } = await import('../language-packs/verify-native-analyzers.mjs');
  await assert.rejects(validateNativeFixtures({ CODEGRAPH_NATIVE_PACKS: '/missing/codegraph' }, 'go'), /ENOENT/);
  for (const id of ['rust', 'typescript-javascript', 'python', 'java', 'go', 'csharp', 'c-cpp']) {
    const env = Object.fromEntries(['GO_SDK', 'GO_TARGET_SDK', 'DOTNET_SDK', 'DOTNET_REF8_ROOT', 'DOTNET_REF9_ROOT', 'JAVA_RUNTIME', 'JAVA_TARGET_JDK', 'JAVA_TARGET_8', 'LLVM_SDK', 'PROJECT_RUST_TOOLCHAIN', 'TEST_CARGO_TOOLCHAINS', 'TEST_CARGO_194'].map(name => [`CODEGRAPH_${name}`, '/fixed']));
    const commands = verificationCommands(env, 'darwin', id);
    assert.ok(commands.some(command => command.args.includes('scripts/language-packs/verify-public-cli.mjs') && command.args.at(-1) === id));
    assert.ok(commands.some(command => command.args.includes('analyzers/integration-tests/Cargo.toml') && command.requiredTests.length));
  }
});

/** 完整 self-contained 恢复必须核对两个真实 RID 的官方版本和 SHA512。 */
test('self-contained runtime packs bind official RID version and digest', async () => {
  const { validateRuntimePack } = await import('../language-packs/dotnet-runtime-pack.mjs');
  for (const target of ['aarch64-apple-darwin', 'x86_64-pc-windows-msvc']) {
    const entry = manifest.runtime_packs[target];
    assert.equal(validateRuntimePack(entry, target), entry);
    assert.equal(entry.version, '10.0.0');
    for (const mutation of [{ rid: 'other' }, { version: '10.0.1' }, { digest: 'abc' }, { url: entry.url.replace('10.0.0', '10.0.1') }, { metadata_source: 'https://example.org/' }]) assert.throws(() => validateRuntimePack({ ...entry, ...mutation }, target), /fixed .NET runtime/);
  }
  assert.notEqual(manifest.runtime_packs['aarch64-apple-darwin'].digest, manifest.runtime_packs['x86_64-pc-windows-msvc'].digest);
});
