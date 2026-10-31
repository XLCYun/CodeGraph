import fs from 'node:fs/promises';
import path from 'node:path';
import os from 'node:os';
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
  const deadline = Date.now() + 60000;
  while (!pid && Date.now() < deadline) {
    try { pid = Number(await fs.readFile(config.marker, 'utf8')); }
    catch (error) { if (error.code !== 'ENOENT') throw error; }
    if (!pid) await delay(25);
  }
  if (!pid) throw new Error('real Gradle configuration was not reached');
  await delay(200);
  await owner.reap();
  let alive = true;
  try { process.kill(pid, 0); } catch (error) { if (error.code === 'ESRCH') alive = false; else throw error; }
  if (alive) throw new Error('caller cancellation leaked owned daemon');
  if (stdout || stderr) throw new Error('cancellation produced protocol noise');
  console.log(JSON.stringify({ daemon: pid, alive, status: owner.rootStatus() }));
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
