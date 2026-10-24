import { migratedNativePacks } from './native-migration.mjs';
import fs from 'node:fs/promises';
import path from 'node:path';
import os from 'node:os';
import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { stageAnalyzer } from './build-analyzer.mjs';
import { nativeTarget, repository, releaseIdentity } from './distribution.mjs';
import { verifyAndExtract } from './verify-distribution.mjs';
import { verifyAnalyzerDistribution } from './verify-analyzer-package.mjs';
import { exchangeCli } from './public-cli-session.mjs';
import { prepareRuntimePack, verifyPublishedRuntime } from './dotnet-runtime-pack.mjs';

/** 只复制自己构建的工具和批准的 SDK 声明，展开 SDK 内部链接，拒绝外向链接。 */
export async function copyToolTree(source, destination) {
  const root = await fs.realpath(source);
  /** 对每个实际文件重新验证来源，归档最终只含普通文件。 */
  async function copy(current, target, ancestors = new Set()) {
    const resolved = await fs.realpath(current);
    if (resolved !== root && !resolved.startsWith(`${root}${path.sep}`)) throw new Error(`Tool symlink escapes approved root: ${current}`);
    const info = await fs.stat(resolved);
    if (info.isDirectory()) {
      if (ancestors.has(resolved)) throw new Error('Tool directory symlink cycle');
      const next = new Set(ancestors).add(resolved);
      await fs.mkdir(target, { recursive: true });
      for (const name of (await fs.readdir(resolved)).sort()) await copy(path.join(resolved, name), path.join(target, name), next);
    } else if (info.isFile()) {
      await fs.mkdir(path.dirname(target), { recursive: true });
      await fs.copyFile(resolved, target);
      await fs.chmod(target, info.mode & 0o111 ? 0o755 : 0o644);
    } else throw new Error('Tool tree contains a special file');
  }
  await copy(root, destination);
}

/** 工具候选只来自明确构建输入或约定安装位置，不扫描被分析项目。 */
async function findDirectory(candidates, label) {
  for (const candidate of candidates.filter(Boolean)) {
    try { if ((await fs.stat(candidate)).isDirectory()) return await fs.realpath(candidate); } catch (error) { if (error.code !== 'ENOENT') throw error; }
  }
  throw new Error(`${label} not installed; provide an isolated tool directory for local pack construction`);
}

/** 为原生组件验收读取独立测试 SDK 的引用目录；目标声明不再复制进支持包。 */
export async function csharpFixturePlatforms(sdk) {
  const base = path.join(sdk, 'packs/Microsoft.NETCore.App.Ref');
  const targets = new Map();
  for (const version of (await fs.readdir(base)).sort((a, b) => a.localeCompare(b, 'en', { numeric: true }))) {
    if (!/^[1-9]\d*\.0\.(0|[1-9]\d*)$/.test(version)) continue;
    const tfm = `net${version.split('.')[0]}.0`;
    const root = path.join(base, version, 'ref', tfm);
    try { await fs.access(path.join(root, 'System.Runtime.dll')); } catch (error) { if (error.code === 'ENOENT') continue; throw error; }
    targets.set(tfm, { id: `${tfm}-reference-assemblies`, kind: 'dotnet-reference-assemblies', version, root,
      metadata: { tfm, reference_pack: 'Microsoft.NETCore.App.Ref', reference_pack_version: version } });
  }
  return [...targets.values()];
}

/** 验收只描述明确准备的真实 JDK；产品的安装发现由 Java 程序负责。 */
export async function javaFixturePlatforms(roots) {
  const platforms = [];
  for (const root of [...new Set(roots.filter(Boolean))]) {
    const release = await fs.readFile(path.join(root, 'release'), 'utf8');
    const version = release.match(/^JAVA_VERSION="([^"]+)"/m)?.[1];
    const major = version?.startsWith('1.') ? version.split('.')[1] : version?.split('.')[0];
    if (!/^\d+$/.test(major ?? '')) throw new Error('Java fixture has no stable version');
    platforms.push({ id: `java${major}`, kind: 'java-jdk', version, root, metadata: { release: major } });
  }
  return platforms;
}

