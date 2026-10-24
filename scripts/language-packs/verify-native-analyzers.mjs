import { selectedNativePacks } from './native-migration.mjs';
import fs from 'node:fs/promises';
import { buildPack } from './build-language-support-packs.mjs';
import { writeVerificationEvidence } from '../release/release-artifacts.mjs';
import path from 'node:path';
import { spawn } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import { prepareRustNativeTests } from './prepare-rust-native-tests.mjs';
import { nativeTargets } from './provision-native-toolchains.mjs';
import { verificationCommands } from './verification-commands.mjs';
export { verificationCommands } from './verification-commands.mjs';

const repository = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../..');

/** 完整套件先核对安装入口和真实 SDK 夹具，缺少任何输入都不能靠 skip 通过。 */
export async function validateNativeFixtures(environment, language = 'all') {
  const ids = selectedNativePacks(language), suffix = process.platform === 'win32' ? '.exe' : '';
  const target = nativeTargets[`${process.platform}-${process.arch}`];
  for (const id of ids) {
    const executable = path.join(environment.CODEGRAPH_NATIVE_PACKS, 'smoke-installed', id, '0.1.0', target, 'bin', `codegraph-${id}${suffix}`);
    if (!(await fs.stat(executable)).isFile()) throw new Error(`Missing direct analyzer fixture: ${id}`);
  }
  const required = [];
  if (ids.includes('go')) for (const root of [environment.CODEGRAPH_GO_SDK, environment.CODEGRAPH_GO_TARGET_SDK]) for (const file of ['VERSION', 'src/builtin/builtin.go', 'src/runtime/runtime.go']) required.push(path.join(root, file));
  if (ids.includes('java')) required.push(path.join(environment.CODEGRAPH_JAVA_TARGET_JDK, 'release'), path.join(environment.CODEGRAPH_JAVA_TARGET_JDK, 'lib/jrt-fs.jar'));
  if (ids.includes('java')) required.push(path.join(environment.CODEGRAPH_JAVA_TARGET_8, 'jre/lib/rt.jar'), path.join(repository, 'analyzers/java/build/test-classes/dev/codegraph/java/ComponentDriver.class'));
  if (ids.includes('csharp')) {
    required.push(path.join(repository, 'analyzers/csharp/tests/bin/Release/net10.0/CodeGraph.CSharp.Components.dll'));
    for (const [root, tfm] of [[environment.CODEGRAPH_DOTNET_REF8_ROOT, 'net8.0'], [environment.CODEGRAPH_DOTNET_REF9_ROOT, 'net9.0'], [path.join(environment.CODEGRAPH_DOTNET_SDK, 'packs/Microsoft.NETCore.App.Ref/10.0.0'), 'net10.0']]) required.push(path.join(root, 'ref', tfm, 'System.Runtime.dll'));
  }
  if (ids.includes('c-cpp')) required.push(path.join(repository, `analyzers/clang/build/codegraph-clang-components${suffix}`));
  if (ids.includes('c-cpp') && process.platform === 'win32') for (const key of ['CODEGRAPH_WINDOWS_SDK', 'CODEGRAPH_MSVC_ROOT']) if (!environment[key] || !(await fs.stat(environment[key])).isDirectory()) throw new Error(`Missing Windows SDK fixture: ${key}`);
  for (const file of required) if (!(await fs.stat(file)).isFile()) throw new Error(`Missing requested fixture: ${file}`);
}

/** 构建非交付的同进程语义驱动，与 payload 中的生产入口保持严格目录边界。 */
async function prepareComponentDrivers(ids, environment) {
  const suffix = process.platform === 'win32' ? '.exe' : '';
  if (ids.includes('java')) await execute({ executable: environment.CODEGRAPH_PYTHON ?? (process.platform === 'win32' ? 'python' : 'python3'), args: ['analyzers/java/build.py', environment.CODEGRAPH_JAVA_RUNTIME, path.join(repository, '.tools/java-native/lib'), '--tests'] }, environment);
  if (ids.includes('csharp')) await execute({ executable: path.join(environment.CODEGRAPH_DOTNET_SDK, `dotnet${suffix}`), args: ['build', 'analyzers/csharp/tests/Components.csproj', '-c', 'Release', '--nologo'], environment: { DOTNET_ROOT: environment.CODEGRAPH_DOTNET_SDK } }, environment);
  if (ids.includes('c-cpp')) {
    if (process.platform === 'win32') {
      await execute({ executable: environment.CODEGRAPH_CMAKE || 'cmake', args: ['-S', 'analyzers/clang', '-B', 'analyzers/clang/build', '-DCODEGRAPH_BUILD_COMPONENT_TESTS=ON'] }, environment);
      await execute({ executable: environment.CODEGRAPH_CMAKE || 'cmake', args: ['--build', 'analyzers/clang/build', '--config', 'Release', '--parallel', environment.CMAKE_BUILD_PARALLEL_LEVEL || '2'] }, environment);
    } else await execute({ executable: 'bash', args: ['analyzers/clang/build.sh', environment.CODEGRAPH_LLVM_SDK], environment: { CODEGRAPH_BUILD_COMPONENT_TESTS: '1' } }, environment);
  }
}

