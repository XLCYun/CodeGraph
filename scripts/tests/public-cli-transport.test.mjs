import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';
import { setTimeout as delay } from 'node:timers/promises';
import { preparePublicCliOwner, spawnOwnedCli } from '../language-packs/public-cli-owner.mjs';
import { decodePublicResult, exchangeCli } from '../language-packs/public-cli-session.mjs';

const identity = { protocol: 'codegraph.analyzer/1', engineVersion: '0.1.1' };
const request = { protocol: identity.protocol, type: 'analyze', id: 'transport', repository_root: '/unused', files: [], options: { include_tests: false, rust_dependency_scope: 'repository-only' } };
const graph = { root_contains: [], nodes: [], external_symbols: [] };
const result = { ...identity, engine_version: identity.engineVersion, pack_id: 'go', type: 'completed', id: request.id, outcome: 'ok', graph };
delete result.engineVersion;
test.before(preparePublicCliOwner);

/** 成功退出或单独 EOF 都不充分，唯一完整结果和最终身份缺一不可。 */
test('public decoder rejects missing malformed duplicate lifecycle and nonzero success results', () => {
  const bytes = value => Buffer.from(JSON.stringify(value) + '\n');
  assert.deepEqual(decodePublicResult(bytes(result), { code: 0, signal: null }, 'go', request, identity), graph);
  for (const output of [Buffer.from(''), Buffer.from('{'), Buffer.from(JSON.stringify(result)), bytes([result]), Buffer.concat([bytes({ type: 'ready' }), bytes(result)]), Buffer.concat([bytes(result), bytes(result)]), bytes({ ...result, pack_id: 'rust' }), bytes({ ...result, engine_version: '9.9.9' }), bytes({ ...result, id: 'other' })]) assert.throws(() => decodePublicResult(output, { code: 0, signal: null }, 'go', request, identity));
  assert.throws(() => decodePublicResult(bytes(result), { code: 1, signal: null }, 'go', request, identity));
  assert.throws(() => decodePublicResult(bytes(result), { code: null, signal: 'SIGKILL' }, 'go', request, identity));
});

/** 前置请求失败可尚未接受 ID，或已经接受同一个合法 ID；成功和分析失败仍必须匹配。 */
test('public decoder binds early-error ids to null or the independently valid request id', () => {
  const bytes = value => Buffer.from(JSON.stringify(value) + '\n');
  const failure = (code, id) => ({ protocol: identity.protocol, engine_version: identity.engineVersion, pack_id: 'go', type: 'completed', id, outcome: 'error', error: { code, message: 'actual failure' } });
  for (const code of ['invalid_request', 'incompatible_protocol']) {
    for (const id of [null, request.id]) decodePublicResult(bytes(failure(code, id)), { code: 1, signal: null }, 'go', request, identity, code);
    assert.throws(() => decodePublicResult(bytes(failure(code, 'other')), { code: 1, signal: null }, 'go', request, identity, code));
    for (const id of ['', ' \t', '\u0085', '\0', 'x'.repeat(1025), '\ud800', 7, []]) assert.throws(() => decodePublicResult(bytes(failure(code, id)), { code: 1, signal: null }, 'go', { ...request, id }, identity, code));
  }
  assert.throws(() => decodePublicResult(bytes(failure('analysis_failed', null)), { code: 1, signal: null }, 'go', request, identity, 'analysis_failed'));
  decodePublicResult(bytes(failure('analysis_failed', request.id)), { code: 1, signal: null }, 'go', request, identity, 'analysis_failed');
});

