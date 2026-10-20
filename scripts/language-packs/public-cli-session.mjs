import assert from 'node:assert/strict';
import os from 'node:os';
import fs from 'node:fs/promises';
import path from 'node:path';
import { spawn, execFile } from 'node:child_process';
import { promisify } from 'node:util';
import { StringDecoder } from 'node:string_decoder';
import { setTimeout as delay } from 'node:timers/promises';

const run = promisify(execFile);
const probeVariable = 'CODEGRAPH_TEST_DISCOVERY_PROBE';
export const discoveryMilestones = {
  rust: ['codegraph-rust-engine', 'workspace-metadata-loaded', /codegraph-rust-engine/],
  python: ['codegraph-python-engine', 'project-configuration-read', /codegraph-python-engine/],
  go: ['codegraph-go-engine', 'module-configurations-loaded', /codegraph-go-engine/],
  java: ['codegraph-java-engine', 'project-configurations-loaded', /dev\.codegraph\.java\.Main/],
  csharp: ['codegraph-csharp-engine', 'source-text-read', /CodeGraph\.CSharp\.Engine\.dll/],
  'typescript-javascript': ['codegraph-typescript-engine', 'repository-snapshot-read', /runtime[/\\]helper[/\\]main\.mjs/],
  'c-cpp': ['codegraph-clang-engine', 'compilation-database-read', /codegraph-clang-engine/],
};

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

/** 原子标记必须来自本次真实 helper 的已执行加载点，启动或 ready 本身不能满足。 */
export async function cancelLoadedProject(child, pack, request, observed, closed, probe, { timeout = 30_000 } = {}) {
  assert.ok(!Object.hasOwn(request, 'files'), 'Discovery cancellation requires omitted files');
  const [helperId, milestone, helper] = discoveryMilestones[pack];
  const deadline = Date.now() + timeout;
  let marker;
  while (true) {
    const snapshot = await processSnapshot();
    observeTree(snapshot, observed);
    assert.ok(!closed(), `${pack}: request completed before real load cancellation`);
    const stat = await fs.lstat(probe).catch(error => { if (error.code !== 'ENOENT') throw error; });
    if (stat) {
      assert.ok(stat.isFile() && !stat.isSymbolicLink() && stat.size > 0 && stat.size <= 4096, 'Invalid discovery observation file');
      marker = JSON.parse(await fs.readFile(probe, 'utf8'));
      assert.deepEqual(Object.keys(marker).sort(), ['helper_id', 'milestone', 'pid', 'request_id', 'schema_version']);
      assert.equal(marker.schema_version, 1);
      assert.equal(marker.helper_id, helperId);
      assert.equal(marker.request_id, 'analysis-1');
      assert.equal(marker.milestone, milestone);
      // 发布可能发生在前次快照之后；重新取样才能判断标记中的真实进程归属。
      const current = await processSnapshot();
      observeTree(current, observed);
      assert.ok(Number.isSafeInteger(marker.pid) && marker.pid > 0 && observed.has(marker.pid), 'Discovery observer is outside the owned process tree');
      assert.ok(current.some(item => item.pid === marker.pid && helper.test(item.command)), 'Discovery helper exited before cancellation');
      assert.ok(!closed(), `${pack}: request completed before real load cancellation`);
      break;
    }
    assert.ok(Date.now() < deadline, `${pack}: discovery/load milestone was not observed`);
    await delay(25);
  }
  console.log(`Public discovery milestone: ${JSON.stringify({ pack, ...marker })}`);
  // 生命周期管道保持打开；正常 EOF 在普通分析用例单独验证。
  child.stdin.write(`${JSON.stringify({ protocol: request.protocol, type: 'cancel', id: request.id })}\n`);
  return marker;
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
  const probeDirectory = cancel ? await fs.mkdtemp(path.join(os.tmpdir(), 'codegraph-discovery-probe-')) : undefined;
  const probe = probeDirectory && path.join(probeDirectory, 'reached.json');
  const environment = publicEnvironment();
  if (probe) environment[probeVariable] = probe;
  const child = spawn(executable, ['analyze'], { cwd: os.tmpdir(), env: environment, stdio: ['pipe', 'pipe', 'pipe'] });
  const chunks = [], observed = new Map();
  const decoder = new StringDecoder('utf8');
  let length = 0, stderr = '', failure, closed = false, completed = false, pending = '', cancelledAt, treeExited = false;
  const exit = new Promise((resolve, reject) => {
    child.once('error', reject);
    child.once('close', (code, signal) => { closed = true; resolve({ code, signal }); });
  });
  // 提前挂接拒绝处理，取消阶段的进程观察不会产生未处理 rejection。
  exit.catch(() => {});
  if (child.pid) observed.set(child.pid, executable);
  const timeout = cancel ? 45_000 : 120_000;
  const timer = setTimeout(() => { failure = new Error('Public CLI acceptance timed out'); child.kill('SIGKILL'); }, timeout);
  child.stdout.on('data', chunk => {
    length += chunk.length;
    if (length > 128 * 1024 * 1024) { failure = new Error('Public CLI output exceeds limit'); child.kill('SIGKILL'); return; }
    chunks.push(chunk);
    // 终态可能先于 close 到达；等待里不能忽略已完成但尚在退出的公开请求。
    if (cancel) {
      pending += decoder.write(chunk);
      let end;
      while ((end = pending.indexOf('\n')) !== -1) {
        const line = pending.slice(0, end); pending = pending.slice(end + 1);
        try { if (JSON.parse(line).type === 'completed') completed = true; }
        catch (error) { failure ??= error; child.kill('SIGKILL'); }
      }
    }
  });
  child.stderr.on('data', chunk => { stderr = (stderr + chunk).slice(-262144); });
  child.stdin.on('error', error => { failure ??= error; });
  try {
    if (cancel) {
      child.stdin.write(`${JSON.stringify(request)}\n`);
      await cancelLoadedProject(child, pack, request, observed, () => closed || completed, probe);
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
      console.log(`Public directory cancellation verified: ${JSON.stringify({ pack, outcome: result.outcome, exit_code: code, cancellation_ms: Date.now() - cancelledAt, observed_processes: observed.size })}`);
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
    if (probeDirectory) await fs.rm(probeDirectory, { recursive: true, force: true });
  }
}
