import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';
import { spawn } from 'node:child_process';
import { once } from 'node:events';
import { fileURLToPath } from 'node:url';
import { cancelLoadedProject } from '../language-packs/public-cli-session.mjs';

const request = { protocol: 'codegraph.analyzer/1', id: 'public-request' };

/** 真子进程提供可观察身份，初始化由夹具控制，测试不用睡眠冒充加载证据。 */
async function fixture(t, mode) {
  t.mock.method(console, 'log', () => {});
  const directory = await fs.mkdtemp(path.join(os.tmpdir(), 'codegraph-observer-test-'));
  const probe = path.join(directory, 'reached.json'), source = path.join(directory, 'go.mod');
  await fs.writeFile(source, 'module example.test\n');
  const script = fileURLToPath(new URL('./fixtures/codegraph-go-engine-observer.mjs', import.meta.url));
  const child = spawn(process.execPath, [script, mode, probe, source], { stdio: ['pipe', 'inherit', 'inherit', 'ipc'] });
  let closed = false;
  const exit = once(child, 'close').then(() => { closed = true; });
  const messages = [];
  child.on('message', value => messages.push(value));
  child.stdin.on('error', () => {});
  t.after(async () => {
    if (!closed) child.kill('SIGKILL');
    await exit;
    await fs.rm(directory, { recursive: true, force: true });
  });
  /** 逐次确认夹具消息，避免父子事件顺序或快速退出造成漏读。 */
  async function message(type, command) {
    const existing = messages.find(value => value.type === type);
    if (existing && !command) return existing;
    const pending = once(child, 'message');
    if (command) child.send(command);
    const value = (await pending)[0];
    assert.equal(value.type, type);
    return value;
  }
  await message('started');
  return { child, probe, exit, messages, message, cancel: options => cancelLoadedProject(child, 'go', request, new Map([[child.pid, process.execPath]]), () => closed, probe, { terminate: async () => { child.kill('SIGKILL'); await exit; }, ...options }) };
}

/** 已存在的初始化进程必须先失败等待；只有真实读取后的匹配标记才能触发取消。 */
test('delayed initialization cannot satisfy loading cancellation; a later matching milestone can', { timeout: 15_000 }, async t => {
  const item = await fixture(t, 'delayed');
  await assert.rejects(item.cancel({ timeout: 200 }), /discovery\/load milestone was not observed/);
  assert.equal((await item.message('state', 'state')).phase, 'initializing');
  assert.ok(!item.messages.some(value => value.type === 'unexpected-stdin'));
  await item.message('loaded', 'load');
  const marker = await item.cancel();
  assert.equal(marker.pid, item.child.pid);
  assert.ok(!item.messages.some(value => value.type === 'unexpected-stdin'));
  await item.exit;
});

/** 已 ready 却从未进入加载的 helper 也不能被记为成功的加载期取消。 */
test('ready helper without a discovery milestone fails without sending cancellation', { timeout: 15_000 }, async t => {
  const item = await fixture(t, 'never');
  await assert.rejects(item.cancel({ timeout: 200 }), /discovery\/load milestone was not observed/);
  assert.equal((await item.message('state', 'state')).phase, 'waiting-without-loading');
  assert.ok(!item.messages.some(value => value.type === 'unexpected-stdin'));
});

/** 标记不能冒用其它请求、非所属进程或启动阶段；严格失败后仍不发送取消。 */
for (const mode of ['wrong-request', 'foreign-pid', 'wrong-phase']) {
  test(`discovery observation rejects ${mode}`, { timeout: 15_000 }, async t => {
    const item = await fixture(t, mode);
    await item.message('loaded', 'load');
    await assert.rejects(item.cancel(), mode === 'foreign-pid' ? /outside the owned process tree/ : { code: 'ERR_ASSERTION' });
    assert.ok(!item.messages.some(value => value.type === 'unexpected-stdin'));
  });
}

/** 即使先留下合法标记，已退出的请求也不能获得加载期取消成功声明。 */
test('completion before cancellation rejects even an otherwise valid milestone', { timeout: 15_000 }, async t => {
  const item = await fixture(t, 'delayed');
  await item.message('loaded', 'load');
  item.child.send('exit');
  await item.exit;
  await assert.rejects(item.cancel(), /request completed before real load cancellation/);
});
