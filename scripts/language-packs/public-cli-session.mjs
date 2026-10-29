import assert from 'node:assert/strict';
import os from 'node:os';
import fs from 'node:fs/promises';
import path from 'node:path';
import { execFile } from 'node:child_process';
import { promisify } from 'node:util';
import { setTimeout as delay } from 'node:timers/promises';
import { spawnOwnedCli } from './public-cli-owner.mjs';

const run = promisify(execFile);
const probeVariable = 'CODEGRAPH_TEST_DISCOVERY_PROBE';
export const discoveryMilestones = {
  rust: ['codegraph-rust', 'workspace-metadata-loaded', /codegraph-rust/],
  python: ['codegraph-python', 'project-configuration-read', /codegraph-python/],
  go: ['codegraph-go-engine', 'module-configurations-loaded', /codegraph-go/],
  java: ['codegraph-java', 'project-configurations-loaded', /dev\.codegraph\.java\.Main/],
  csharp: ['codegraph-csharp-engine', 'source-text-read', /codegraph-csharp/],
  'typescript-javascript': ['codegraph-typescript-javascript', 'repository-snapshot-read', /runtime[/\\]analyzer[/\\]main\.mjs/],
  'c-cpp': ['codegraph-clang-engine', 'source-selection-prepared', /codegraph-c-cpp/],
};

/** 公开消费者只保留标准 SDK 环境；生产者私有配置不能帮助安装包通过验收。 */
export function publicEnvironment(environment = process.env) {
  return Object.fromEntries(Object.entries(environment).filter(([key]) => !key.toUpperCase().startsWith('CODEGRAPH_')));
}

/** 原生进程快照只用于核对本次调用方拥有的子树。 */
async function processSnapshot() {
  if (process.platform === 'win32') {
    const command = 'Get-CimInstance Win32_Process | Select-Object ProcessId,ParentProcessId,CommandLine,@{Name="Started";Expression={if ($_.CreationDate) {$_.CreationDate.ToUniversalTime().Ticks.ToString()}}} | ConvertTo-Json -Compress';
    const { stdout } = await run('powershell.exe', ['-NoProfile', '-NonInteractive', '-Command', command], { timeout: 10_000, maxBuffer: 8 * 1024 * 1024 });
    return [].concat(JSON.parse(stdout)).map(item => ({ pid: item.ProcessId, parent: item.ParentProcessId, command: item.CommandLine ?? '', started: item.Started ?? null }));
  }
  const { stdout } = await run('ps', ['-axo', 'pid=,ppid=,command='], { timeout: 10_000, maxBuffer: 8 * 1024 * 1024 });
  return stdout.split('\n').flatMap(line => { const match = line.match(/^\s*(\d+)\s+(\d+)\s+(.*)$/); return match ? [{ pid: Number(match[1]), parent: Number(match[2]), command: match[3] }] : []; });
}

/** Windows 以出生时间绑定同一个进程；Unix 保留原有命令观察方式。 */
function sameBirth(recorded, current) {
  return recorded && current && typeof recorded === 'object' && recorded.started === current.started;
}

/** 已知自有进程必须有可比较的 Windows 出生身份，系统中的无关受保护进程不参与判断。 */
function requireBirth(item) {
  assert.ok(item.started === undefined || typeof item.started === 'string' && /^\d+$/.test(item.started), `Owned process creation time unavailable: ${item.pid}`);
}

/** 仅沿当前仍匹配的父进程扩展子树，已记录成员在父进程退出后仍保留。 */
export function observeTree(snapshot, observed) {
  const current = new Map(snapshot.map(item => [item.pid, item]));
  for (const [pid, recorded] of observed) {
    const item = current.get(pid);
    if (item && (recorded === undefined || typeof recorded === 'string' || sameBirth(recorded, item))) {
      requireBirth(item);
      observed.set(pid, item);
    }
  }
  let changed;
  do {
    changed = false;
    for (const item of snapshot) {
      if (sameBirth(observed.get(item.pid), item)) continue;
      const parent = current.get(item.parent);
      if (!sameBirth(observed.get(item.parent), parent)) continue;
      requireBirth(item);
      if (item.started !== undefined && BigInt(item.started) < BigInt(parent.started)) continue;
      observed.set(item.pid, item);
      changed = true;
    }
  } while (changed);
}

/** PID 被复用的新进程不能冒充原来的成员，真实存活成员仍须通过退出断言。 */
export function liveObservedProcesses(snapshot, observed) {
  return snapshot.filter(item => sameBirth(observed.get(item.pid), item) && observed.get(item.pid).command === item.command);
}

/** 只在真实加载标记出现后触发调用方终止；stdin 已正常 EOF，不发送控制消息。 */
export async function cancelLoadedProject(child, pack, request, observed, closed, probe, { timeout = 30_000, terminate } = {}) {
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
      assert.ok(liveObservedProcesses(current, observed).some(item => item.pid === marker.pid && analyzer.test(item.command)), 'Discovery analyzer exited before cancellation');
      assert.ok(!closed(), `${pack}: request completed before real load cancellation`);
      break;
    }
    assert.ok(Date.now() < deadline, `${pack}: discovery/load milestone was not observed`);
    await delay(25);
  }
  console.log(`Public discovery milestone: ${JSON.stringify({ pack, ...marker })}`);
  console.log(`Public discovery process identities: ${JSON.stringify([...observed.values()].filter(item => item && typeof item === 'object').map(({ pid, parent, started }) => ({ pid, parent, started })))}`);
  assert.equal(typeof terminate, 'function', 'Loading cancellation requires a retained caller owner');
  await terminate();
  return marker;
}

