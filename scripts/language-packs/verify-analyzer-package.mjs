import assert from 'node:assert/strict';
import { list } from 'tar';
import { packIds, targets } from './distribution.mjs';
import { verifyDistribution } from './verify-distribution.mjs';
import { validateNativePackPolicy } from './verify-native-pack-policy.mjs';

/** 必需入口和运行时锚点；完整依赖仍由逐文件摘要与真实公开执行共同验收。 */
export function analyzerPackageFiles(id, target) {
  if (!packIds.includes(id) || !targets.includes(target)) throw new Error('Invalid analyzer package identity');
  const suffix = target.includes('windows') ? '.exe' : '';
  const runtime = {
    rust: [], python: [], go: [],
    'typescript-javascript': [`bin/node${suffix}`, ...['main', 'request', 'runtime', 'analyze', 'graph', 'projection', 'workspace', 'config', 'syntax', 'observation'].map(name => `runtime/analyzer/${name}.mjs`), 'runtime/typescript/lib/typescript.js', 'runtime/@types/node/index.d.ts', 'runtime/undici-types/index.d.ts'],
    java: [`runtime/jdk/bin/java${suffix}`, 'runtime/analyzer/classes/dev/codegraph/java/Main.class', 'licenses/Java-dependencies.json'],
    csharp: ['bin/codegraph-csharp.dll', 'bin/codegraph-csharp.deps.json', 'bin/codegraph-csharp.runtimeconfig.json', 'bin/Microsoft.CodeAnalysis.dll', 'bin/Microsoft.CodeAnalysis.CSharp.dll', 'bin/System.Private.CoreLib.dll', ...(suffix ? ['bin/hostfxr.dll', 'bin/hostpolicy.dll', 'bin/coreclr.dll'] : ['bin/libhostfxr.dylib', 'bin/libhostpolicy.dylib', 'bin/libcoreclr.dylib']), 'licenses/Runtime-LICENSE.TXT', 'licenses/Runtime-THIRD-PARTY-NOTICES.TXT'],
    'c-cpp': ['runtime/platform/clang-resource/include/stddef.h', ...(suffix ? [] : ['runtime/lib/libclang-cpp.dylib'])],
  }[id];
  return [`bin/codegraph-${id}${suffix}`, 'control/codegraph.json', 'control/native.json', ...runtime];
}

/** 只允许该分析器及其私有依赖区域，通用或其他语言的入口不能混入归档。 */
export function validateAnalyzerPackage(manifest) {
  const { id, target } = manifest, suffix = target.includes('windows') ? '.exe' : '';
  const required = analyzerPackageFiles(id, target), files = new Map(manifest.files.map(file => [file.path, file]));
  for (const name of required) {
    const file = files.get(name);
    if (!file?.size || ((name === `bin/codegraph-${id}${suffix}` || name === `bin/node${suffix}` || name === `runtime/jdk/bin/java${suffix}`) && !file.executable)) throw new Error(`Missing package dependency or executable: ${name}`);
  }
  const regions = {
    rust: [], python: [], go: [],
    'typescript-javascript': ['runtime/analyzer/', 'runtime/typescript/', 'runtime/@types/node/', 'runtime/undici-types/'],
    java: ['runtime/jdk/', 'runtime/analyzer/classes/', 'runtime/analyzer/lib/'],
    csharp: [],
    'c-cpp': ['runtime/lib/', 'runtime/platform/clang-resource/'],
  }[id];
  for (const name of files.keys()) {
    const basename = name.split('/').at(-1).toLowerCase();
    if (name.startsWith('runtime/helper/') || /(?:ComponentDriver|CodeGraph\.CSharp\.Components|codegraph-clang-components|test-classes)/i.test(name) || /^codegraph-.*-engine(?:\.exe)?$/.test(basename)) throw new Error(`Unexpected package file: ${name}`);
    const codegraphEntry = /^codegraph(?:-[\w-]+)?(?:\.exe)?$/.test(basename);
    const csharpSidecar = id === 'csharp' && !codegraphEntry && (/^bin\/[^/]+\.(dll|json|pdb|dylib|so)$/.test(name) || /^bin\/[a-z]{2}(?:-[A-Za-z0-9]+)*\/[^/]+\.resources\.dll$/.test(name) || name === 'bin/createdump' || name === 'bin/createdump.exe') && !/^bin\/codegraph-(?!csharp\.)/.test(name);
    const allowed = required.includes(name) || !codegraphEntry && (name.startsWith('licenses/') || regions.some(prefix => name.startsWith(prefix)) || csharpSidecar || id === 'c-cpp' && suffix && /^bin\/[^/]+\.dll$/i.test(name));
    if (!allowed) throw new Error(`Unexpected package file: ${name}`);
  }
  if (!manifest.files.some(file => file.path.startsWith('licenses/') && file.size)) throw new Error('Missing package dependency: licenses');
}

/** 在完整字节验证后读取有界控制文件，发布与公开验收使用同一包策略。 */
export async function verifyAnalyzerDistribution(directory, expected = {}) {
  const result = await verifyDistribution(directory, expected);
  validateAnalyzerPackage(result.manifest);
  const controls = new Map();
  for (const name of ['control/codegraph.json', 'control/native.json']) if (result.manifest.files.find(file => file.path === name).size > 1024 * 1024) throw new Error('Package control exceeds size limit');
  await list({ file: result.archive, strict: true, onReadEntry(entry) {
    if (!['control/codegraph.json', 'control/native.json'].includes(entry.path)) return;
    const chunks = [];
    entry.on('data', chunk => chunks.push(chunk));
    entry.on('end', () => controls.set(entry.path, Buffer.concat(chunks).toString('utf8')));
  } });
  const { id, version, engineVersion, protocol } = result.manifest;
  const descriptor = { schema_version: 1, pack_id: id, pack_version: version, engine_version: engineVersion, protocol };
  // 构建器写 canonical JSON；同时拒绝重复字段、额外字段及错误身份。
  assert.equal(controls.get('control/codegraph.json').trim(), JSON.stringify(descriptor), 'Invalid public package descriptor');
  validateNativePackPolicy(id, JSON.parse(controls.get('control/native.json')), result.manifest.files.map(file => file.path));
  return result;
}
