import fs from 'node:fs/promises';
import path from 'node:path';
import { repository } from './distribution.mjs';

/** 暂存完整生产模块；测试文件和旧私有 helper 目录均不进入归档。 */
export async function stageTypeScript(payload) {
  const source = path.join(repository, 'analyzers/typescript');
  const destination = path.join(payload, 'runtime/analyzer');
  await fs.mkdir(destination, { recursive: true });
  for (const name of (await fs.readdir(source)).filter(name => name.endsWith('.mjs')).sort()) {
    await fs.copyFile(path.join(source, name), path.join(destination, name));
  }
}
