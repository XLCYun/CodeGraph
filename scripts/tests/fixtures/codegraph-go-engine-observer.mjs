import fs from 'node:fs/promises';
import readline from 'node:readline';

// 只用于同步回归：父测试控制初始化边界；实际验收的原生引擎没有此控制通道。
const [mode, destination, source] = process.argv.slice(2);
let phase = mode === 'never' ? 'ready-without-loading' : 'initializing';
process.send({ type: 'started', phase });
process.on('message', async message => {
  if (message === 'state') process.send({ type: 'state', phase });
  if (message === 'exit') process.exit(0);
  if (message !== 'load') return;
  await fs.readFile(source);
  phase = 'module-configurations-loaded';
  const marker = { schema_version: 1, helper_id: 'codegraph-go-engine', request_id: 'analysis-1', pid: process.pid, milestone: phase };
  if (mode === 'wrong-request') marker.request_id = 'unrelated-request';
  if (mode === 'foreign-pid') marker.pid = process.ppid;
  if (mode === 'wrong-phase') marker.milestone = 'ready';
  await fs.writeFile(`${destination}.pending`, JSON.stringify(marker));
  await fs.rename(`${destination}.pending`, destination);
  process.send({ type: 'loaded', phase });
});
readline.createInterface({ input: process.stdin }).on('line', line => {
  process.send({ type: 'cancelled', phase, request: JSON.parse(line) }, () => process.exit(0));
});
