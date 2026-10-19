import fs from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';
import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import { nativeTarget, releaseIdentity, sha256 } from './distribution.mjs';
import { selectedNativePacks } from './native-migration.mjs';
import { verifyAndExtract } from './verify-distribution.mjs';

/** 每种语言都包含真实本地调用；测试项目不借用应用状态或源仓库。 */
export const fixtures = {
  rust: { files: ['src/lib.rs'], content: { 'Cargo.toml': '[package]\nname="codegraph_acceptance"\nversion="0.1.0"\nedition="2021"\n', 'src/lib.rs': 'pub fn target() -> i32 { 1 }\npub fn caller() -> i32 { target() }\n#[test]\nfn test_only() { caller(); }\n' } },
  'typescript-javascript': { files: ['main.ts', 'target.js'], content: { 'main.ts': "import {target} from './target.js'; export function caller() { target(); Promise.resolve(1); }\n", 'target.js': 'export function target() { return 1; }\n' } },
  python: { files: ['main.py'], content: { 'main.py': 'def target():\n    return 1\ndef caller():\n    return target()\n' } },
  java: { files: ['src/Sample.java'], prerequisite: 'narrant-java.json', content: { 'src/Sample.java': 'public class Sample { public static int target() { return 1; } public int caller() { return target(); } }\n', 'narrant-java.json': JSON.stringify({ schema: 'java-static-v1', release: '17', source_roots: [{ path: 'src', role: 'main' }] }) + '\n' } },
  go: { files: ['main.go'], content: { 'go.mod': 'module example.com/codegraph-acceptance\n\ngo 1.23\n', 'main.go': 'package acceptance\nfunc target() int { return 1 }\nfunc caller() int { return target() }\n' } },
  csharp: { files: ['Sample.cs'], content: { 'Sample.csproj': '<Project Sdk="Microsoft.NET.Sdk"><PropertyGroup><TargetFramework>net10.0</TargetFramework></PropertyGroup></Project>\n', 'Sample.cs': 'public static class Sample { public static int target() { return 1; } public static int caller() { return target(); } }\n' } },
  'c-cpp': { files: ['main.cpp'], prerequisite: 'compile_commands.json', content: { 'main.cpp': 'int target() { return 1; }\nint caller() { return target(); }\n' } },
};

/** 在真实临时仓库生成静态配置，源码与元数据共同构成只读快照。 */
export async function preparePublicProject(id, directory) {
  await fs.mkdir(directory, { recursive: true });
  const root = await fs.realpath(directory), fixture = structuredClone(fixtures[id]);
  if (id === 'c-cpp') {
    // 固定工厂只授予一个宿主目标 SDK；不猜部署版本，也不读取私有 helper 配置。
    fixture.content['compile_commands.json'] = JSON.stringify([{ directory: root, file: 'main.cpp', arguments: ['clang++', '-std=c++20', '-c', 'main.cpp'] }]) + '\n';
  }
  for (const [name, content] of Object.entries(fixture.content)) { await fs.mkdir(path.dirname(path.join(root, name)), { recursive: true }); await fs.writeFile(path.join(root, name), content); }
  return { ...fixture, root };
}

/** 所有输入文件和配置必须保留原字节，分析也不能在仓库添加文件。 */
export async function assertPublicProjectUnchanged(project) {
  for (const [name, content] of Object.entries(project.content)) assert.equal(await fs.readFile(path.join(project.root, name), 'utf8'), content, `Analysis changed ${name}`);
  assert.deepEqual((await fs.readdir(project.root, { recursive: true, withFileTypes: true })).filter(item => item.isFile()).map(item => path.relative(project.root, path.join(item.parentPath, item.name)).replaceAll('\\', '/')).sort(), Object.keys(project.content).sort(), 'Analysis wrote files into the project');
}

/** 完整校验公共 NDJSON、完成身份和退出；绝不把进程启动或 EOF 视为成功。 */
export async function exchangeCli(executable, pack, root, request, identity, expectedFailure) {
  const child = spawn(executable, ['analyze', '--pack', pack, '--pack-root', root], { env: process.env, stdio: ['pipe', 'pipe', 'pipe'] });
  const chunks = [];
  let length = 0, stderr = '', failure;
  const timer = setTimeout(() => { failure = new Error('Public CLI acceptance timed out'); child.kill('SIGKILL'); }, 120_000);
  child.stdout.on('data', chunk => { length += chunk.length; if (length > 128 * 1024 * 1024) { failure = new Error('Public CLI output exceeds limit'); child.kill('SIGKILL'); } else chunks.push(chunk); });
  child.stderr.on('data', chunk => { stderr = (stderr + chunk).slice(-262144); });
  child.stdin.on('error', error => { failure ??= error; });
  try {
    const exit = new Promise((resolve, reject) => { child.once('error', reject); child.once('close', (code, signal) => resolve({ code, signal })); });
    child.stdin.end(`${JSON.stringify(request)}\n`);
    const { code, signal } = await exit;
    if (failure) throw failure;
    assert.equal(signal, null, stderr);
    assert.equal(code, expectedFailure ? 1 : 0, stderr + Buffer.concat(chunks).toString());
    const output = Buffer.concat(chunks).toString('utf8');
    assert.ok(output.endsWith('\n'), 'Unterminated CLI output');
    const messages = output.slice(0, -1).split('\n').map(line => JSON.parse(line));
    assert.equal(messages.length, 2);
    assert.deepEqual(messages[0], { protocol: identity.protocol, type: 'ready', engine_version: identity.engineVersion, pack_id: pack });
    assert.equal(messages[1].protocol, identity.protocol);
    assert.equal(messages[1].type, 'completed');
    assert.equal(messages[1].id, request.id);
    assert.equal(messages[1].outcome, expectedFailure ? 'error' : 'ok');
    if (expectedFailure) {
      assert.equal(messages[1].error?.code, expectedFailure);
      assert.equal(typeof messages[1].error?.message, 'string');
      assert.equal(messages[1].graph, undefined);
      return;
    }
    return messages[1].graph;
  } finally { clearTimeout(timer); }
}

