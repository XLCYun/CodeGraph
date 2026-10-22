import fs from 'node:fs/promises';
import path from 'node:path';
import { execFileSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import { nativeTarget, packIds, releaseIdentity, repository, targets } from './distribution.mjs';
import { packageCargoLicenses } from './native-language-support-packs.mjs';

const builds = new Map();

/** 构建语言自己的入口；薄入口只来自 launchers，三种非 Rust 程序由原生构建器准备。 */
export function analyzerBuildDefinition(id, target) {
  if (!packIds.includes(id) || !targets.includes(target)) throw new Error('Invalid analyzer identity');
  const binary = `codegraph-${id}`;
  const source = ['rust', 'python'].includes(id) ? `analyzers/${id}` : ['java', 'typescript-javascript'].includes(id) ? 'launchers' : undefined;
  const args = source ? ['scripts/cargo-agent.sh', ...(['rust', 'python'].includes(id) ? ['+1.98.0'] : []), 'build', '--manifest-path', `${source}/Cargo.toml`, '--locked', '--release', '--bin', binary] : undefined;
  return { binary, entry: `bin/${binary}${target.includes('windows') ? '.exe' : ''}`, source, args };
}

/** 只编译实际需要的同语言程序，锁定 crate 许可与相同 manifest 的生产依赖闭包。 */
async function prepareAnalyzer(id, target) {
  const definition = analyzerBuildDefinition(id, target);
  if (target !== nativeTarget()) throw new Error('Analyzer must be built on its native target');
  if (!definition.source) return { definition };
  const key = `${id}:${target}`;
  if (!builds.has(key)) builds.set(key, (async () => {
    execFileSync('bash', definition.args, { cwd: repository, stdio: 'inherit' });
    const targetDirectory = definition.source === 'launchers' ? 'target' : `${definition.source}/target`;
    const executable = path.join(repository, targetDirectory, 'release', path.basename(definition.entry));
    const identity = await releaseIdentity();
    // launcher 的 --version 必须在完整运行时资源暂存后再验证。
    if (definition.source !== 'launchers' && execFileSync(executable, ['--version'], { encoding: 'utf8' }).trim() !== `${definition.binary} ${identity.version}`) throw new Error('Built analyzer identity mismatch');
    return { executable, definition };
  })());
  return builds.get(key);
}

/** 安装唯一固定公开入口与五字段描述；这里不创建任何私有分析程序。 */
export async function stageAnalyzer(payload, id, target) {
  const { executable, definition } = await prepareAnalyzer(id, target);
  const identity = await releaseIdentity();
  await fs.mkdir(path.join(payload, 'bin'), { recursive: true });
  await fs.mkdir(path.join(payload, 'control'), { recursive: true });
  const entry = path.join(payload, definition.entry);
  if (executable) await fs.copyFile(executable, entry);
  if (!(await fs.stat(entry)).isFile()) throw new Error(`Missing direct analyzer: ${id}`);
  await fs.chmod(entry, 0o755);
  if (definition.source) await packageCargoLicenses(path.join(repository, definition.source), target, payload, id === 'python' ? 'Python' : id === 'rust' ? 'Rust' : 'Launcher');
  const descriptor = { schema_version: 1, pack_id: id, pack_version: identity.version, engine_version: identity.engineVersion, protocol: identity.protocol };
  await fs.writeFile(path.join(payload, 'control/codegraph.json'), `${JSON.stringify(descriptor)}\n`);
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  if (!process.argv[2] || !process.argv[3]) throw new Error('Usage: build-analyzer.mjs <language> <staging-directory>');
  await stageAnalyzer(path.resolve(process.argv[3]), process.argv[2], nativeTarget());
}
