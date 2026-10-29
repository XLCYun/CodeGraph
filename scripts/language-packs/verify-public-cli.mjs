import fs from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';
import assert from 'node:assert/strict';
import { fileURLToPath } from 'node:url';
import { spawnSync } from 'node:child_process';
import { publicEnvironment } from './public-cli-session.mjs';
import { nativeTarget, releaseIdentity } from './distribution.mjs';
import { selectedNativePacks } from './native-migration.mjs';
import { verifyAndExtract } from './verify-distribution.mjs';
import { verifyNativePackPolicy } from './verify-native-pack-policy.mjs';
import { verifyAnalyzerDistribution } from './verify-analyzer-package.mjs';
import { preparePublicProject, writePublicProject, prepareCancellationProject, assertPublicProjectUnchanged } from './public-cli-projects.mjs';
import { exchangeCli as exchangePublic } from './public-cli-session.mjs';
import { writeLocalCandidates } from './local-candidates.mjs';
export { fixtures, preparePublicProject, assertPublicProjectUnchanged } from './public-cli-projects.mjs';
export { exchangeCli } from './public-cli-session.mjs';

/** 图必须保留调用、源文本、完整 containment 和引用闭包。 */
export function assertGraph(graph, { resolvedCall = true } = {}) {
  assert.deepEqual(Object.keys(graph).sort(), ['external_symbols', 'nodes', 'root_contains']);
  const nodes = new Map(graph.nodes.map(node => [node.id, node]));
  assert.equal(nodes.size, graph.nodes.length);
  const caller = graph.nodes.find(node => node.name === 'caller');
  const target = graph.nodes.find(node => node.name === 'target');
  assert.ok(caller && target, 'Missing real callable graph');
  assert.ok(caller.source?.includes('target'), 'Original callable source lost');
  if (resolvedCall) assert.ok(caller.references.some(ref => ref.semantic === 'code.calls' && ref.target.kind === 'node' && ref.target.id === target.id), 'Local call is not resolved');
  else assert.ok(caller.references.some(ref => ref.target.kind === 'unresolved' && ref.target.name.includes('target')), 'Missing context must retain an unresolved target');
  const seen = new Set();
  /** 递归验证每个节点只归一个父节点，且所有节点可达。 */
  function visit(id) {
    assert.ok(nodes.has(id) && !seen.has(id), `Invalid containment: ${id}`);
    seen.add(id);
    const node = nodes.get(id);
    for (const ref of node.references) {
      if (ref.target.kind === 'node') assert.ok(nodes.has(ref.target.id));
      if (ref.target.kind === 'external') assert.equal(graph.external_symbols.filter(item => item.symbol === ref.target.symbol).length, 1);
    }
    for (const child of node.contains) visit(child);
  }
  for (const id of graph.root_contains) visit(id);
  assert.equal(seen.size, nodes.size);
}

/** 各语言独立串行验收；完整保留失败，再统一阻止成功 receipt。 */
export async function runPublicPackAcceptances(ids, verify) {
  const failures = [];
  for (const id of ids) {
    try {
      await verify(id);
    } catch (cause) {
      const failure = new Error(`Public CLI acceptance failed: ${id}`, { cause });
      failures.push(failure);
      console.error(failure);
    }
  }
  if (failures.length) throw new AggregateError(failures, `${failures.length} public pack acceptance(s) failed`);
}

/** 所有公开请求均只带根、可选选集与公共选项；不暴露生产者配置。 */
function request(identity, id, root, label, files, includeTests = false, scope = 'repository-only') {
  return { protocol: identity.protocol, type: 'analyze', id: `${id}-${label}`, repository_root: root, ...(files === undefined ? {} : { files }), options: { include_tests: includeTests, rust_dependency_scope: scope } };
}

/** 显式选集外仍可解析目标，但不能把完整目标节点或其它源码加入结果。 */
function assertExactGraph(graph, testRole) {
  const caller = graph.nodes.find(node => node.name === 'caller');
  assert.ok(caller?.source?.includes('target'), 'Selected caller/source lost');
  assert.ok(!graph.nodes.some(node => ['target', 'excluded_member', ...(testRole === false ? [] : ['test_only'])].includes(node.name)), 'Selection expanded to an unselected callable');
  const calls = caller.references.filter(ref => ref.semantic === 'code.calls' && ref.target.kind === 'external');
  assert.ok(calls.some(ref => graph.external_symbols.some(symbol => symbol.symbol === ref.target.symbol && symbol.name === 'target' && symbol.scope === 'repository')), 'Outside target is not a repository external symbol');
}

