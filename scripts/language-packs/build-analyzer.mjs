import fs from 'node:fs/promises';
import path from 'node:path';
import { execFileSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import { nativeTarget, packIds, releaseIdentity, repository, targets } from './distribution.mjs';
import { packageCargoLicenses } from './native-language-support-packs.mjs';

const analyzerBuilds = new Map();
let runtimeLicenses;

/** 每个语言对应独立 Cargo target，入口名与包身份都不能由调用方覆盖。 */
export function analyzerBuildDefinition(id, target) {
  if (!packIds.includes(id) || !targets.includes(target)) throw new Error('Invalid analyzer identity');
  const binary = `codegraph-${id}`;
  return { binary, entry: `bin/${binary}${target.includes('windows') ? '.exe' : ''}`,
    args: ['scripts/cargo-agent.sh', 'build', '--locked', '--release', '--bin', binary] };
}

/** 共享依赖许可证只收集一次，语言可执行文件按独立 target 构建和核对。 */
async function prepareAnalyzer(id, target) {
  const definition = analyzerBuildDefinition(id, target);
  if (target !== nativeTarget()) throw new Error('Analyzer must be built on its native target');
  const key = `${id}:${target}`;
  if (!analyzerBuilds.has(key)) analyzerBuilds.set(key, (async () => {
    execFileSync('bash', definition.args, { cwd: repository, stdio: 'inherit' });
    const executable = path.join(repository, 'target/release', path.basename(definition.entry));
    const identity = await releaseIdentity();
    if (execFileSync(executable, ['--version'], { encoding: 'utf8' }).trim() !== `${definition.binary} ${identity.version}`) throw new Error('Built analyzer identity mismatch');
    if (!runtimeLicenses) runtimeLicenses = (async () => {
      const licenses = path.join(repository, 'target/distribution-licenses');
      await fs.rm(licenses, { recursive: true, force: true });
      await fs.mkdir(licenses, { recursive: true });
      await packageCargoLicenses(repository, target, licenses, 'CodeGraph');
      return licenses;
    })();
    return { executable, licenses: await runtimeLicenses, definition };
  })());
  return analyzerBuilds.get(key);
}

/** 包内保存与语言绑定的独立入口和五字段公开描述，依赖仍由对应构建器准备。 */
export async function stageAnalyzer(payload, id, target) {
  const { executable, licenses, definition } = await prepareAnalyzer(id, target);
  const identity = await releaseIdentity();
  await fs.mkdir(path.join(payload, 'bin'), { recursive: true });
  await fs.mkdir(path.join(payload, 'control'), { recursive: true });
  const entry = path.join(payload, definition.entry);
  await fs.copyFile(executable, entry);
  await fs.chmod(entry, 0o755);
  await fs.cp(path.join(licenses, 'licenses'), path.join(payload, 'licenses/codegraph'), { recursive: true });
  const descriptor = { schema_version: 1, pack_id: id, pack_version: identity.version, engine_version: identity.engineVersion, protocol: identity.protocol };
  await fs.writeFile(path.join(payload, 'control/codegraph.json'), `${JSON.stringify(descriptor)}\n`);
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  if (!process.argv[2] || !process.argv[3]) throw new Error('Usage: build-analyzer.mjs <language> <staging-directory>');
  await stageAnalyzer(path.resolve(process.argv[3]), process.argv[2], nativeTarget());
}
