import fs from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';
import { spawn, execFile } from 'node:child_process';
import { promisify } from 'node:util';
import { fileURLToPath } from 'node:url';
import { setTimeout as delay } from 'node:timers/promises';

const repository = fileURLToPath(new URL('../../', import.meta.url));
const run = promisify(execFile);
let prepared;

/** 非交付所有者每个 Node 进程只检查构建一次，所有 Cargo 仍走仓库锁和 sccache。 */
export function preparePublicCliOwner() {
  return prepared ??= (async () => {
    const { stdout, stderr } = await run('bash', ['scripts/cargo-agent.sh', 'build', '--locked', '--manifest-path', 'analyzers/windows-process-probe/Cargo.toml', '--bin', 'codegraph-process-owner'], { cwd: repository, timeout: 180_000, maxBuffer: 1024 * 1024 });
    if (stdout) process.stderr.write(stdout);
    if (stderr) process.stderr.write(stderr);
    return path.join(repository, 'analyzers/windows-process-probe/target/debug', `codegraph-process-owner${process.platform === 'win32' ? '.exe' : ''}`);
  })();
}

/** 有界等待必须由调用方取消计时器，不能用悬挂的 Promise 掩盖收尾失败。 */
async function bounded(promise, milliseconds, message) {
  let timer;
  try { return await Promise.race([promise, new Promise((_, reject) => { timer = setTimeout(() => reject(new Error(message)), milliseconds); })]); }
  finally { clearTimeout(timer); }
}

/** 状态仅通过自有临时目录传递，公开标准管道保持原字节和 EOF。 */
export async function spawnOwnedCli(executable, args, { cwd, environment, lifetimeMs }) {
  const utility = await preparePublicCliOwner();
  const directory = await fs.mkdtemp(path.join(os.tmpdir(), 'codegraph-cli-owner-'));
  const child = spawn(utility, [directory, String(lifetimeMs + 10_000), executable, ...args], { cwd, env: environment, windowsHide: true, stdio: ['pipe', 'pipe', 'pipe'] });
  let closed = false, disposed = false, cleanup, rootStatus;
  const exit = new Promise((resolve, reject) => {
    child.once('error', reject);
    child.once('close', (code, signal) => { closed = true; resolve({ code, signal }); });
  });
  exit.catch(() => {});
  /** 缺少状态仅表示尚未发生，其他 IO 错误必须显式失败。 */
  const read = async name => fs.readFile(path.join(directory, name), 'utf8').catch(error => { if (error.code !== 'ENOENT') throw error; });
  /** 根退出状态随 owner 回收后仍可读取，不依赖此前监视器是否已报告传输错误。 */
  const readRoot = async () => {
    const status = await read('root-exited.json');
    if (status !== undefined) {
      const actual = JSON.parse(status);
      rootStatus = { code: actual.code, signal: actual.signal === null ? null : Object.entries(os.constants.signals).find(([, value]) => value === actual.signal)?.[0] ?? `signal-${actual.signal}` };
    }
    return rootStatus;
  };
  const completed = (async () => {
    while (true) {
      if (disposed) throw new Error('Public CLI owner monitoring stopped without completed status');
      for (const pipe of ['stdout', 'stderr']) {
        const error = await read(`${pipe}-error`);
        if (error !== undefined) throw new Error(`Public CLI ${pipe} failed: ${error}`);
      }
      if (await readRoot()) {
        if (await read('stdout-closed') !== undefined && await read('stderr-closed') !== undefined) return rootStatus;
      }
      const error = await read('cleanup-error');
      if (error !== undefined) throw new Error(`Public CLI owner failed: ${error}`);
      const inputError = await read('stdin-error');
      if (inputError !== undefined && !cleanup) throw new Error(`Public CLI input failed: ${inputError}`);
      if (closed) throw new Error('Public CLI owner exited without completed root and pipe status');
      await delay(10);
    }
  })();
  completed.catch(() => {});
  return {
    child, completed,
    rootStatus: () => rootStatus,
    closed: () => closed,
    /** 所有路径都要求 retained native owner 回收成功、join 完成和真实 stdio close。 */
    reap() {
      return cleanup ??= (async () => {
        child.stdin.destroy();
        await fs.writeFile(path.join(directory, 'cleanup'), '', { flag: 'wx' });
        let status;
        try { status = await bounded(exit, 8000, 'Public CLI owner cleanup/pipe-close deadline exceeded'); }
        catch (error) {
          const failures = [error];
          try { if (!closed && !child.kill('SIGKILL')) failures.push(new Error('Public CLI owner could not be terminated')); }
          catch (cause) { failures.push(cause); }
          // 超出正常屏障后不再假称完成排空；关闭本地句柄并明确报告强制收尾结果。
          child.stdout.destroy();
          child.stderr.destroy();
          child.unref();
          try { await bounded(exit, 1000, 'Public CLI owner forced-close deadline exceeded'); }
          catch (cause) { failures.push(cause); }
          throw new AggregateError(failures, failures.map(cause => cause.message).join('; '), { cause: error });
        }
        const failure = await read('cleanup-error');
        if (failure !== undefined) throw new Error(`Public CLI tree/drain cleanup failed: ${failure}`);
        if (status.code !== 0 || status.signal || await read('cleanup-complete') === undefined) throw new Error(`Public CLI owner cleanup unconfirmed: ${JSON.stringify(status)}`);
        await readRoot();
        await bounded(completed.catch(() => {}), 1000, 'Public CLI owner monitor did not stop after cleanup');
      })();
    },
    /** 状态目录只在 owner 和已完成的状态读取结束后删除。 */
    async dispose() { disposed = true; await bounded(completed.catch(() => {}), 1000, 'Public CLI owner monitor did not stop'); await fs.rm(directory, { recursive: true, force: true }); },
  };
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) await preparePublicCliOwner();