/** 新旧 Go SDK 作为真实版本样本传入，不随支持包复制标准源码。 */
export async function goFixturePlatforms(roots) {
  const platforms = [];
  for (const root of [...new Set(roots.filter(Boolean))]) {
    const version = (await fs.readFile(path.join(root, 'VERSION'), 'utf8')).split(/\r?\n/)[0].match(/^go(\d+\.\d+(?:\.\d+)?)$/)?.[1];
    if (!version) throw new Error('Go fixture has no stable version');
    platforms.push({ id: `go-sdk-${version}`, kind: 'go-sdk', version, root,
      metadata: { goos: process.platform === 'win32' ? 'windows' : 'darwin', goarch: process.arch === 'x64' ? 'amd64' : 'arm64', ...(process.arch === 'x64' ? { goamd64: 'v1' } : {}) } });
  }
  return platforms;
}

/** 实际唯一公开入口在空业务环境中接收无尾换行 JSON 后 EOF，并返回固定最终身份。 */
export async function smokeAnalyzer({ executable, args = ['analyze'], id, environment = {}, timeoutMs }) {
  const root = await fs.mkdtemp(path.join(os.tmpdir(), 'codegraph-direct-pack-smoke-'));
  let primaryError;
  try {
    const graph = await exchangeCli(executable, id, { protocol: 'codegraph.analyzer/1', type: 'analyze', id: 'pack-smoke', repository_root: root, files: [], options: { include_tests: false, rust_dependency_scope: 'repository-only' } }, await releaseIdentity(), undefined, { args, timeoutMs, cwd: root, environment: { ...(process.platform === 'win32' ? { SystemRoot: process.env.SystemRoot } : {}), TMPDIR: root, TMP: root, TEMP: root, ...environment } });
    assert.deepEqual(graph, { root_contains: [], nodes: [], external_symbols: [] }, 'Empty public graph mismatch');
  } catch (error) { primaryError = error; }
  try { await fs.rm(root, { recursive: true, force: true }); }
  catch (error) { if (primaryError) primaryError.message += `; scratch cleanup failed: ${error.message}`; else primaryError = error; }
  if (primaryError) throw primaryError;
}

/** 从实际目标的锁定依赖闭包收集许可；Rust 正常构建入口使用，后续 Python 可复用此归档步骤。 */
export async function packageCargoLicenses(source, target, payload, label) {
  // 构建自己的工具时允许获取锁定 metadata 所需的开发 crate；release 缓存不保证含它们。
  // 分发仍只收集下面的目标生产闭包，此步骤不参与用户项目的离线分析。
  const metadata = JSON.parse(execFileSync('bash', [path.join(repository, 'scripts/cargo-agent.sh'), 'metadata', '--locked', '--filter-platform', target, '--format-version', '1'], { cwd: source, encoding: 'utf8', maxBuffer: 16 * 1024 * 1024 }));
  const nodes = new Map(metadata.resolve.nodes.map(node => [node.id, node]));
  const included = new Set();
  const owner = metadata.packages.find(pkg => path.resolve(pkg.manifest_path) === path.resolve(source, 'Cargo.toml'));
  if (!owner) throw new Error('Cargo license owner not found');
  const pending = [owner.id];
  while (pending.length) {
    const id = pending.pop();
    if (included.has(id)) continue;
    included.add(id);
    for (const dependency of nodes.get(id).deps) {
      if (dependency.dep_kinds.some(kind => kind.kind !== 'dev')) pending.push(dependency.pkg);
    }
  }
  const fallbackRoot = path.join(source, 'licenses');
  let fallbacks = {};
  try { fallbacks = JSON.parse(await fs.readFile(path.join(fallbackRoot, 'fallbacks.json'), 'utf8')); }
  catch (error) { if (error.code !== 'ENOENT') throw error; }
  const notices = [];
  for (const pkg of metadata.packages.filter(pkg => included.has(pkg.id))) {
    const destination = path.join(payload, 'licenses', `${pkg.name}-${pkg.version}`);
    await fs.mkdir(destination, { recursive: true });
    const files = new Set();
    let directory = path.dirname(pkg.manifest_path);
    for (let level = 0; level < 5; level++) {
      const names = await fs.readdir(directory);
      for (const name of names.filter(name => /^(LICENSE|COPYING|NOTICE)([.-]|$)/i.test(name))) {
        if ((await fs.stat(path.join(directory, name))).isFile()) {
          await fs.copyFile(path.join(directory, name), path.join(destination, name));
          files.add(name);
        }
      }
      if (files.size || path.dirname(directory) === directory) break;
      directory = path.dirname(directory);
    }
    if (pkg.license_file) {
      const name = path.basename(pkg.license_file);
      await fs.copyFile(path.resolve(path.dirname(pkg.manifest_path), pkg.license_file), path.join(destination, name));
      files.add(name);
    }
    if (!files.size && pkg.license) {
      // 缺文本的精确锁定版本只能使用已保存一手出处的原许可，不猜测作者或许可模板。
      const names = fallbacks[`${pkg.name}@${pkg.version}`];
      if (!Array.isArray(names) || !names.length) throw new Error(`Locked crate has no distributable license text: ${pkg.name}`);
      for (const name of names) {
        await fs.copyFile(path.join(fallbackRoot, name), path.join(destination, name));
        files.add(name);
      }
    }
    notices.push({ name: pkg.name, version: pkg.version, license: pkg.license, source: pkg.source, repository: pkg.repository, authors: pkg.authors, files: [...files].sort() });
  }
  if (Object.keys(fallbacks).length) await fs.copyFile(path.join(fallbackRoot, 'sources.json'), path.join(payload, `licenses/${label}-license-sources.json`));
  await fs.writeFile(path.join(payload, `licenses/${label}-dependencies.json`), JSON.stringify(notices, null, 2));
}

