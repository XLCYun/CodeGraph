import fs from 'node:fs/promises';
import path from 'node:path';
import { execFileSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import { nativeTarget, releaseIdentity, repository } from './distribution.mjs';
import { packageCargoLicenses } from './native-language-support-packs.mjs';

let engineBuild;
/** 一次进程只构建一次 CLI，所有语言包复制相同优化 binary 和许可。 */
async function prepareEngine(target) {
  if (target !== nativeTarget()) throw new Error('CLI must be built on its native target');
  if (!engineBuild) engineBuild = (async () => {
    execFileSync('bash', ['scripts/cargo-agent.sh', 'build', '--locked', '--release', '--bin', 'codegraph'], { cwd: repository, stdio: 'inherit' });
    const executable = path.join(repository, `target/release/codegraph${process.platform === 'win32' ? '.exe' : ''}`);
    const identity = await releaseIdentity();
    if (execFileSync(executable, ['--version'], { encoding: 'utf8' }).trim() !== `codegraph ${identity.version}`) throw new Error('Built CLI version mismatch');
    const licenses = path.join(repository, 'target/distribution-licenses');
    await fs.rm(licenses, { recursive: true, force: true });
    await fs.mkdir(licenses, { recursive: true });
    await packageCargoLicenses(repository, target, licenses, 'CodeGraph');
    return { executable, licenses };
  })();
  return engineBuild;
}

/** 每包同时保存公开描述和相同 CLI；私有 native.json 保持原 schema。 */
export async function stageEngine(payload, id, target) {
  const { executable, licenses } = await prepareEngine(target);
  const identity = await releaseIdentity();
  await fs.mkdir(path.join(payload, 'bin'), { recursive: true });
  await fs.mkdir(path.join(payload, 'control'), { recursive: true });
  const entry = path.join(payload, 'bin', path.basename(executable));
  await fs.copyFile(executable, entry);
  await fs.chmod(entry, 0o755);
  await fs.cp(path.join(licenses, 'licenses'), path.join(payload, 'licenses/codegraph'), { recursive: true });
  const descriptor = id === 'codegraph' ? identity : { schema_version: 1, pack_id: id, pack_version: identity.version, engine_version: identity.engineVersion, protocol: identity.protocol };
  await fs.writeFile(path.join(payload, `control/${id === 'codegraph' ? 'engine' : 'codegraph'}.json`), `${JSON.stringify(descriptor)}\n`);
}

/** 独立 CLI 归档和每包 CLI 完全相同，允许手工消费者另行指定 pack-root。 */
export async function buildCli(output, target = nativeTarget()) {
  const identity = await releaseIdentity();
  await fs.mkdir(output, { recursive: true });
  const payload = await fs.mkdtemp(path.join(output, '.cli-build-'));
  try {
    await stageEngine(payload, 'codegraph', target);
    const { packageDirectory } = await import('./build-language-support-packs.mjs');
    return await packageDirectory({ id: 'codegraph', version: identity.version, target }, payload, output);
  } finally { await fs.rm(payload, { recursive: true, force: true }); }
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) await buildCli(path.resolve(process.argv[2] ?? 'dist/language-support-packs'));
