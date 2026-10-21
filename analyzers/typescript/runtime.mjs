import fs from 'node:fs';
import path from 'node:path';
import { createRequire } from 'node:module';
import { AnalysisFailure } from './request.mjs';

/** 只读取随包固定声明；项目 node_modules 和环境变量不能替换编译器。 */
export function runtimeContext(directory = path.resolve(import.meta.dirname, '..')) {
  try {
    const packages = [['typescript', '6.0.3'], ['@types/node', '22.20.1'], ['undici-types', '6.21.0']];
    const platforms = packages.map(([name, version]) => {
      const root = path.join(directory, name);
      if (JSON.parse(fs.readFileSync(path.join(root, 'package.json'), 'utf8')).version !== version) throw new Error(`固定声明版本不符：${name}`);
      return { id: name, kind: 'typescript-declarations', version,
        root: path.join(root, name === 'typescript' ? 'lib' : ''), metadata: { package: name } };
    });
    const ts = createRequire(import.meta.url)(path.join(directory, 'typescript/lib/typescript.js'));
    return { ts, platforms };
  } catch (error) { throw new AnalysisFailure('invalid_pack', `随包 TypeScript 声明不可用：${error.code ?? error.message}`); }
}