/** 完成或终止后检查全部已观察成员，幸存进程不能被根退出掩盖。 */
async function assertTreeExited(observed) {
  const deadline = Date.now() + 10_000;
  while (true) {
    const snapshot = await processSnapshot();
    observeTree(snapshot, observed);
    const live = liveObservedProcesses(snapshot, observed);
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
export async function exchangeCli(executable, pack, request, identity, expectedFailure, { cancel = false, environment = process.env, args = ['analyze'], rawInput, timeoutMs, trace, cwd = os.tmpdir(), createOwner = spawnOwnedCli } = {}) {
  const probeDirectory = cancel ? await fs.mkdtemp(path.join(os.tmpdir(), 'codegraph-discovery-probe-')) : undefined;
  const probe = probeDirectory && path.join(probeDirectory, 'reached.json');
  const env = publicEnvironment(environment);
  if (probe) env[probeVariable] = probe;
  const lifetimeMs = timeoutMs ?? (cancel ? 45_000 : 120_000);
  let owner;
  try { owner = await createOwner(executable, args, { cwd, environment: env, lifetimeMs }); }
  catch (error) { if (probeDirectory) await fs.rm(probeDirectory, { recursive: true, force: true }); throw error; }
  const { child } = owner;
  const chunks = [], observed = new Map();
  let length = 0, stderr = '', failure, stopped = false, exitStatus, decoded;
  const cleanupFailures = [];
  let stopWaiting;
  const interrupted = new Promise((_, reject) => { stopWaiting = reject; });
  interrupted.catch(() => {});
  if (child.pid) observed.set(child.pid, executable);
  /** 保留首错并唤醒等待者；真实 owner 清理失败由收尾屏障另行汇总。 */
  const stop = error => { failure ??= error; stopped = true; stopWaiting(failure); owner.reap().catch(() => {}); };
  const timer = setTimeout(() => stop(new Error('Public CLI acceptance timed out')), lifetimeMs);
  child.stdout.on('data', chunk => { length += chunk.length; if (length > 128 * 1024 * 1024) stop(new Error('Public CLI output exceeds limit')); else chunks.push(chunk); });
  child.stderr.on('data', chunk => { stderr = (stderr + chunk).slice(-262144); });
  child.stdin.on('error', error => { if (!cancel) stop(error); });
  try {
    // 刻意不加尾换行，所有真实包都必须在 EOF 正常解码完整 JSON。
    child.stdin.end(rawInput ?? JSON.stringify(request));
    if (cancel) await Promise.race([cancelLoadedProject(child, pack, request, observed, () => stopped || owner.closed() || length > 0, probe, { terminate: () => owner.reap() }), interrupted]);
    exitStatus = await Promise.race([owner.completed, interrupted]);
  } catch (error) { failure ??= error; }
  clearTimeout(timer);
  // 根退出和 pipe EOF 都不能释放后代；成功、失败、超时逐条经过同一树/排空/join屏障。
  try { await owner.reap(); }
  catch (error) { cleanupFailures.push(error); }
  exitStatus ??= owner.rootStatus();
  // 完整传输已保留输出和退出状态时，清理失败不能掩盖独立的最终结果错误。
  if (!failure && (!cleanupFailures.length || !cancel && exitStatus)) try {
    const output = Buffer.concat(chunks);
    if (cancel) {
      assert.ok(exitStatus.signal || exitStatus.code !== 0, 'Caller tree termination returned success');
      assert.equal(output.length, 0, 'Cancelled analysis emitted a final result');
      await assertTreeExited(observed);
      console.log(`Public caller tree termination verified: ${JSON.stringify({ pack, ...exitStatus, observed_processes: observed.size })}`);
    } else decoded = decodePublicResult(output, exitStatus, pack, request, identity, expectedFailure);
  } catch (error) { failure ??= error; }
  try {
    if (trace) {
      await fs.mkdir(trace, { recursive: true });
      await fs.writeFile(path.join(trace, 'request.json'), rawInput ?? JSON.stringify(request), { flag: 'wx' });
      await fs.writeFile(path.join(trace, 'stdout.json'), Buffer.concat(chunks), { flag: 'wx' });
      await fs.writeFile(path.join(trace, 'stderr.txt'), stderr, { flag: 'wx' });
      await fs.writeFile(path.join(trace, 'status.json'), JSON.stringify({ ...exitStatus, expectedFailure: expectedFailure ?? null, callerTreeTermination: cancel, failure: failure?.message ?? null, cleanupFailures: cleanupFailures.map(error => error.message) }), { flag: 'wx' });
    }
    if (probeDirectory) await fs.rm(probeDirectory, { recursive: true, force: true });
  } catch (error) { cleanupFailures.push(error); }
  try { await owner.dispose(); } catch (error) { cleanupFailures.push(error); }
  if (failure || cleanupFailures.length) {
    const error = cleanupFailures.length ? new AggregateError([failure, ...cleanupFailures].filter(Boolean), `${failure?.message ?? 'Public CLI cleanup failed'}; cleanup: ${cleanupFailures.map(error => error.message).join('; ')}`, { cause: failure }) : failure;
    error.message += stderr ? `; stderr: ${stderr}` : '';
    throw error;
  }
  return decoded;
}
