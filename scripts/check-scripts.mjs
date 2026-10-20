import fs from 'node:fs/promises';
import path from 'node:path';
import { execFileSync } from 'node:child_process';
import { releaseIdentity } from './language-packs/distribution.mjs';

/** 递归检查自有 JavaScript 语法，不加载测试或执行产物发布代码。 */
async function check(directory) {
  for (const entry of await fs.readdir(directory, { withFileTypes: true })) {
    const file = path.join(directory, entry.name);
    if (entry.isDirectory()) await check(file);
    else if (file.endsWith('.mjs')) execFileSync(process.execPath, ['--check', file], { stdio: 'inherit' });
  }
}
await check('scripts');
await check('analyzers/typescript');
await releaseIdentity();
console.log('JavaScript syntax and release version contracts verified');
