import fs from 'node:fs/promises';
import path from 'node:path';
import os from 'node:os';
import { processSnapshot, observeTree, liveObservedProcesses } from '../../../scripts/language-packs/public-cli-session.mjs';
import { setTimeout as delay } from 'node:timers/promises';
import { spawnOwnedCli } from '../../../scripts/language-packs/public-cli-owner.mjs';

// Exercise the existing caller-owned tree with the real product, never a replacement supervisor.
const config = JSON.parse(await fs.readFile(process.argv[2], 'utf8'));
let owner;
let stdout = '';
let stderr = '';
let pid;
try {
  owner = await spawnOwnedCli(config.command[0], config.command.slice(1), {
    cwd: config.root, environment: config.environment, lifetimeMs: 90000,
  });
  owner.child.stdout.on('data', value => { stdout += value; });
  owner.child.stderr.on('data', value => { stderr += value; });
  owner.child.stdin.on('error', () => {});
  owner.child.stdin.end(JSON.stringify(config.message));
  const observed = new Map([[owner.child.pid, config.command[0]]]);
  const deadline = Date.now() + 60000;
  while (!pid && Date.now() < deadline) {
    observeTree(await processSnapshot(), observed);
    try { pid = Number(await fs.readFile(config.marker, 'utf8')); }
    catch (error) { if (error.code !== 'ENOENT') throw error; }
    if (!pid) await delay(25);
  }
  if (!pid) throw new Error('real Gradle configuration was not reached');
  const beforeSnapshot = await processSnapshot(); observeTree(beforeSnapshot, observed);
  const before = liveObservedProcesses(beforeSnapshot, observed).find(item => item.pid === pid);
  if (!before) throw new Error('marked Gradle daemon is not live in caller-owned tree');
  await owner.reap();
  const after = await processSnapshot();
  const survivors = liveObservedProcesses(after, observed);
  const alive = survivors.some(item => item.pid === pid);
  if (survivors.length) throw new Error('caller cancellation leaked owned processes: ' + JSON.stringify(survivors));
  if (stdout || stderr) throw new Error('cancellation produced protocol noise');
  console.log(JSON.stringify({ daemon: pid, alive, observed: observed.size, daemonStateAfter: after.find(item => item.pid === pid)?.state ?? 'absent', status: owner.rootStatus() }));
} finally {
  if (owner) {
    await owner.reap(); await owner.dispose();
    try {
      const container = await fs.realpath((await fs.readFile(config.snapshotMarker, 'utf8')).trim());
      const temporary = await fs.realpath(os.tmpdir());
      if (path.dirname(container) !== temporary || !path.basename(container).startsWith('codegraph-java-input-'))
        throw new Error('unexpected owned snapshot path');
      await fs.rm(container, { recursive: true });
    } catch (error) { if (error.code !== 'ENOENT') throw error; }
  }
}
