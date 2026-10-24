import fs from 'node:fs';

/** 只在验收显式提供独立位置时记录实际加载；不发送协议消息、不暂停工作。 */
export function createObservation(requestId) {
  const destination = process.env.CODEGRAPH_TEST_DISCOVERY_PROBE;
  let recorded = false;
  return () => {
    if (!destination || recorded) return;
    const bytes = JSON.stringify({ schema_version: 1, helper_id: 'codegraph-typescript-javascript', request_id: requestId, pid: process.pid, milestone: 'repository-snapshot-read' });
    if (Buffer.byteLength(bytes) > 4096) throw new Error('验收观察超过限额');
    const pending = destination.replace(/\.json$/, '.pending');
    fs.mkdirSync(pending);
    try {
      const temporary = `${pending}/marker.json`;
      fs.writeFileSync(temporary, bytes, { flag: 'wx' });
      fs.linkSync(temporary, destination);
      recorded = true;
    } finally { fs.rmSync(pending, { recursive: true, force: true }); }
  };
}
