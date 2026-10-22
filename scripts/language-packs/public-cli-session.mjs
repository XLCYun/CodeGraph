import assert from 'node:assert/strict';
import os from 'node:os';
import fs from 'node:fs/promises';
import path from 'node:path';
import { spawn, execFile } from 'node:child_process';
import { promisify } from 'node:util';
import { setTimeout as delay } from 'node:timers/promises';

const run = promisify(execFile);
const probeVariable = 'CODEGRAPH_TEST_DISCOVERY_PROBE';
export const discoveryMilestones = {
  rust: ['codegraph-rust', 'workspace-metadata-loaded', /codegraph-rust/],
  python: ['codegraph-python', 'project-configuration-read', /codegraph-python/],
  go: ['codegraph-go-engine', 'module-configurations-loaded', /codegraph-go/],
  java: ['codegraph-java', 'project-configurations-loaded', /dev\.codegraph\.java\.Main/],
  csharp: ['codegraph-csharp-engine', 'source-text-read', /codegraph-csharp/],
  'typescript-javascript': ['codegraph-typescript-javascript', 'repository-snapshot-read', /runtime[/\\]analyzer[/\\]main\.mjs/],
  'c-cpp': ['codegraph-clang-engine', 'compilation-database-read', /codegraph-c-cpp/],
};

/** 公开消费者只保留标准 SDK 环境；生产者私有配置不能帮助安装包通过验收。 */
export function publicEnvironment(environment = process.env) {
  return Object.fromEntries(Object.entries(environment).filter(([key]) => !key.toUpperCase().startsWith('CODEGRAPH_')));
}

/** 原生进程快照只用于核对本次调用方拥有的子树。 */
async function processSnapshot() {
  if (process.platform === 'win32') {
    const { stdout } = await run('powershell.exe', ['-NoProfile', '-NonInteractive', '-Command', 'Get-CimInstance Win32_Process | Select-Object ProcessId,ParentProcessId,CommandLine | ConvertTo-Json -Compress'], { timeout: 10_000, maxBuffer: 8 * 1024 * 1024 });
    return [].concat(JSON.parse(stdout)).map(item => ({ pid: item.ProcessId, parent: item.ParentProcessId, command: item.CommandLine ?? '' }));
  }
  const { stdout } = await run('ps', ['-axo', 'pid=,ppid=,command='], { timeout: 10_000, maxBuffer: 8 * 1024 * 1024 });
  return stdout.split('\n').flatMap(line => { const match = line.match(/^\s*(\d+)\s+(\d+)\s+(.*)$/); return match ? [{ pid: Number(match[1]), parent: Number(match[2]), command: match[3] }] : []; });
}

/** 保存后代的实际命令身份，父节点退出后也保留退出核对集合。 */
function observeTree(snapshot, observed) {
  let changed;
  do {
    changed = false;
    for (const item of snapshot) {
      if (observed.has(item.pid)) observed.set(item.pid, item.command);
      if (observed.has(item.parent) && !observed.has(item.pid)) { observed.set(item.pid, item.command); changed = true; }
    }
  } while (changed);
}

/** 调用方终止整树；macOS 使用独立进程组，Windows 使用实际 taskkill /T。 */
export async function terminateTree(child, { grouped = false } = {}) {
  if (!child.pid) return;
  if (process.platform === 'win32') await run('taskkill.exe', ['/F', '/T', '/PID', String(child.pid)], { windowsHide: true }).catch(error => { if (child.exitCode === null && child.signalCode === null) throw error; });
  else { try { process.kill(grouped ? -child.pid : child.pid, 'SIGKILL'); } catch (error) { if (error.code !== 'ESRCH') throw error; } }
}

/** 只在真实加载标记出现后触发调用方终止；stdin 已正常 EOF，不发送控制消息。 */
export async function cancelLoadedProject(child, pack, request, observed, closed, probe, { timeout = 30_000, grouped = false } = {}) {
  assert.ok(!Object.hasOwn(request, 'files'), 'Discovery cancellation requires omitted files');
  const [helperId, milestone, analyzer] = discoveryMilestones[pack];
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
      assert.equal(marker.request_id, request.id);
      assert.equal(marker.milestone, milestone);
      const current = await processSnapshot();
      observeTree(current, observed);
      assert.ok(Number.isSafeInteger(marker.pid) && marker.pid > 0 && observed.has(marker.pid), 'Discovery observer is outside the owned process tree');
      assert.ok(current.some(item => item.pid === marker.pid && analyzer.test(item.command)), 'Discovery analyzer exited before cancellation');
      assert.ok(!closed(), `${pack}: request completed before real load cancellation`);
      break;
    }
    assert.ok(Date.now() < deadline, `${pack}: discovery/load milestone was not observed`);
    await delay(25);
  }
  console.log(`Public discovery milestone: ${JSON.stringify({ pack, ...marker })}`);
  await terminateTree(child, { grouped });
  return marker;
}

/** 完成或终止后检查全部已观察成员，幸存进程不能被根退出掩盖。 */
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

