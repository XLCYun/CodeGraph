import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';
import { decodePublicResult, exchangeCli } from '../language-packs/public-cli-session.mjs';

const identity = { protocol: 'codegraph.analyzer/1', engineVersion: '0.1.0' };
const request = { protocol: identity.protocol, type: 'analyze', id: 'transport', repository_root: '/unused', files: [], options: { include_tests: false, rust_dependency_scope: 'repository-only' } };
const graph = { root_contains: [], nodes: [], external_symbols: [] };
const result = { ...identity, engine_version: identity.engineVersion, pack_id: 'go', type: 'completed', id: request.id, outcome: 'ok', graph };
delete result.engineVersion;

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
