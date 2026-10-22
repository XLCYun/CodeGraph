import test from 'node:test';
import assert from 'node:assert/strict';
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

/** 真实子进程必须先读到 EOF；UTF-8 无尾换行输入与 stderr 在实际管道中往返。 */
test('one-shot exchange closes stdin and accepts only matching actual stdio and exit', async () => {
  const code = `const chunks=[];process.stdin.on('data',c=>chunks.push(c));process.stdin.on('end',()=>{const input=JSON.parse(Buffer.concat(chunks));if(Buffer.concat(chunks).at(-1)===10)process.exit(9);process.stderr.write('bounded diagnostic');console.log(JSON.stringify({...${JSON.stringify(result)},id:input.id}));});`;
  assert.deepEqual(await exchangeCli(process.execPath, 'go', request, identity, undefined, { args: ['-e', code] }), graph);
  for (const body of ['process.stdin.resume();process.stdin.on("end",()=>process.exit(0))', `process.stdin.resume();process.stdin.on('end',()=>{console.log(JSON.stringify(${JSON.stringify(result)}));process.exit(7);})`, 'process.stdin.resume();process.stdin.on("end",()=>console.log("{truncated"))']) await assert.rejects(exchangeCli(process.execPath, 'go', request, identity, undefined, { args: ['-e', body] }));
});
