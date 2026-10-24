import assert from 'node:assert/strict';
import fs from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';
import { spawn, execFile } from 'node:child_process';
import { promisify } from 'node:util';
import { setTimeout as delay } from 'node:timers/promises';
import { after, test } from 'node:test';
import { directConfiguration } from './support.mjs';

const config = directConfiguration({ after });
const execute = promisify(execFile);

/** 调用方直接启动并持续排空真实 Node 的输出，无 ready 或控制会话。 */
function launch(t, args = config.args) {
  const child = spawn(config.executable, args, { cwd: os.tmpdir(), stdio: ['pipe', 'pipe', 'pipe'] });
  const state = { child, stdout: '', stderr: '', exited: false };
  child.stdout.setEncoding('utf8'); child.stderr.setEncoding('utf8');
  child.stdout.on('data', text => { state.stdout += text; });
  child.stderr.on('data', text => { state.stderr += text; });
  child.stdin.on('error', () => {});
  state.closed = new Promise((resolve, reject) => {
    child.once('error', reject);
    child.once('close', (code, signal) => { state.exited = true; resolve({ code, signal }); });
  });
  t.after(async () => { if (!state.exited) { child.kill('SIGKILL'); await state.closed; } });
  return state;
}

/** 有限观察实际进程状态，等待本身不施加取消。 */
async function until(predicate, milliseconds = 15000) {
  const deadline = performance.now() + milliseconds;
  while (!await predicate()) { assert.ok(performance.now() < deadline, 'runtime did not reach lifecycle boundary'); await delay(25); }
}

/** 普通请求在真实 Unicode 目录，主测试均由调用方关闭 stdin。 */
async function request(t, source = 'export function caller(){return target()} export function target(){return 1}') {
  const root = await fs.mkdtemp(path.join(os.tmpdir(), 'codegraph TS 生命周期-'));
  t.after(() => fs.rm(root, { recursive: true, force: true }));
  await fs.writeFile(path.join(root, 'main.ts'), source);
  return { protocol: 'codegraph.analyzer/1', type: 'analyze', id: 'lifecycle', repository_root: root,
    files: ['main.ts'], options: { include_tests: false, rust_dependency_scope: 'repository-only' } };
}

/** OS CPU 累计时间证明语言程序已在分析，不以 spawn 完成替代活动证据。 */
async function cpuMilliseconds(pid) {
  if (process.platform === 'win32') {
    const { stdout } = await execute('powershell.exe', ['-NoProfile', '-Command', `(Get-Process -Id ${pid}).TotalProcessorTime.TotalMilliseconds`]);
    return Number(stdout.trim());
  }
  const { stdout } = await execute('/bin/ps', ['-p', String(pid), '-o', 'time=']);
  return stdout.trim().split(':').reduce((total, part) => total * 60 + Number(part), 0) * 1000;
}

/** EOF 后继续进行实际大源码分析，后续只允许调用方终止。 */
async function running(t, discovery = false) {
  const input = await request(t, '');
  input.files = Array.from({ length: 40 }, (_, i) => `part${i}.ts`);
  await fs.writeFile(path.join(input.repository_root, 'tsconfig.json'), JSON.stringify({ compilerOptions: { target: 'ES2022', module: 'Preserve', moduleResolution: 'Bundler', types: [] }, files: input.files }));
  for (const [i, file] of input.files.entries()) await fs.writeFile(path.join(input.repository_root, file),
    Array.from({ length: 100 }, (_, j) => `export function f${i}_${j}(){return ${j ? `f${i}_${j - 1}()` : '1'}}`).join('\n'));
  if (discovery) delete input.files;
  const state = launch(t);
  state.child.stdin.end(JSON.stringify(input));
  let cpu = 0;
  await until(async () => { assert.equal(state.exited, false, state.stderr); cpu = await cpuMilliseconds(state.child.pid); return cpu >= 500; });
  assert.equal(state.stdout, '', 'fixture must still be analyzing after EOF');
  t.diagnostic(`direct Node CPU after EOF: ${Math.round(cpu)}ms`);
  return state;
}

test('valid JSON waits for EOF, then flushes exactly one real graph and exits', { timeout: 15000 }, async t => {
  const input = await request(t), state = launch(t);
  state.child.stdin.write(JSON.stringify(input));
  await delay(250);
  assert.equal(state.stdout, ''); assert.equal(state.exited, false);
  state.child.stdin.end();
  assert.deepEqual(await state.closed, { code: 0, signal: null }, state.stderr);
  const result = JSON.parse(state.stdout);
  assert.equal(result.outcome, 'ok');
  const target = result.graph.nodes.find(node => node.name === 'target');
  assert.ok(result.graph.nodes.find(node => node.name === 'caller').references.some(ref => ref.target.id === target.id));
});

test('a second JSON after a complete request is rejected after EOF', async t => {
  const state = launch(t);
  state.child.stdin.end(`${JSON.stringify(await request(t))}\n{}`);
  assert.equal((await state.closed).code, 1);
  assert.equal(JSON.parse(state.stdout).error.code, 'invalid_request');
});

for (const discovery of [false, true]) test(`caller terminates active direct ${discovery ? 'discovery' : 'analysis'} after normal EOF`, { timeout: 25000 }, async t => {
  const state = await running(t, discovery);
  state.child.kill('SIGKILL');
  const result = await state.closed;
  assert.ok(result.signal || result.code !== 0);
  assert.equal(state.stdout, '');
});

test('caller can terminate an input wait without a fabricated terminal', async t => {
  const state = launch(t);
  state.child.stdin.write('{');
  await delay(250);
  assert.equal(state.stdout, '');
  state.child.kill('SIGKILL');
  assert.notEqual((await state.closed).code, 0);
  assert.equal(state.stdout, '');
});

test('caller can terminate a real graph blocked by stdout backpressure', { timeout: 30000 }, async t => {
  const state = await running(t);
  state.child.stdout.pause();
  await until(() => state.child.stdout.readableLength > 0 || state.exited, 20000);
  assert.equal(state.exited, false);
  await delay(100);
  assert.equal(state.exited, false);
  state.child.kill('SIGKILL');
  state.child.stdout.resume();
  assert.notEqual((await state.closed).code, 0);
  assert.throws(() => JSON.parse(state.stdout));
});

test('real extraction failure is one handled error after EOF', async t => {
  const state = launch(t);
  state.child.stdin.end(JSON.stringify(await request(t, 'return;')));
  assert.deepEqual(await state.closed, { code: 1, signal: null });
  const result = JSON.parse(state.stdout);
  assert.equal(result.error.code, 'analysis_failed');
  assert.match(result.error.message, /源码无法解析/);
  assert.ok(!Object.hasOwn(result, 'graph'));
});

test('direct analysis works when the runtime denies Worker creation', async t => {
  const state = launch(t, ['--permission', '--allow-fs-read=*', ...config.args]);
  state.child.stdin.end(JSON.stringify(await request(t)));
  assert.equal((await state.closed).code, 0, state.stderr);
  assert.equal(JSON.parse(state.stdout).outcome, 'ok');
});

test('stdout write failure makes runtime exit nonzero', async t => {
  const state = launch(t);
  state.child.stdout.destroy();
  state.child.stdin.end(JSON.stringify(await request(t)));
  assert.equal((await state.closed).code, 1);
  assert.match(state.stderr, /EPIPE|EIO|EBADF/);
});