/** 原样传递参数而不经过 shell，保留 Windows 路径中的空格与分号。 */
async function execute(command, environment) {
  console.log(`Native validation: ${command.executable} ${command.args.join(' ')}`);
  const child = spawn(command.executable, command.args, { cwd: path.resolve(repository, command.cwd ?? '.'), stdio: ['ignore', 'pipe', 'inherit'], env: { ...environment, ...command.environment } });
  let output = '';
  child.stdout.on('data', chunk => { process.stdout.write(chunk); output += chunk; });
  await new Promise((resolve, reject) => {
    child.once('error', reject);
    child.once('close', code => code === 0 ? resolve() : reject(new Error(`Native validation failed (${code}): ${command.executable}`)));
  });
  for (const test of command.requiredTests ?? []) if (!output.includes(`test ${test} ... ok`)) throw new Error(`Required semantic test did not pass: ${test}`);
  if (command.executable === 'bash' && command.args.includes('test') && !/test result: ok\. [1-9]\d* passed;/.test(output)) throw new Error('Cargo validation ran zero tests');
}

/** 串行完成全部独立验收组，再汇总命令身份及原始失败；先决构建和配置不走此入口。 */
export async function runVerificationCommands(commands, environment, run = execute) {
  const failures = [];
  for (const command of commands) {
    try {
      await run(command, environment);
    } catch (cause) {
      const failure = new Error(`Native validation group failed: ${command.executable} ${command.args.join(' ')}`, { cause });
      failures.push(failure);
      console.error(failure);
    }
  }
  if (failures.length) throw new AggregateError(failures, `${failures.length} native validation group(s) failed`);
}

/** CI 从精确源码构建七包，再串行完成全部原生、公共和调用方验收，最后绑定不可变字节。 */
export async function verifyNativeAnalyzers(artifacts, build = false, language = 'all') {
  const ids = selectedNativePacks(language), target = nativeTargets[`${process.platform}-${process.arch}`];
  if (!target) throw new Error('Native validation requires macOS ARM64 or Windows x64');
  artifacts = path.resolve(artifacts);
  const environment = { ...process.env, CODEGRAPH_TEST_NODE: process.execPath, CODEGRAPH_NATIVE_LANGUAGE: language, CODEGRAPH_NATIVE_PACKS: artifacts };
  for (const id of ids) {
    const pack = path.join(artifacts, 'smoke-installed', id, '0.1.0', target);
    if (id === 'typescript-javascript') environment.CODEGRAPH_TYPESCRIPT_PACK = pack;
  }
  if (ids.includes('rust')) Object.assign(environment, await prepareRustNativeTests(environment));
  const commands = verificationCommands(environment, process.platform, language);
  await fs.mkdir(artifacts, { recursive: true });
  // 已有 receipt 不覆盖；每个精确 HEAD 使用新的 output。
  try { await fs.access(path.join(artifacts, `verification-${target}.json`)); throw new Error('Verification receipt already exists'); } catch (error) { if (error.code !== 'ENOENT') throw error; }
  if (build) {
    if (ids.includes('java')) await execute({ executable: environment.CODEGRAPH_PYTHON ?? (process.platform === 'win32' ? 'python' : 'python3'), args: ['analyzers/java/tools/fetch-deps.py'] }, environment);
    for (const id of ids) await buildPack({ id, target, output: artifacts });
    await prepareComponentDrivers(ids, environment);
  }
  await validateNativeFixtures(environment, language);
  await runVerificationCommands(commands, environment);
  if (language === 'all') await writeVerificationEvidence(artifacts, commands, environment);
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const language = process.argv.includes('--language') ? process.argv[process.argv.indexOf('--language') + 1] : 'all';
  if (!language) throw new Error('--language requires a completed language or all');
  await verifyNativeAnalyzers(process.argv[2] ?? 'artifacts', process.argv.includes('--build'), language);
}