/** 真实子进程必须先读到 EOF；UTF-8 无尾换行输入与 stderr 在实际管道中往返。 */
test('one-shot exchange closes stdin and accepts only matching actual stdio and exit', async () => {
  const code = `const chunks=[];process.stdin.on('data',c=>chunks.push(c));process.stdin.on('end',()=>{const input=JSON.parse(Buffer.concat(chunks));if(Buffer.concat(chunks).at(-1)===10)process.exit(9);process.stderr.write('bounded diagnostic');console.log(JSON.stringify({...${JSON.stringify(result)},id:input.id}));});`;
  assert.deepEqual(await exchangeCli(process.execPath, 'go', request, identity, undefined, { args: ['-e', code] }), graph);
  for (const body of ['process.stdin.resume();process.stdin.on("end",()=>process.exit(0))', `process.stdin.resume();process.stdin.on('end',()=>{console.log(JSON.stringify(${JSON.stringify(result)}));process.exit(7);})`, 'process.stdin.resume();process.stdin.on("end",()=>console.log("{truncated"))']) await assert.rejects(exchangeCli(process.execPath, 'go', request, identity, undefined, { args: ['-e', body] }));
});

/** 排空完整 stderr 管道并按原字节拼接被拆开的 Unicode，运行时随机源也须可用。 */
test('one-shot exchange preserves fragmented UTF-8 and drains a full stderr pipe', async () => {
  const input = { ...request, id: '片段-é-😀' };
  const code = `require('node:crypto').randomBytes(16);process.stdin.resume();process.stdin.on('end',()=>{process.stderr.write(Buffer.alloc(1024*1024,120),()=>{const bytes=Buffer.from(JSON.stringify({...${JSON.stringify(result)},id:${JSON.stringify(input.id)}})+'\\n');for(const byte of bytes)process.stdout.write(Buffer.from([byte]));});});`;
  assert.deepEqual(await exchangeCli(process.execPath, 'go', input, identity, undefined, { args: ['-e', code] }), graph);
});

/** stdout 结束不能提前丢弃随后到达的真实启动诊断。 */
test('one-shot exchange retains delayed stderr after stdout EOF', async () => {
  const code = 'process.stdin.resume();process.stdout.end();setTimeout(()=>{process.stderr.write("late-startup-diagnostic-é");process.exit(9)},30);';
  await assert.rejects(exchangeCli(process.execPath, 'go', request, identity, undefined, { args: ['-e', code] }), /late-startup-diagnostic-é/);
});

/** 子进程完全不读 stdin 时，调用方仍须终止实际进程并释放未完成的大写入。 */
test('one-shot exchange bounds a child that never reads stdin', async t => {
  const root = await fs.mkdtemp(path.join(os.tmpdir(), 'codegraph-blocked-input-'));
  t.after(() => fs.rm(root, { recursive: true, force: true }));
  const marker = path.join(root, 'pid');
  const code = `require('node:fs').writeFileSync(${JSON.stringify(marker)},String(process.pid));setInterval(()=>{},1000)`;
  const started = Date.now();
  await assert.rejects(exchangeCli(process.execPath, 'go', request, identity, undefined, { args: ['-e', code], rawInput: Buffer.alloc(8 * 1024 * 1024, 32), timeoutMs: 300 }), /timed out/);
  assert.ok(Date.now() - started < 5000);
  const pid = Number(await fs.readFile(marker, 'utf8'));
  assert.throws(() => process.kill(pid, 0), { code: 'ESRCH' });
});

