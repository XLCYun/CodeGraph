import path from 'node:path';
import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { isDeepStrictEqual } from 'node:util';
import { selectedNativePacks } from './native-migration.mjs';

const repository = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../..');
const semanticTests = JSON.parse(readFileSync(new URL('../../resources/semantic-acceptance.json', import.meta.url), 'utf8'));

/** 全套包含各语言组件、实际公共入口和测试调用方；所有 Cargo 构建经过同一全局锁。 */
export function verificationCommands(environment, platform = process.platform, language = 'all', { node = process.execPath, root = repository, paths = path } = {}) {
  const ids = selectedNativePacks(language);
  const required = { python: [], rust: ['PROJECT_RUST_TOOLCHAIN', 'TEST_CARGO_TOOLCHAINS', 'TEST_CARGO_194'], 'typescript-javascript': [], go: ['GO_SDK', 'GO_TARGET_SDK'], java: ['JAVA_RUNTIME', 'JAVA_TARGET_JDK', 'JAVA_TARGET_8'], csharp: ['DOTNET_SDK', 'DOTNET_REF8_ROOT', 'DOTNET_REF9_ROOT'], 'c-cpp': ['LLVM_SDK'] };
  const suffix = platform === 'win32' ? '.exe' : '';
  const python = environment.CODEGRAPH_PYTHON ?? (platform === 'win32' ? 'python' : 'python3');
  for (const name of ids.flatMap(id => required[id].map(name => `CODEGRAPH_${name}`))) if (!environment[name]) throw new Error(`Missing fixed toolchain: ${name}`);
  const cargo = ['scripts/cargo-agent.sh', 'test', '--locked'];
  const target = platform === 'win32' ? 'x86_64-pc-windows-msvc' : 'aarch64-apple-darwin';
  const installed = id => paths.join(environment.CODEGRAPH_NATIVE_PACKS ?? 'artifacts', 'smoke-installed', id, '0.1.1', target, 'bin', `codegraph-${id}${suffix}`);
  // TOML 参数不经过 shell，Windows 用正斜杠避免 Git Bash 再解释反斜杠。
  const rustTestRunner = `target.'cfg(all())'.runner=${JSON.stringify([node, paths.join(root, 'scripts/language-packs/run-rust-helper-test.mjs')].map(file => platform === 'win32' ? file.replaceAll('\\', '/') : file))}`;
  const commands = [
    ...(ids.includes('go') ? [
      { executable: paths.join(environment.CODEGRAPH_GO_SDK, `bin/go${suffix}`), args: ['test', './...'], cwd: 'analyzers/go', environment: { GOROOT: environment.CODEGRAPH_GO_SDK, GOTOOLCHAIN: 'local', GOWORK: 'off', GOPROXY: 'off', GOSUMDB: 'off', CODEGRAPH_GO_TEST_SDKS: [environment.CODEGRAPH_GO_TARGET_SDK, environment.CODEGRAPH_GO_SDK].join(platform === 'win32' ? ';' : ':') } },
      { executable: python, args: ['-m', 'unittest', 'discover', '-s', 'analyzers/go/tests', '-v'], environment: { CODEGRAPH_GO_ANALYZER: installed('go'), CODEGRAPH_TEST_TARGET_SDK: environment.CODEGRAPH_GO_TARGET_SDK } },
    ] : []),
    ...(ids.includes('java') ? [{ executable: python, args: ['-m', 'unittest', 'discover', '-s', 'analyzers/java/tests', '-v'], environment: { CODEGRAPH_JAVA_TARGET: environment.CODEGRAPH_JAVA_TARGET_JDK, CODEGRAPH_JAVA_LIB: paths.join(root, '.tools/java-native/lib') } }] : []),
    ...(ids.includes('csharp') ? [{ executable: python, args: ['-m', 'unittest', 'discover', '-s', 'analyzers/csharp/tests', '-v'], environment: { CODEGRAPH_CSHARP_ANALYZER: installed('csharp'), CODEGRAPH_TEST_TARGET_SDK: environment.CODEGRAPH_DOTNET_SDK, CODEGRAPH_TEST_DOTNET: paths.join(environment.CODEGRAPH_DOTNET_SDK, `dotnet${suffix}`), CODEGRAPH_TEST_REFS: paths.join(environment.CODEGRAPH_DOTNET_SDK, 'packs/Microsoft.NETCore.App.Ref/10.0.0/ref/net10.0'), CODEGRAPH_TEST_REFS8: paths.join(environment.CODEGRAPH_DOTNET_REF8_ROOT, 'ref/net8.0'), CODEGRAPH_TEST_REFS9: paths.join(environment.CODEGRAPH_DOTNET_REF9_ROOT, 'ref/net9.0'), CODEGRAPH_TEST_REFS9_VERSION: '9.0.20' } }] : []),
    ...(ids.includes('c-cpp') ? [{ executable: python, args: ['-m', 'unittest', 'discover', '-s', 'analyzers/clang/tests', '-v'], environment: { CODEGRAPH_LLVM_ROOT: environment.CODEGRAPH_LLVM_SDK, CODEGRAPH_CLANG_HELPER: paths.join(root, `analyzers/clang/build/codegraph-clang-components${suffix}`), CODEGRAPH_CLANG_ANALYZER: installed('c-cpp') } }] : []),
    ...(ids.includes('typescript-javascript') ? [{ executable: node, args: ['--test', '--test-concurrency=1', ...['projection', 'occurrences', 'config', 'workspace', 'helper', 'syntax', 'lifecycle', 'output', 'protocol', 'graph', 'public', 'historical'].map(name => `analyzers/typescript/tests/${name}.test.mjs`)] }] : []),
    ...(ids.includes('rust') ? [{ executable: 'bash', args: ['scripts/cargo-agent.sh', '+1.98.0', 'test', '--manifest-path', 'analyzers/rust/Cargo.toml', '--config', rustTestRunner, '--locked', '--no-fail-fast', '--lib', '--test', 'initial_protocol', '--test', 'lifecycle', '--', '--include-ignored', '--test-threads=1'] }] : []),
    ...(ids.includes('python') ? [{ executable: 'bash', args: ['scripts/cargo-agent.sh', '+1.98.0', 'test', '--manifest-path', 'analyzers/python/Cargo.toml', '--locked', '--lib', '--test', 'initial_protocol', '--test', 'lifecycle', '--', '--test-threads=1'] }] : []),
    // 每个旧语义断言绑定到实际可运行的新公共入口测试；未成功逐名输出即失败。
    ...ids.map(id => ({ executable: 'bash', args: [...cargo, '--manifest-path', 'analyzers/integration-tests/Cargo.toml', '--lib', '--', '--ignored', '--test-threads=1'], requiredTests: semanticTests.filter(test => test.language === id && test.platforms.includes(platform)).map(test => test.name), environment: { CODEGRAPH_INTEGRATION_LANGUAGE: id } })),
    { executable: 'bash', args: ['scripts/cargo-agent.sh', '+1.98.0', 'test', '--locked', '--manifest-path', 'analyzers/common/Cargo.toml', '--lib'] },
    { executable: 'bash', args: [...cargo, '--workspace', '--', '--test-threads=1'] },
    { executable: 'bash', args: [...cargo, '--manifest-path', 'analyzers/windows-process-probe/Cargo.toml', '--lib', 'child_process::', '--', '--test-threads=1'] },
    { executable: node, args: ['scripts/language-packs/verify-public-cli.mjs', environment.CODEGRAPH_NATIVE_PACKS ?? 'artifacts', language] },
  ];
  // libtest 只支持一个名字筛选器；相同语言的测试共享可识别的模块前缀。
  for (const command of commands.filter(command => command.environment?.CODEGRAPH_INTEGRATION_LANGUAGE)) {
    const id = command.environment.CODEGRAPH_INTEGRATION_LANGUAGE;
    const filter = { rust: 'tests::rust_', python: 'tests::python_', 'typescript-javascript': 'tests::typescript_graph::', go: 'tests::directory::go::', java: 'tests::directory::java::', csharp: 'csharp', 'c-cpp': 'tests::directory::clang::' }[id];
    command.args.splice(command.args.indexOf('--'), 0, filter);
    if (!command.requiredTests.length) throw new Error(`Missing semantic acceptance inventory: ${id}`);
  }
  return commands;
}