/** 唯一完整最终 JSON、退出码与三个固定身份同时匹配才允许消费结果。 */
export function decodePublicResult(bytes, { code, signal }, pack, request, identity, expectedFailure) {
  const output = new TextDecoder('utf-8', { fatal: true }).decode(bytes);
  assert.equal(signal, null, `Analyzer interrupted: ${signal}`);
  assert.equal(code, expectedFailure ? 1 : 0, output);
  assert.ok(output.endsWith('\n'), 'Unterminated or missing CLI result');
  const result = JSON.parse(output);
  assert.equal(result.protocol, identity.protocol, 'Public final identity mismatch');
  assert.equal(result.engine_version, identity.engineVersion, 'Public final identity mismatch');
  assert.equal(result.pack_id, pack, 'Public final identity mismatch');
  assert.equal(result.type, 'completed');
  const earlyFailure = expectedFailure === 'invalid_request' || expectedFailure === 'incompatible_protocol';
  // 各语言可在其余请求字段之前验证 ID；只允许 null 或独立确认合法且完全相同的 ID。
  const validId = typeof request.id === 'string' && request.id.isWellFormed() && !request.id.includes('\0') && Buffer.byteLength(request.id, 'utf8') <= 1024 && /[^\p{White_Space}\uFEFF]/u.test(request.id);
  if (earlyFailure) assert.ok(result.id === null || validId && result.id === request.id, 'Rejected request returned an unaccepted or mismatched id');
  else assert.equal(result.id, request.id);
  assert.equal(result.outcome, expectedFailure ? 'error' : 'ok');
  assert.deepEqual(Object.keys(result).sort(), ['protocol', 'engine_version', 'pack_id', 'type', 'id', 'outcome', expectedFailure ? 'error' : 'graph'].sort());
  if (expectedFailure) {
    assert.ok((Array.isArray(expectedFailure) ? expectedFailure : [expectedFailure]).includes(result.error?.code), `Unexpected public error: ${JSON.stringify(result.error)}`);
    assert.equal(typeof result.error?.message, 'string');
    assert.equal(result.graph, undefined);
  }
  return result.graph;
}

/** 实际标准管道写完单个 JSON 后关闭输入，等待唯一结果与退出；取消由调用方整树终止。 */
export async function exchangeCli(executable, pack, request, identity, expectedFailure, { cancel = false, environment = process.env, args = ['analyze'], rawInput, timeoutMs, trace, cwd = os.tmpdir() } = {}) {
  const probeDirectory = cancel ? await fs.mkdtemp(path.join(os.tmpdir(), 'codegraph-discovery-probe-')) : undefined;
  const probe = probeDirectory && path.join(probeDirectory, 'reached.json');
  const env = publicEnvironment(environment);
  if (probe) env[probeVariable] = probe;
  const grouped = process.platform !== 'win32';
  const child = spawn(executable, args, { cwd, env, detached: grouped, windowsHide: true, stdio: ['pipe', 'pipe', 'pipe'] });
  const chunks = [], observed = new Map();
  let length = 0, stderr = '', failure, closed = false, exitStatus;
  const exit = new Promise((resolve, reject) => {
    child.once('error', reject);
    child.once('close', (code, signal) => { closed = true; resolve({ code, signal }); });
  });
  exit.catch(() => {});
  if (child.pid) observed.set(child.pid, executable);
  /** 保留首个传输失败并启动整树清理，close 屏障仍由 finally 等待。 */
  const stop = error => { failure ??= error; terminateTree(child, { grouped }).catch(cause => { failure ??= cause; }); };
  const timer = setTimeout(() => stop(new Error('Public CLI acceptance timed out')), timeoutMs ?? (cancel ? 45_000 : 120_000));
  child.stdout.on('data', chunk => { length += chunk.length; if (length > 128 * 1024 * 1024) stop(new Error('Public CLI output exceeds limit')); else chunks.push(chunk); });
  child.stderr.on('data', chunk => { stderr = (stderr + chunk).slice(-262144); });
  child.stdin.on('error', error => { if (!cancel) stop(error); });
  try {
    // 刻意不加尾换行，所有真实包都必须在 EOF 正常解码完整 JSON。
    child.stdin.end(rawInput ?? JSON.stringify(request));
    if (cancel) await cancelLoadedProject(child, pack, request, observed, () => closed || length > 0, probe, { grouped });
    const status = await exit;
    exitStatus = status;
    if (failure) throw failure;
    const output = Buffer.concat(chunks);
    if (cancel) {
      assert.ok(status.signal || status.code !== 0, 'Caller tree termination returned success');
      assert.equal(output.length, 0, 'Cancelled analysis emitted a final result');
      await assertTreeExited(observed);
      console.log(`Public caller tree termination verified: ${JSON.stringify({ pack, ...status, observed_processes: observed.size })}`);
    } else return decodePublicResult(output, status, pack, request, identity, expectedFailure);
  } catch (error) {
    error.message += stderr ? `; stderr: ${stderr}` : '';
    throw error;
  } finally {
    clearTimeout(timer);
    child.stdin.destroy();
    if (!closed) await terminateTree(child, { grouped });
    await exit.catch(() => {});
    if (cancel) await assertTreeExited(observed);
    if (trace) {
      await fs.mkdir(trace, { recursive: true });
      await fs.writeFile(path.join(trace, 'request.json'), rawInput ?? JSON.stringify(request), { flag: 'wx' });
      await fs.writeFile(path.join(trace, 'stdout.json'), Buffer.concat(chunks), { flag: 'wx' });
      await fs.writeFile(path.join(trace, 'stderr.txt'), stderr, { flag: 'wx' });
      await fs.writeFile(path.join(trace, 'status.json'), JSON.stringify({ ...exitStatus, expectedFailure: expectedFailure ?? null, callerTreeTermination: cancel, failure: failure?.message ?? null }), { flag: 'wx' });
    }
    if (probeDirectory) await fs.rm(probeDirectory, { recursive: true, force: true });
  }
}