/** PID 标记只由本用例的真实后代写入，测试兜底不能掩盖调用方未完成回收。 */
async function descendantFixture(t, inherited, final = result) {
  const directory = await fs.mkdtemp(path.join(os.tmpdir(), 'codegraph-owned-descendant-'));
  const marker = path.join(directory, 'child.json');
  let fallback = false;
  const cleanup = async () => {
    const record = await fs.readFile(marker, 'utf8').then(JSON.parse).catch(() => undefined);
    if (record) {
      try { process.kill(record.pid, 0); fallback = true; process.kill(record.pid, 'SIGKILL'); } catch (error) { if (error.code !== 'ESRCH') throw error; }
    }
  };
  t.after(async () => { await cleanup(); await fs.rm(directory, { recursive: true, force: true }); });
  const descendant = `require('node:fs').writeFileSync(${JSON.stringify(marker)},JSON.stringify({pid:process.pid,parent:process.ppid}));setInterval(()=>{},1000);`;
  // Windows 的 unref 不会避开 libuv 内层 kill-on-close Job；detached 不设 breakaway，仍归 native owner 的外层 Job。
  // Unix 保留同一进程组，否则会绕过本用例要验证的真实树回收。
  const spawnOptions = `stdio:${JSON.stringify(inherited ? ['ignore', 'inherit', 'inherit'] : 'ignore')}${process.platform === 'win32' ? ',detached:true' : ''}`;
  const body = `process.stdin.resume();process.stdin.on('end',()=>{const fs=require('node:fs');const child=require('node:child_process').spawn(process.execPath,['-e',${JSON.stringify(descendant)}],{${spawnOptions}});child.unref();const timer=setInterval(()=>{if(fs.existsSync(${JSON.stringify(marker)})){clearInterval(timer);${inherited ? 'process.stderr.write("root-exited-before-caller-deadline",()=>process.exit(0));' : `process.stdout.write(${JSON.stringify(JSON.stringify(final) + '\n')},()=>process.exit(0));`}}},5);});`;
  return { body, marker, cleanup, usedFallback: () => fallback };
}

/** 有界等待真实后代启动，不能把未启动的用例当作清理成功。 */
async function readDescendant(marker) {
  const deadline = Date.now() + 2000;
  while (true) {
    const record = await fs.readFile(marker, 'utf8').then(JSON.parse).catch(() => undefined);
    if (record) return record;
    assert.ok(Date.now() < deadline, 'Real descendant did not reach its PID milestone');
    await delay(10);
  }
}

/** 按真实 PID 有界确认根已退出，不能用固定睡眠假设 Windows 的启动/退出调度。 */
async function waitForRootExit(pid) {
  const deadline = Date.now() + 1000;
  while (true) {
    try { process.kill(pid, 0); }
    catch (error) { if (error.code === 'ESRCH') return; throw error; }
    assert.ok(Date.now() < deadline, 'Fixture root did not exit before the caller deadline');
    await delay(10);
  }
}

/** 两条超时路径都先观察真实根退、后代持管道且调用未结束，再等待同一个完整调用预算。 */
async function observeHeldDescendant(t, fixture, pending, started) {
  let settled = false;
  pending.finally(() => { settled = true; }).catch(() => {});
  const record = await readDescendant(fixture.marker);
  await waitForRootExit(record.parent);
  process.kill(record.pid, 0);
  assert.equal(settled, false, 'Descendant must still hold the inherited pipes after root exit');
  const elapsed = Date.now() - started;
  assert.ok(elapsed < 2000, 'Root exit must precede the caller deadline');
  t.diagnostic(`Real inherited-pipe milestone: ${JSON.stringify({ platform: process.platform, root: record.parent, descendant: record.pid, rootExited: true, descendantLive: true, callSettled: settled, elapsedMs: elapsed, startupObservationBudgetMs: 2000, rootExitObservationBudgetMs: 1000, fullCallBudgetMs: 2000 })}`);
  return record;
}

/** 正常成功也必须在返回前回收已退出根留下的闭管道后代。 */
test('one-shot exchange reaps a closed-stdio descendant after valid final and root exit', async t => {
  const fixture = await descendantFixture(t, false);
  const started = Date.now();
  assert.deepEqual(await exchangeCli(process.execPath, 'go', request, identity, undefined, { args: ['-e', fixture.body], timeoutMs: 2000 }), graph);
  const record = await readDescendant(fixture.marker);
  assert.throws(() => process.kill(record.parent, 0), { code: 'ESRCH' });
  assert.throws(() => process.kill(record.pid, 0), { code: 'ESRCH' });
  assert.equal(fixture.usedFallback(), false);
  assert.ok(Date.now() - started < 5000);
});

