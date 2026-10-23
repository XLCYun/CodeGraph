import fs from 'node:fs';
import path from 'node:path';
import { createRequire } from 'node:module';
import { AnalysisFailure } from './request.mjs';
import { canonicalPath } from './workspace.mjs';

/** 一次加载并校验固定编译器、声明 catalog 和真实目录，供分析直接使用。 */
export function runtimeContext(directory = path.resolve(import.meta.dirname, '..')) {
  let ts, platforms;
  try {
    const packages = [['typescript', '6.0.3'], ['@types/node', '22.20.1'], ['undici-types', '6.21.0']];
    platforms = packages.map(([name, version]) => {
      const root = path.join(directory, name);
      if (JSON.parse(fs.readFileSync(path.join(root, 'package.json'), 'utf8')).version !== version) throw new Error(`固定声明版本不符：${name}`);
      return { id: name, kind: 'typescript-declarations', version,
        root: path.join(root, name === 'typescript' ? 'lib' : ''), metadata: { package: name } };
    });
    ts = createRequire(import.meta.url)(path.join(directory, 'typescript/lib/typescript.js'));
  } catch (error) { throw new AnalysisFailure('invalid_pack', `随包 TypeScript 声明不可用：${error.code ?? error.message}`); }
  // 保留既有失败边界：加载/声明版本错误属于 invalid_pack，编译器与目录冲突属于分析失败。
  if (ts.version !== '6.0.3') throw new Error('需要固定 TypeScript 6.0.3 与声明平台');
  const libraryRoot = canonicalPath(platforms.find(p => p.metadata.package === 'typescript').root);
  const runtime = path.dirname(path.dirname(libraryRoot));
  for (const platform of platforms) {
    const name = platform.metadata.package;
    const expected = path.join(runtime, name, name === 'typescript' ? 'lib' : '');
    if (canonicalPath(platform.root) !== canonicalPath(expected)) throw new Error('声明 catalog 与固定运行时目录不一致');
  }
  return { ts, platforms, runtime, libraryRoot };
}
