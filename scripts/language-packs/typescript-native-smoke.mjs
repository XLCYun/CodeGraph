import fs from 'node:fs/promises';
import path from 'node:path';
import os from 'node:os';
import { exchangeCli } from './public-cli-session.mjs';
import { releaseIdentity } from './distribution.mjs';

/** 归档前后通过唯一固定入口验证真实跨 TS/JS 调用和随包标准声明。 */
export async function smokeTypeScript(root) {
  const scratch = await fs.mkdtemp(path.join(os.tmpdir(), 'codegraph-ts-direct-'));
  try {
    await fs.writeFile(path.join(scratch, 'main.ts'), "import {target} from './target.js'; export function caller() { target(); Promise.resolve(1); }");
    await fs.writeFile(path.join(scratch, 'target.js'), 'export function target() { return 1; }');
    const request = { protocol: 'codegraph.analyzer/1', type: 'analyze', id: 'typescript-pack-smoke', repository_root: scratch, files: ['main.ts', 'target.js'], options: { include_tests: false, rust_dependency_scope: 'repository-only' } };
    const graph = await exchangeCli(path.join(root, `bin/codegraph-typescript-javascript${process.platform === 'win32' ? '.exe' : ''}`), 'typescript-javascript', request, await releaseIdentity());
    const caller = graph.nodes.find(node => node.name === 'caller'), target = graph.nodes.find(node => node.name === 'target');
    if (!caller?.references.some(ref => ref.semantic === 'code.calls' && ref.target.id === target?.id) || !caller.references.some(ref => ref.target.kind === 'external') || target.language !== 'javascript') throw new Error('TypeScript public smoke lacks concrete cross-language/standard calls');
  } finally { await fs.rm(scratch, { recursive: true, force: true }); }
}