/** 图必须保留调用、源文本、完整 containment 和引用闭包。 */
export function assertGraph(graph) {
  assert.deepEqual(Object.keys(graph).sort(), ['external_symbols', 'nodes', 'root_contains']);
  const nodes = new Map(graph.nodes.map(node => [node.id, node]));
  assert.equal(nodes.size, graph.nodes.length);
  const caller = graph.nodes.find(node => node.name === 'caller');
  const target = graph.nodes.find(node => node.name === 'target');
  assert.ok(caller && target, 'Missing real callable graph');
  assert.ok(caller.source?.includes('target'), 'Original callable source lost');
  assert.ok(caller.references.some(ref => ref.semantic === 'code.calls' && ref.target.kind === 'node' && ref.target.id === target.id), 'Local call is not resolved');
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

/** 对实际解包后的 CLI 与语言包做端到端分析，保留原文件内容与文件集合。 */
export async function verifyPublicCli(artifacts, language = 'all') {
  const target = nativeTarget(), identity = await releaseIdentity();
  const scratch = await fs.mkdtemp(path.join(os.tmpdir(), 'codegraph-public-cli-'));
  try {
    const cliRoot = path.join(scratch, 'cli');
    await verifyAndExtract(path.join(artifacts, 'codegraph', identity.version, target), cliRoot, { id: 'codegraph', target, version: identity.version });
    const executableName = `codegraph${process.platform === 'win32' ? '.exe' : ''}`;
    const standalone = path.join(cliRoot, 'bin', executableName);
    for (const id of selectedNativePacks(language)) {
      const pack = path.join(scratch, id);
      await verifyAndExtract(path.join(artifacts, id, identity.version, target), pack, { id, target, version: identity.version });
      assert.equal(sha256(await fs.readFile(standalone)), sha256(await fs.readFile(path.join(pack, 'bin', executableName))), 'Pack CLI must equal standalone CLI');
      assert.deepEqual(JSON.parse(await fs.readFile(path.join(pack, 'control/codegraph.json'))), { schema_version: 1, pack_id: id, pack_version: identity.version, engine_version: identity.engineVersion, protocol: identity.protocol });
      const fixture = await preparePublicProject(id, path.join(scratch, `${id}-source`)), source = fixture.root;
      const scopes = id === 'rust' ? ['repository-only', 'standard-library', 'all-dependencies'] : ['repository-only'];
      for (const scope of scopes) for (const includeTests of [false, true]) {
        const request = { protocol: identity.protocol, type: 'analyze', id: `${id}-${scope}-${includeTests}`, repository_root: await fs.realpath(source), files: fixture.files, options: { include_tests: includeTests, rust_dependency_scope: scope } };
        // 交替使用独立 CLI 与包内 CLI，两种手工消费路径都必须真正运行。
        const graph = await exchangeCli(includeTests ? standalone : path.join(pack, 'bin', executableName), id, pack, request, identity);
        assertGraph(graph);
        if (id === 'rust') assert.equal(graph.nodes.some(node => node.name === 'test_only'), includeTests);
      }
      const empty = await exchangeCli(standalone, id, pack, { protocol: identity.protocol, type: 'analyze', id: `${id}-empty`, repository_root: await fs.realpath(source), files: [], options: { include_tests: false, rust_dependency_scope: 'repository-only' } }, identity);
      assert.deepEqual(empty, { root_contains: [], nodes: [], external_symbols: [] });
      await assertPublicProjectUnchanged(fixture);
      if (fixture.prerequisite) {
        // 先要求有效项目成功，随后缺配置只能返回明确分析失败，启动错误/超时不能冒充通过。
        const metadata = path.join(source, fixture.prerequisite);
        await fs.rm(metadata);
        try {
          await exchangeCli(standalone, id, pack, { protocol: identity.protocol, type: 'analyze', id: `${id}-missing-metadata`, repository_root: source, files: fixture.files, options: { include_tests: false, rust_dependency_scope: 'repository-only' } }, identity, 'analysis_failed');
          const content = { ...fixture.content }; delete content[fixture.prerequisite];
          await assertPublicProjectUnchanged({ ...fixture, content });
        } finally { await fs.writeFile(metadata, fixture.content[fixture.prerequisite]); }
        await assertPublicProjectUnchanged(fixture);
      }
      console.log(`Public CLI verified: ${id} (${scopes.join(', ')}), read-only, include_tests, source, call graph, empty selection${fixture.prerequisite ? ', missing metadata rejected' : ''}`);
    }
  } finally { await fs.rm(scratch, { recursive: true, force: true }); }
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) await verifyPublicCli(path.resolve(process.argv[2] ?? 'artifacts'), process.argv[3] ?? 'all');
