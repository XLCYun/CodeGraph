import assert from 'node:assert/strict';
import os from 'node:os';
import { spawn, execFile } from 'node:child_process';
import { promisify } from 'node:util';
import { setTimeout as delay } from 'node:timers/promises';

const run = promisify(execFile);

/** 公开消费者只保留标准 SDK 环境；生产者私有测试配置不能帮助安装包通过验收。 */
export function publicEnvironment(environment = process.env) {
  return Object.fromEntries(Object.entries(environment).filter(([key]) => !key.startsWith('CODEGRAPH_')));
}

/** 原生系统进程快照仅用于验收拥有的子树，不读取或终止其他任务。 */
async function processSnapshot() {
  if (process.platform === 'win32') {
    const { stdout } = await run('powershell.exe', ['-NoProfile', '-NonInteractive', '-Command', 'Get-CimInstance Win32_Process | Select-Object ProcessId,ParentProcessId,CommandLine | ConvertTo-Json -Compress'], { timeout: 10_000, maxBuffer: 8 * 1024 * 1024 });
    return [].concat(JSON.parse(stdout)).map(item => ({ pid: item.ProcessId, parent: item.ParentProcessId, command: item.CommandLine ?? '' }));
  }
  const { stdout } = await run('ps', ['-axo', 'pid=,ppid=,command='], { timeout: 10_000, maxBuffer: 8 * 1024 * 1024 });
  return stdout.split('\n').flatMap(line => { const match = line.match(/^\s*(\d+)\s+(\d+)\s+(.*)$/); return match ? [{ pid: Number(match[1]), parent: Number(match[2]), command: match[3] }] : []; });
}

/** 收集实际观察到的所有后代；已脱离父节点的进程仍留在退出核对集合。 */
function observeTree(snapshot, observed) {
  let changed;
  do {
    changed = false;
    for (const item of snapshot) if (observed.has(item.parent) && !observed.has(item.pid)) { observed.set(item.pid, item.command); changed = true; }
  } while (changed);
}

/** 安装包的真实引擎进入请求后才发送取消，并断言公开 owner、worker 与 helper 全部退出。 */
async function cancelLoadedProject(child, pack, request, observed, closed) {
  const helper = { rust: /codegraph-rust-engine/, python: /codegraph-python-engine/, go: /codegraph-go-engine/, java: /dev\.codegraph\.java\.Main/, csharp: /CodeGraph\.CSharp\.Engine\.dll/, 'typescript-javascript': /runtime[/\\]helper[/\\]main\.mjs/, 'c-cpp': /codegraph-clang-engine/ }[pack];
  const deadline = Date.now() + 30_000;
  while (true) {
    observeTree(await processSnapshot(), observed);
    assert.ok(!closed(), `${pack}: request completed before real load cancellation`);
    if ([...observed.values()].some(command => helper.test(command))) break;
    assert.ok(Date.now() < deadline, `${pack}: actual engine was not observed`);
    await delay(25);
  }
  // 生命周期管道保持打开；正常 EOF 在普通分析用例单独验证。
  child.stdin.write(`${JSON.stringify({ protocol: request.protocol, type: 'cancel', id: request.id })}\n`);
}

/** 终态和进程关闭之后仍检查已观察子树；幸存进程不能被成功响应掩盖。 */
async function assertTreeExited(observed) {
  const deadline = Date.now() + 10_000;
  while (true) {
    const snapshot = await processSnapshot();
    observeTree(snapshot, observed);
    const live = snapshot.filter(item => observed.get(item.pid) === item.command);
    if (!live.length) return;
    assert.ok(Date.now() < deadline, `Public CLI descendants survived: ${JSON.stringify(live)}`);
    await delay(50);
  }
}

/** 协议只承认唯一完整终态；超时、启动失败、普通 EOF 都不是分析或取消成功。 */
export async function exchangeCli(executable, pack, request, identity, expectedFailure, { cancel = false } = {}) {
  const child = spawn(executable, ['analyze'], { cwd: os.tmpdir(), env: publicEnvironment(), stdio: ['pipe', 'pipe', 'pipe'] });
  const chunks = [], observed = new Map();
  let length = 0, stderr = '', failure, closed = false, cancelledAt, treeExited = false;
  const exit = new Promise((resolve, reject) => {
    child.once('error', reject);
    child.once('close', (code, signal) => { closed = true; resolve({ code, signal }); });
  });
  // 提前挂接拒绝处理，取消阶段的进程观察不会产生未处理 rejection。
  exit.catch(() => {});
  if (child.pid) observed.set(child.pid, executable);
  const timeout = cancel ? 45_000 : 120_000;
  const timer = setTimeout(() => { failure = new Error('Public CLI acceptance timed out'); child.kill('SIGKILL'); }, timeout);
  child.stdout.on('data', chunk => { length += chunk.length; if (length > 128 * 1024 * 1024) { failure = new Error('Public CLI output exceeds limit'); child.kill('SIGKILL'); } else chunks.push(chunk); });
  child.stderr.on('data', chunk => { stderr = (stderr + chunk).slice(-262144); });
  child.stdin.on('error', error => { failure ??= error; });
  try {
    if (cancel) {
      child.stdin.write(`${JSON.stringify(request)}\n`);
      await cancelLoadedProject(child, pack, request, observed, () => closed);
      cancelledAt = Date.now();
    } else child.stdin.end(`${JSON.stringify(request)}\n`);
    const { code, signal } = await exit;
    if (failure) throw failure;
    assert.equal(signal, null, stderr);
    assert.equal(code, cancel ? 130 : expectedFailure ? 1 : 0, stderr + Buffer.concat(chunks).toString());
    const output = Buffer.concat(chunks).toString('utf8');
    assert.ok(output.endsWith('\n'), 'Unterminated CLI output');
    const messages = output.slice(0, -1).split('\n').map(line => JSON.parse(line));
    assert.equal(messages.length, 2);
    assert.deepEqual(messages[0], { protocol: identity.protocol, type: 'ready', engine_version: identity.engineVersion, pack_id: pack });
    const result = messages[1];
    assert.equal(result.protocol, identity.protocol);
    assert.equal(result.type, 'completed');
    assert.equal(result.id, expectedFailure === 'invalid_request' ? null : request.id);
    assert.equal(result.outcome, cancel ? 'cancelled' : expectedFailure ? 'error' : 'ok');
    if (cancel) {
      assert.ok(Date.now() - cancelledAt < 15_000, 'Public cancellation exceeded bounded cleanup deadline');
      assert.deepEqual(Object.keys(result).sort(), ['id', 'outcome', 'protocol', 'type']);
      await assertTreeExited(observed);
      treeExited = true;
    } else if (expectedFailure) {
      assert.equal(result.error?.code, expectedFailure);
      assert.equal(typeof result.error?.message, 'string');
      assert.equal(result.graph, undefined);
    } else return result.graph;
  } finally {
    clearTimeout(timer);
    child.stdin.destroy();
    if (!closed) child.kill('SIGKILL');
    // 失败路径也只回收本次观察的子树；原失败仍传播给完整验收。
    if (cancel && !treeExited) {
      await exit.catch(() => {});
      try { await assertTreeExited(observed); } catch {
        for (const item of await processSnapshot()) if (observed.get(item.pid) === item.command) {
          if (process.platform === 'win32') await run('taskkill.exe', ['/F', '/T', '/PID', String(item.pid)]).catch(() => {});
          else { try { process.kill(item.pid, 'SIGKILL'); } catch {} }
        }
      }
    }
  }
}
