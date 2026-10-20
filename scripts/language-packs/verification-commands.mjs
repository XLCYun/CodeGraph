import path from 'node:path';
import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { isDeepStrictEqual } from 'node:util';
import { selectedNativePacks } from './native-migration.mjs';

const repository = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../..');
const semanticTests = JSON.parse(readFileSync(new URL('../../resources/semantic-acceptance.json', import.meta.url), 'utf8'));

/** 明确列出串行验收命令；所有 Cargo 入口统一经过仓库 wrapper。 */
export function verificationCommands(environment, platform = process.platform, language = 'all', { node = process.execPath, root = repository, paths = path } = {}) {
  const ids = selectedNativePacks(language);
  const required = { python: [], rust: ['PROJECT_RUST_TOOLCHAIN', 'TEST_CARGO_TOOLCHAINS', 'TEST_CARGO_194'], 'typescript-javascript': [], go: ['GO_SDK', 'GO_TARGET_SDK'], java: ['JAVA_RUNTIME', 'JAVA_TARGET_JDK', 'JAVA_TARGET_8'], csharp: ['DOTNET_SDK', 'DOTNET_REF8_ROOT', 'DOTNET_REF9_ROOT'], 'c-cpp': ['LLVM_SDK'] };
  const suffix = platform === 'win32' ? '.exe' : '';
  const python = environment.CODEGRAPH_PYTHON ?? (platform === 'win32' ? 'python' : 'python3');
  for (const name of ids.flatMap(id => required[id].map(name => `CODEGRAPH_${name}`))) if (!environment[name]) throw new Error(`Missing fixed toolchain: ${name}`);
  const cargo = ['scripts/cargo-agent.sh', 'test', '--locked'];
  // Git Bash 会丢失内联 TOML 中一层反斜杠转义；Windows 接受正斜杠路径，Unix 路径保持原样。
  const rustTestRunner = `target.'cfg(all())'.runner=${JSON.stringify([node, paths.join(root, 'scripts/language-packs/run-rust-helper-test.mjs')].map(file => platform === 'win32' ? file.replaceAll('\\', '/') : file))}`;
  const commands = [
    ...(ids.includes('go') ? [{ executable: paths.join(environment.CODEGRAPH_GO_SDK, `bin/go${suffix}`), args: ['test', './...'], cwd: 'analyzers/go', environment: { GOROOT: environment.CODEGRAPH_GO_SDK, GOTOOLCHAIN: 'local', GOWORK: 'off', GOPROXY: 'off', GOSUMDB: 'off', CODEGRAPH_GO_TEST_SDKS: [environment.CODEGRAPH_GO_TARGET_SDK, environment.CODEGRAPH_GO_SDK].join(platform === 'win32' ? ';' : ':') } }] : []),
    ...(ids.includes('java') ? [{ executable: python, args: ['-m', 'unittest', 'discover', '-s', 'analyzers/java/tests', '-v'], environment: { CODEGRAPH_JAVA_TARGET: environment.CODEGRAPH_JAVA_TARGET_JDK, CODEGRAPH_JAVA_LIB: paths.join(root, '.tools/java-native/lib') } }] : []),
    ...(ids.includes('csharp') ? [
      { executable: python, args: ['-m', 'unittest', 'discover', '-s', 'analyzers/csharp/tests', '-v'], environment: { CODEGRAPH_TEST_DOTNET: paths.join(environment.CODEGRAPH_DOTNET_SDK, `dotnet${suffix}`), CODEGRAPH_TEST_REFS: paths.join(environment.CODEGRAPH_DOTNET_SDK, 'packs/Microsoft.NETCore.App.Ref/10.0.0/ref/net10.0'), CODEGRAPH_TEST_REFS8: paths.join(environment.CODEGRAPH_DOTNET_REF8_ROOT, 'ref/net8.0'), CODEGRAPH_TEST_REFS9: paths.join(environment.CODEGRAPH_DOTNET_REF9_ROOT, 'ref/net9.0'), CODEGRAPH_TEST_REFS9_VERSION: '9.0.20' } },
      { executable: 'bash', args: [...cargo, '--lib', 'adapters::csharp_graph::', '--', '--ignored', '--test-threads=1'] },
    ] : []),
    ...(ids.includes('c-cpp') ? [{ executable: python, args: ['-m', 'unittest', 'discover', '-s', 'analyzers/clang/tests', '-v'], environment: { CODEGRAPH_LLVM_ROOT: environment.CODEGRAPH_LLVM_SDK, CODEGRAPH_CLANG_HELPER: paths.join(root, `analyzers/clang/build/codegraph-clang-engine${suffix}`) } }] : []),
    // 真实协议子进程与 CPU 生命周期夹具按文件串行，避免准备测试本身耗尽退出观察期限。
    ...(ids.includes('typescript-javascript') ? [
      { executable: node, args: ['--test', '--test-concurrency=1', 'analyzers/typescript/tests/projection.test.mjs', 'analyzers/typescript/tests/occurrences.test.mjs', 'analyzers/typescript/tests/config.test.mjs', 'analyzers/typescript/tests/workspace.test.mjs', 'analyzers/typescript/tests/helper.test.mjs', 'analyzers/typescript/tests/syntax.test.mjs', 'analyzers/typescript/tests/lifecycle.test.mjs', 'analyzers/typescript/tests/output.test.mjs', 'analyzers/typescript/tests/protocol.test.mjs'] },
      { executable: 'bash', args: [...cargo, '--lib', 'adapters::typescript_graph::', '--', '--ignored', '--test-threads=4'] },
    ] : []),
    ...(ids.includes('rust') ? [
      { executable: 'bash', args: ['scripts/cargo-agent.sh', '+1.98.0', 'test', '--manifest-path', 'analyzers/rust/Cargo.toml', '--config', rustTestRunner, '--locked', '--no-fail-fast', '--lib', '--test', 'initial_protocol', '--test', 'lifecycle', '--', '--include-ignored', '--test-threads=1'] },
      { executable: 'bash', args: [...cargo, '--lib', 'native_analyzer::tests::rust_graph::', '--', '--ignored', '--test-threads=1'] },
      { executable: 'bash', args: [...cargo, '--lib', 'native_analyzer::tests::rust_syntax::', '--', '--ignored', '--test-threads=1'] },
      // 原生产探针夹具声明 cfg(unix)；Windows 的实际取消/Job 验收仍在公共 owner 与 CLI 组。
      ...(platform === 'darwin' ? [{ executable: 'bash', args: [...cargo, '--lib', 'adapters::rust::tests::', '--', '--ignored', '--test-threads=1'] }] : []),
    ] : []),
    ...(ids.includes('python') ? [
      { executable: 'bash', args: ['scripts/cargo-agent.sh', '+1.98.0', 'test', '--manifest-path', 'analyzers/python/Cargo.toml', '--locked', '--lib', '--test', 'initial_protocol', '--test', 'lifecycle', '--', '--test-threads=1'] },
      { executable: 'bash', args: [...cargo, '--lib', 'native_analyzer::tests::python_graph::', '--', '--ignored', '--test-threads=1'] },
      { executable: 'bash', args: [...cargo, '--lib', 'native_analyzer::tests::python_native::', '--', '--ignored', '--test-threads=1'] },
    ] : []),
    // 私有运行时的完整非 ignored 单元测试覆盖图、Host、SDK 工厂和进程所有者。
    { executable: 'bash', args: [...cargo, '--lib', '--', '--test-threads=1'] },
    { executable: 'bash', args: [...cargo, '--test', 'cli', '--', '--test-threads=1'] },
    { executable: node, args: ['scripts/language-packs/verify-public-cli.mjs', environment.CODEGRAPH_NATIVE_PACKS ?? 'artifacts', language] },
  ];
  for (const command of commands) {
    const filter = command.args.find(arg => (arg.startsWith('native_') || arg.startsWith('adapters::')) && arg.endsWith('::'));
    if (filter) command.requiredTests = semanticTests.filter(test => test.name.startsWith(filter) && test.platforms.includes(platform)).map(test => test.name);
    if (filter && !command.requiredTests.length) throw new Error(`Missing semantic acceptance inventory: ${filter}`);
  }
  return commands;
}