/** 跨 runner 记录相同命令规范，路径占位仅作身份，不执行。 */
export function verificationCommandIdentities(platform) {
  if (!['darwin', 'win32'].includes(platform)) throw new Error('Unsupported verification platform');
  const names = ['GO_SDK', 'GO_TARGET_SDK', 'DOTNET_SDK', 'DOTNET_REF8_ROOT', 'DOTNET_REF9_ROOT', 'JAVA_RUNTIME', 'JAVA_TARGET_JDK', 'JAVA_TARGET_8', 'LLVM_SDK', 'PROJECT_RUST_TOOLCHAIN', 'TEST_CARGO_TOOLCHAINS', 'TEST_CARGO_194'];
  const environment = Object.fromEntries(names.map(name => [`CODEGRAPH_${name}`, '${' + name + '}']));
  environment.CODEGRAPH_PYTHON = '${PYTHON}'; environment.CODEGRAPH_NATIVE_PACKS = '${ARTIFACTS}';
  return verificationCommands(environment, platform, 'all', { node: '${NODE}', root: '${REPOSITORY}', paths: path.posix });
}

/** 必须完整执行精确 argv、cwd 和 env 规范，子集或命令替换不能写完整验收 receipt。 */
export function recordVerifiedCommands(commands, environment, platform = process.platform) {
  if (!isDeepStrictEqual(commands, verificationCommands(environment, platform, 'all'))) throw new Error('Incomplete verification command identity');
  return verificationCommandIdentities(platform).map(command => ({ ...command, outcome: 'passed' }));
}