/** 错误最终身份仍须先回收真实遗留子树，再向调用方报告原协议错误。 */
test('one-shot exchange reaps a closed-stdio descendant before rejecting an invalid final', async t => {
  const fixture = await descendantFixture(t, false, { ...result, pack_id: 'wrong-pack' });
  await assert.rejects(exchangeCli(process.execPath, 'go', request, identity, undefined, { args: ['-e', fixture.body], timeoutMs: 2000 }), /Public final identity mismatch/);
  const record = await readDescendant(fixture.marker);
  assert.throws(() => process.kill(record.parent, 0), { code: 'ESRCH' });
  assert.throws(() => process.kill(record.pid, 0), { code: 'ESRCH' });
  assert.equal(fixture.usedFallback(), false);
});

/** 根先退出而后代持有真实继承管道时，deadline仍须有界清理并保留首错和实际诊断。 */
test('one-shot exchange bounds inherited descendant pipes after root exit at caller deadline', async t => {
  const fixture = await descendantFixture(t, true);
  const started = Date.now();
  const pending = exchangeCli(process.execPath, 'go', request, identity, undefined, { args: ['-e', fixture.body], timeoutMs: 2000 });
  const record = await observeHeldDescendant(t, fixture, pending, started);
  const watchdog = setTimeout(() => fixture.cleanup().catch(() => {}), 4000);
  try { await assert.rejects(pending, /Public CLI acceptance timed out.*root-exited-before-caller-deadline/s); }
  finally { clearTimeout(watchdog); }
  assert.equal(fixture.usedFallback(), false, 'Caller needed external fixture cleanup to close descendant pipes');
  assert.throws(() => process.kill(record.pid, 0), { code: 'ESRCH' });
  assert.ok(Date.now() - started < 5000);
});

/** 真实树已清理后注入收尾报告失败，首个 deadline 和实际 stderr 仍须可见。 */
test('one-shot exchange preserves primary failure alongside a cleanup failure', async t => {
  const fixture = await descendantFixture(t, true);
  const cleanupFailure = new Error('fixture cleanup receipt failure');
  const started = Date.now();
  const pending = exchangeCli(process.execPath, 'go', request, identity, undefined, {
    args: ['-e', fixture.body], timeoutMs: 2000,
    createOwner: async (...args) => {
      const owner = await spawnOwnedCli(...args);
      return { ...owner, reap: async () => { await owner.reap(); throw cleanupFailure; } };
    },
  });
  const record = await observeHeldDescendant(t, fixture, pending, started);
  const watchdog = setTimeout(() => fixture.cleanup().catch(() => {}), 4000);
  try {
    await assert.rejects(pending, error => {
      assert.ok(error instanceof AggregateError);
      assert.match(error.cause.message, /Public CLI acceptance timed out/);
      assert.equal(error.errors[0], error.cause);
      assert.ok(error.errors.includes(cleanupFailure));
      assert.match(error.message, /fixture cleanup receipt failure/);
      assert.match(error.message, /root-exited-before-caller-deadline/);
      return true;
    });
  } finally { clearTimeout(watchdog); }
  assert.throws(() => process.kill(record.parent, 0), { code: 'ESRCH' });
  assert.throws(() => process.kill(record.pid, 0), { code: 'ESRCH' });
  assert.equal(fixture.usedFallback(), false);
  assert.ok(Date.now() - started < 5000);
});

/** 已收集完整输出后注入清理收据失败，协议首错仍须独立保留。 */
test('one-shot exchange retains invalid-final diagnostics alongside cleanup failure', async () => {
  const cleanupFailure = new Error('fixture cleanup receipt failure');
  const code = `process.stdin.resume();process.stdin.on('end',()=>console.log(JSON.stringify(${JSON.stringify({ ...result, pack_id: 'foreign-pack' })})));`;
  await assert.rejects(exchangeCli(process.execPath, 'go', request, identity, undefined, {
    args: ['-e', code],
    createOwner: async (...args) => {
      const owner = await spawnOwnedCli(...args);
      return { ...owner, reap: async () => { await owner.reap(); throw cleanupFailure; } };
    },
  }), error => {
    assert.ok(error instanceof AggregateError);
    assert.match(error.cause?.message ?? '', /Public final identity mismatch/);
    assert.equal(error.errors[0], error.cause);
    assert.ok(error.errors.includes(cleanupFailure));
    return true;
  });
});