/** 对实际安装字节检查帮助、版本和旧入口拒绝；不借构建环境或运行时命令替身。 */
async function verifyFixedCli(executable, id, identity, trace) {
  const records = [];
  for (const args of [['--version'], ['--help'], ['analyze', '--help'], ['__worker'], ['--pack', id], ['analyze', 'analyze']]) {
    const result = spawnSync(executable, args, { cwd: os.tmpdir(), env: publicEnvironment(), encoding: 'utf8', timeout: 30_000, maxBuffer: 256 * 1024, windowsHide: true });
    assert.equal(result.signal, null, result.stderr);
    const usageError = args.includes('__worker') || args.includes('--pack') || args.length === 2 && args[1] === 'analyze';
    assert.equal(result.status, usageError ? 2 : 0, result.stderr);
    if (args[0] === '--version') assert.equal(result.stdout.trim(), `codegraph-${id} ${identity.version}`);
    if (usageError) assert.equal(result.stdout, '');
    records.push({ args, status: result.status, stdout: result.stdout, stderr: result.stderr });
  }
  await fs.mkdir(path.dirname(trace), { recursive: true });
  await fs.writeFile(trace, `${JSON.stringify(records, null, 2)}\n`, { flag: 'wx' });
}

/** 对实际归档的唯一固定入口做配置发现、精确边界、失败、取消和只读验收。 */
export async function verifyPublicCli(artifacts, language = 'all') {
  const target = nativeTarget(), identity = await releaseIdentity();
  const scratch = await fs.mkdtemp(path.join(os.tmpdir(), 'codegraph-public-cli-'));
  const emptyGraph = { root_contains: [], nodes: [], external_symbols: [] };
  try {
    await runPublicPackAcceptances(selectedNativePacks(language), async id => {
      let sequence = 0;
      const exchangeCli = (executable, packId, input, version, failure, options = {}) => exchangePublic(executable, packId, input, version, failure, { ...options, trace: path.join(artifacts, 'public-acceptance', id, String(++sequence).padStart(3, '0')) });
      const pack = path.join(scratch, id), directory = path.join(artifacts, id, identity.version, target);
      await verifyAnalyzerDistribution(directory, { id, target, version: identity.version, sourceCommit: identity.sourceCommit, sourceDirty: identity.sourceDirty });
      await verifyAndExtract(directory, pack, { id, target, version: identity.version });
      await verifyNativePackPolicy(id, pack);
      const executable = path.join(pack, 'bin', `codegraph-${id}${process.platform === 'win32' ? '.exe' : ''}`);
      await verifyFixedCli(executable, id, identity, path.join(artifacts, 'public-acceptance', id, 'cli.json'));
      const fixture = await preparePublicProject(id, path.join(scratch, `${id}-source`));
      const vectors = JSON.parse(await fs.readFile(new URL('../../analyzers/common/src/tests/public-conformance.json', import.meta.url), 'utf8'));
      for (const vector of vectors) {
        const base = { ...request(identity, id, fixture.root, 'conformance', []), id: '公共-é-😀', ...vector.patch };
        const graph = await exchangeCli(executable, id, base, identity, vector.error_code, { rawInput: JSON.stringify(base) + vector.suffix });
        if (vector.outcome === 'ok') assert.deepEqual(graph, emptyGraph);
      }
      const scopes = id === 'rust' ? ['repository-only', 'standard-library', 'all-dependencies'] : ['repository-only'];
      for (const scope of scopes) for (const includeTests of [false, true]) for (const discovery of [false, true]) {
        const graph = await exchangeCli(executable, id, request(identity, id, fixture.root, `${scope}-${includeTests}-${discovery}`, discovery ? undefined : fixture.files, includeTests, scope), identity);
        assertGraph(graph);
        assert.equal(graph.nodes.some(node => node.name === 'test_only'), fixture.testRole === false || includeTests, `${id}: incorrect native test role`);
        assert.ok(!graph.nodes.some(node => node.name === 'excluded_member'), 'Configuration excluded source became a full node');
      }
      const exact = await exchangeCli(executable, id, request(identity, id, fixture.root, 'exact', fixture.selected), identity);
      // Rust/C++ 的选中文件含本地测试声明，单独验证图外目标，测试开关按各自政策处理。
      assertExactGraph(exact, fixture.testRole);
      assert.deepEqual(await exchangeCli(executable, id, request(identity, id, fixture.root, 'empty', []), identity), emptyGraph);
      await exchangeCli(executable, id, request(identity, id, fixture.root, 'null', null), identity, 'invalid_request');
      await assertPublicProjectUnchanged(fixture);

      const configuredEmpty = await writePublicProject(path.join(scratch, `${id}-configured-empty`), fixture.empty, id === 'c-cpp');
      const emptyResult = await exchangeCli(executable, id, request(identity, id, configuredEmpty.root, 'configured-empty'), identity);
      if (id === 'c-cpp') assert.ok(!emptyResult.nodes.some(node => node.category === 'callable'), 'Empty TU gained a callable');
      else assert.deepEqual(emptyResult, emptyGraph);
      await assertPublicProjectUnchanged(configuredEmpty);

      // C/C++ 缺少编译上下文继续源码图；其他语言按自身必需项目模型判断。
      const missingContent = fixture.missing ?? (fixture.implicit ? fixture.configless : Object.fromEntries(Object.entries(fixture.content).filter(([name]) => name !== fixture.prerequisite)));
      const missing = await writePublicProject(path.join(scratch, `${id}-missing`), missingContent);
      const sourceFirst = id === 'c-cpp';
      const missingError = !sourceFirst && (fixture.missing || !fixture.implicit) ? ['analysis_failed', 'invalid_input'] : undefined;
      const missingResult = await exchangeCli(executable, id, request(identity, id, missing.root, 'missing'), identity, missingError);
      if (!missingError) assertGraph(missingResult, { resolvedCall: !sourceFirst });
      // Java 保留必需配置负例；C/C++ 的显式选集也必须在缺 CDB 时交付源码。
      if (id === 'java') await exchangeCli(executable, id, request(identity, id, missing.root, 'missing-explicit', fixture.files), identity, ['analysis_failed', 'invalid_input']);
      if (sourceFirst) assertGraph(await exchangeCli(executable, id, request(identity, id, missing.root, 'missing-explicit', fixture.files), identity), { resolvedCall: false });
      assert.deepEqual(await exchangeCli(executable, id, request(identity, id, missing.root, 'missing-empty', []), identity), emptyGraph);
      await assertPublicProjectUnchanged(missing);
      if (fixture.implicit && fixture.missing) {
        const implicit = await writePublicProject(path.join(scratch, `${id}-implicit`), fixture.configless);
        assertGraph(await exchangeCli(executable, id, request(identity, id, implicit.root, 'implicit'), identity));
        await assertPublicProjectUnchanged(implicit);
      }

      const malformed = await preparePublicProject(id, path.join(scratch, `${id}-malformed`));
      malformed.content[fixture.prerequisite] = id === 'c-cpp' ? '[]' : '! invalid configuration [';
      await fs.writeFile(path.join(malformed.root, fixture.prerequisite), malformed.content[fixture.prerequisite]);
      const malformedResult = await exchangeCli(executable, id, request(identity, id, malformed.root, 'malformed'), identity, sourceFirst ? undefined : ['analysis_failed', 'invalid_input']);
      if (sourceFirst) assertGraph(malformedResult, { resolvedCall: false });
      assert.deepEqual(await exchangeCli(executable, id, request(identity, id, malformed.root, 'malformed-empty', []), identity), emptyGraph);
      await assertPublicProjectUnchanged(malformed);

      const cancellation = await prepareCancellationProject(id, path.join(scratch, `${id}-cancellation`));
      await exchangeCli(executable, id, request(identity, id, cancellation.root, 'directory-cancel'), identity, undefined, { cancel: true });
      await assertPublicProjectUnchanged(cancellation);
      console.log(`Public CLI verified: ${id}; configured discovery, explicit/external boundary, native test roles, []/configured empty, missing/invalid configuration, read-only, normal EOF, real caller tree termination and descendant exit; shared conformance=21; no final newline; scopes=${scopes.join(',')}`);
      // 每语言检查完成后释放临时解包；最终候选及 smoke-installed 保留供消费者交接。
      await fs.rm(pack, { recursive: true, force: true });
    });
    if (language === 'all') await writeLocalCandidates(artifacts);
  } finally { await fs.rm(scratch, { recursive: true, force: true }); }
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) await verifyPublicCli(path.resolve(process.argv[2] ?? 'artifacts'), process.argv[3] ?? 'all');