/** 原生程序与随包运行依赖共同暂存；分析入口永远是固定的 bin/codegraph-<id>。 */
export async function buildNativePack({ id, target, output, definition, packageDirectory }) {
  if (!Object.hasOwn(migratedNativePacks, id) || id === 'typescript-javascript') throw new Error('Unknown native pack');
  if (target !== nativeTarget()) throw new Error('Native pack must be built and smoke-tested on its target OS and CPU');
  output = path.resolve(output);
  await fs.mkdir(output, { recursive: true });
  const temporary = await fs.mkdtemp(path.join(output, '.native-build-'));
  const payload = path.join(temporary, 'payload');
  const windows = process.platform === 'win32', suffix = windows ? '.exe' : '';
  const platforms = [];
  try {
    for (const part of ['bin', 'runtime', 'licenses', 'control']) await fs.mkdir(path.join(payload, part), { recursive: true });
    if (id === 'rust' || id === 'python') {
      if (id === 'python') {
        for (const name of ['ruff-LICENSE', 'typeshed-LICENSE']) await fs.copyFile(path.join(repository, 'analyzers/python/licenses', name), path.join(payload, 'licenses', name));
        platforms.push({ id: 'python-stdlib', kind: 'python-embedded-stdlib', version: definition.ty, root: 'bin', metadata: { source_revision: definition.sourceCommit } });
      }
    } else if (id === 'go') {
      const sdk = await findDirectory([process.env.CODEGRAPH_GO_SDK, windows ? 'C:\\Program Files\\Go' : '/usr/local/go'], `Go ${definition.go} build SDK`);
      if (!(await fs.readFile(path.join(sdk, 'VERSION'), 'utf8')).startsWith(`go${definition.go}\n`)) throw new Error('Go SDK version mismatch');
      execFileSync(path.join(sdk, `bin/go${suffix}`), ['build', '-trimpath', '-o', path.join(payload, `bin/codegraph-go${suffix}`), '.'], { cwd: path.join(repository, 'analyzers/go'), stdio: 'inherit', env: { ...process.env, GOROOT: sdk, GOTOOLCHAIN: 'local', GOWORK: 'off', GOPROXY: 'off', GOSUMDB: 'off' } });
      await fs.copyFile(path.join(sdk, 'LICENSE'), path.join(payload, 'licenses/Go-LICENSE'));
    } else if (id === 'csharp') {
      const sdk = await findDirectory([process.env.CODEGRAPH_DOTNET_SDK, '/usr/local/share/dotnet', windows && 'C:\\Program Files\\dotnet'], '.NET 10.0.100 SDK');
      const tool = path.join(sdk, `dotnet${suffix}`);
      const env = { ...process.env, DOTNET_ROOT: sdk, DOTNET_CLI_TELEMETRY_OPTOUT: '1', DOTNET_SKIP_FIRST_TIME_EXPERIENCE: '1', DOTNET_GENERATE_ASPNET_CERTIFICATE: 'false' };
      if (execFileSync(tool, ['--version'], { encoding: 'utf8', env, cwd: path.join(repository, 'analyzers/csharp') }).trim() !== definition.dotnetSdk) throw new Error('.NET SDK version mismatch');
      const runtime = await prepareRuntimePack(target, temporary, env);
      const publish = path.join(temporary, 'publish');
      execFileSync(tool, ['publish', path.join(repository, 'analyzers/csharp/CodeGraph.CSharp.Engine.csproj'), '-c', 'Release', '-r', runtime.entry.rid, '--self-contained', 'true', '-p:RuntimeFrameworkVersion=10.0.0', '-p:TargetLatestRuntimePatch=false', '-p:DisableTransitiveFrameworkReferenceDownloads=true', '--source', runtime.source, '-o', publish, '--nologo'], { stdio: 'inherit', env });
      await verifyPublishedRuntime(publish, runtime, temporary);
      await copyToolTree(publish, path.join(payload, 'bin'));
      for (const name of ['LICENSE.txt', 'ThirdPartyNotices.txt']) await fs.copyFile(path.join(sdk, name), path.join(payload, 'licenses', name));
      for (const name of ['LICENSE.TXT', 'THIRD-PARTY-NOTICES.TXT']) await fs.copyFile(path.join(runtime.notices, name), path.join(payload, 'licenses', `Runtime-${name}`));
    } else if (id === 'java') {
      const runtime = await findDirectory([process.env.CODEGRAPH_JAVA_RUNTIME, path.join(repository, '.tools/java-native/jdk-25.0.4.1+1/Contents/Home')], 'Pinned Java runtime');
      if (!(await fs.readFile(path.join(runtime, 'release'), 'utf8')).includes(`IMPLEMENTOR_VERSION="Temurin-${definition.runtimeJava}"`)) throw new Error('Java runtime version mismatch');
      execFileSync(process.env.CODEGRAPH_PYTHON ?? process.env.PYTHON ?? (windows ? 'python' : 'python3'), [path.join(repository, 'analyzers/java/build.py'), runtime, path.join(repository, '.tools/java-native/lib')], { stdio: 'inherit', env: { ...process.env, CODEGRAPH_JAVA_RUNTIME: runtime } });
      await copyToolTree(runtime, path.join(payload, 'runtime/jdk'));
      await copyToolTree(path.join(repository, 'analyzers/java/build/classes'), path.join(payload, 'runtime/analyzer/classes'));
      await copyToolTree(path.join(repository, '.tools/java-native/lib'), path.join(payload, 'runtime/analyzer/lib'));
      await copyToolTree(path.join(repository, 'analyzers/java/dependencies.lock.json'), path.join(payload, 'licenses/Java-dependencies.json'));
      await copyToolTree(path.join(runtime, 'NOTICE'), path.join(payload, 'licenses/Java-runtime-NOTICE'));
    } else {
      const sdk = await findDirectory([process.env.CODEGRAPH_LLVM_SDK], 'LLVM 21.1.8 SDK');
      const source = path.join(repository, 'analyzers/clang'), build = path.join(source, 'build');
      if (windows) {
        for (const name of ['CODEGRAPH_MSVC_VERSION', 'CODEGRAPH_VISUAL_STUDIO', 'CODEGRAPH_MSVC_REDIST']) if (!process.env[name]) throw new Error(`Missing fixed Windows toolchain input: ${name}`);
        const cmake = process.env.CODEGRAPH_CMAKE || 'cmake';
        execFileSync(cmake, ['-S', source, '-B', build, '-G', 'Visual Studio 17 2022', '-A', 'x64', '-T', `v143,version=${process.env.CODEGRAPH_MSVC_VERSION}`, '-DCMAKE_SYSTEM_VERSION=10.0.26100.0', `-DCMAKE_GENERATOR_INSTANCE=${process.env.CODEGRAPH_VISUAL_STUDIO}`, `-DLLVM_DIR=${path.join(sdk, 'lib/cmake/llvm')}`, `-DClang_DIR=${path.join(sdk, 'lib/cmake/clang')}`, '-DCODEGRAPH_BUILD_COMPONENT_TESTS=OFF'], { stdio: 'inherit' });
        execFileSync(cmake, ['--build', build, '--config', 'Release', '--parallel', process.env.CMAKE_BUILD_PARALLEL_LEVEL || '2'], { stdio: 'inherit' });
        // CMake 邻接输出是实际 LLVM DLL 闭包；CRT 只来自已核验的 VC143 目录。
        const dlls = (await fs.readFile(path.join(build, 'runtime-dlls-Release.txt'), 'utf8')).split(/\r?\n/).filter(Boolean);
        for (const dll of dlls) {
          const relative = path.relative(await fs.realpath(sdk), await fs.realpath(dll));
          if (!path.isAbsolute(dll) || relative === '..' || relative.startsWith(`..${path.sep}`) || path.isAbsolute(relative) || !dll.toLowerCase().endsWith('.dll')) throw new Error('CMake runtime DLL escapes pinned LLVM SDK');
          await copyToolTree(dll, path.join(payload, 'bin', path.basename(dll)));
        }
        for (const name of await fs.readdir(process.env.CODEGRAPH_MSVC_REDIST)) if (name.toLowerCase().endsWith('.dll')) await copyToolTree(path.join(process.env.CODEGRAPH_MSVC_REDIST, name), path.join(payload, 'bin', name));
      } else execFileSync('bash', [path.join(source, 'build.sh'), sdk], { stdio: 'inherit', env: { ...process.env, CODEGRAPH_BUILD_COMPONENT_TESTS: '0' } });
      await fs.copyFile(path.join(build, `codegraph-c-cpp${suffix}`), path.join(payload, `bin/codegraph-c-cpp${suffix}`));
      await copyToolTree(path.join(sdk, 'lib/clang/21'), path.join(payload, 'runtime/platform/clang-resource'));
      if (!windows) await copyToolTree(path.join(build, 'runtime/lib'), path.join(payload, 'runtime/lib'));
      for (const name of ['LLVM', 'Clang']) await copyToolTree(path.join(source, `LICENSE-${name}.txt`), path.join(payload, `licenses/${name}-LICENSE`));
      platforms.push({ id: 'clang-resource-21', kind: 'clang-resource', version: definition.llvm, root: 'runtime/platform/clang-resource', metadata: {} });
    }
    const kind = { java: 'java', go: 'go', csharp: 'dotnet', 'c-cpp': windows ? 'windows-msvc' : 'macos' }[id];
    await fs.writeFile(path.join(payload, 'control/native.json'), `${JSON.stringify({ schema_version: 1, helper_version: definition.version, platforms, ...(kind ? { target_sdk: { kind } } : {}) })}\n`);
    await stageAnalyzer(payload, id, target);
    await smokeAnalyzer({ executable: path.join(payload, `bin/codegraph-${id}${suffix}`), id });
    const destination = await packageDirectory({ id, version: definition.version, target }, payload, output);
    await verifyAnalyzerDistribution(destination, { id, version: definition.version, target });
    const extracted = path.join(output, 'smoke-installed', id, definition.version, target);
    await fs.mkdir(path.dirname(extracted), { recursive: true });
    await verifyAndExtract(destination, extracted);
    await smokeAnalyzer({ executable: path.join(extracted, `bin/codegraph-${id}${suffix}`), id });
    console.log(`Built and public-smoke-tested ${id}@${definition.version}: ${destination}`);
    return destination;
  } finally { await fs.rm(temporary, { recursive: true, force: true }); }
}
