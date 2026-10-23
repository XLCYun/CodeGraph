import fs from 'node:fs';
import path from 'node:path';
import os from 'node:os';
import { runtimeContext } from '../runtime.mjs';
import { createWorkspace } from '../workspace.mjs';
import { prepareConfigurations } from '../config.mjs';

/** 测试与生产入口共用完整准备，旧包只提供固定版本的依赖文件。 */
export const analyzerRuntime = runtimeContext(path.join(process.env.CODEGRAPH_TYPESCRIPT_PACK ?? (() => { throw new Error('CODEGRAPH_TYPESCRIPT_PACK required'); })(), 'runtime'));
export const { ts, runtime } = analyzerRuntime;
/** 旧配置/文件系统断言直接验证迁移后的原生 host，不创建旧 prepare 进程。 */
export function prepare(input) {
  const context = createWorkspace(input);
  const sources = [...context.sources].sort();
  const { host, configFiles } = prepareConfigurations(context, ts, sources);
  return { host, context, output: { sources, config_files: configFiles } };
}

/** 公共入口测试使用一次性包布局；固定声明从已有包复制，不修改旧候选。 */
export function directConfiguration(t) {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'codegraph 公开包 TS-'));
  t.after(() => fs.rmSync(root, { recursive: true, force: true }));
  fs.mkdirSync(path.join(root, 'runtime/analyzer'), { recursive: true });
  for (const file of fs.readdirSync(path.resolve(import.meta.dirname, '..')).filter(file => file.endsWith('.mjs')))
    fs.copyFileSync(path.resolve(import.meta.dirname, '..', file), path.join(root, 'runtime/analyzer', file));
  for (const { metadata: { package: name } } of analyzerRuntime.platforms) fs.cpSync(path.join(runtime, name), path.join(root, 'runtime', name), { recursive: true });
  return { executable: process.execPath, args: ['--disable-proto=throw', path.join(root, 'runtime/analyzer/main.mjs'), 'analyze'], root };
}
