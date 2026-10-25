import test from 'node:test';
import assert from 'node:assert/strict';
import { liveObservedProcesses, observeTree } from '../language-packs/public-cli-session.mjs';

/** Windows 的旧 PPID 不能把早于当前父进程的编译器后台服务纳入分析器子树。 */
test('observer rejects an older process whose stale parent PID matches the new owner', () => {
  const observed = new Map([[4416, undefined]]);
  observeTree([
    { pid: 4416, parent: 9000, command: 'codegraph-process-owner.exe', started: '200' },
    { pid: 7892, parent: 4416, command: 'codegraph-go.exe', started: '220' },
    { pid: 7680, parent: 4416, command: 'VCTIP.EXE', started: '100' },
  ], observed);
  assert.ok(observed.has(7892), 'Actual analyzer must remain observed');
  assert.equal(observed.has(7680), false, 'Unrelated VCTIP must not become an analyzer descendant');
});

/** 已记录进程的 PID 被复用后，不得用新命令覆盖原来的进程身份。 */
test('observer does not replace a recorded analyzer with a reused PID', () => {
  const observed = new Map([[4416, undefined]]);
  observeTree([
    { pid: 4416, parent: 9000, command: 'owner.exe', started: '200' },
    { pid: 7892, parent: 4416, command: 'codegraph-go.exe', started: '220' },
  ], observed);
  observeTree([{ pid: 7892, parent: 9999, command: 'VCTIP.EXE', started: '300' }], observed);
  const recorded = observed.get(7892);
  assert.equal(recorded?.command ?? recorded, 'codegraph-go.exe');
});

/** 父 PID 被新进程复用后不再扩展该分支，但真正的遗留后代仍必须被发现。 */
test('observer rejects a reused parent while retaining live orphaned descendants', () => {
  const observed = new Map([[4416, undefined]]);
  observeTree([
    { pid: 4416, parent: 9000, command: 'owner.exe', started: '200' },
    { pid: 7892, parent: 4416, command: 'codegraph-go.exe', started: '220' },
  ], observed);
  const snapshot = [
    { pid: 4416, parent: 9999, command: 'unrelated.exe', started: '400' },
    { pid: 9001, parent: 4416, command: 'VCTIP.EXE', started: '420' },
    { pid: 7892, parent: 1, command: 'codegraph-go.exe', started: '220' },
  ];
  observeTree(snapshot, observed);
  assert.equal(observed.has(9001), false);
  assert.deepEqual(liveObservedProcesses(snapshot, observed).map(item => item.pid), [7892]);
});

/** 同一个进程变更命令仍保留身份；多层真实子树按出生顺序完整纳入观察。 */
test('observer keeps exec transitions and genuine nested descendants', () => {
  const observed = new Map([[10, undefined]]);
  observeTree([
    { pid: 12, parent: 11, command: 'tool.exe', started: '300' },
    { pid: 11, parent: 10, command: 'launcher.exe', started: '200' },
    { pid: 10, parent: 1, command: 'owner.exe', started: '100' },
  ], observed);
  const snapshot = [
    { pid: 11, parent: 10, command: 'jvm.exe', started: '200' },
    { pid: 12, parent: 11, command: 'tool.exe', started: '300' },
  ];
  observeTree(snapshot, observed);
  assert.deepEqual(liveObservedProcesses(snapshot, observed).map(item => item.pid), [11, 12]);
});

/** 无法取得自有 Windows 进程的出生时间时，应报告观察失败。 */
test('observer reports unavailable Windows creation identity', () => {
  assert.throws(() => observeTree([{ pid: 10, parent: 1, command: 'owner.exe', started: null }], new Map([[10, undefined]])), /creation time unavailable/);
});