/** 同一命令规范生成跨 runner 可比较的身份；占位路径只记录身份，绝不执行。 */
export function verificationCommandIdentities(platform) {
  if (!['darwin', 'win32'].includes(platform)) throw new Error('Unsupported verification platform');
  const names = ['GO_SDK', 'GO_TARGET_SDK', 'DOTNET_SDK', 'DOTNET_REF8_ROOT', 'DOTNET_REF9_ROOT', 'JAVA_RUNTIME', 'JAVA_TARGET_JDK', 'JAVA_TARGET_8', 'LLVM_SDK', 'PROJECT_RUST_TOOLCHAIN', 'TEST_CARGO_TOOLCHAINS', 'TEST_CARGO_194'];
  const environment = Object.fromEntries(names.map(name => [`CODEGRAPH_${name}`, '${' + name + '}']));
  environment.CODEGRAPH_PYTHON = '${PYTHON}';
  environment.CODEGRAPH_NATIVE_PACKS = '${ARTIFACTS}';
  return verificationCommands(environment, platform, 'all', { node: '${NODE}', root: '${REPOSITORY}', paths: path.posix });
}

/** 先核对实际执行列表的完整 argv/cwd/env，再写同一规范的便携身份，不接受子集或替换。 */
export function recordVerifiedCommands(commands, environment, platform = process.platform) {
  if (!isDeepStrictEqual(commands, verificationCommands(environment, platform, 'all'))) throw new Error('Incomplete verification command identity');
  return verificationCommandIdentities(platform).map(command => ({ ...command, outcome: 'passed' }));
}
