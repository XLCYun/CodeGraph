import fs from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { isDeepStrictEqual } from 'node:util';
import { assetNames, nativeTarget, packIds, releaseIdentity, sha256, targets } from '../language-packs/distribution.mjs';
import { verifyDistribution } from '../language-packs/verify-distribution.mjs';
import { recordVerifiedCommands, verificationCommandIdentities } from '../language-packs/verification-commands.mjs';

/** 对一平台全部八种产物做完整字节验证，并保证语言包使用同一 CLI。 */
async function readTarget(artifacts, identity, target, requireClean) {
  const entries = [], receipts = [];
  let cliHash;
  for (const id of ['codegraph', ...packIds]) {
    const directory = path.join(artifacts, id, identity.version, target);
    const { manifest, body, archive } = await verifyDistribution(directory, { id, target, version: identity.version, engineVersion: identity.version, protocol: identity.protocol, sourceCommit: identity.sourceCommit, ...(requireClean ? { sourceDirty: false } : {}) });
    const cli = manifest.files.find(file => file.path === `bin/codegraph${target.includes('windows') ? '.exe' : ''}`);
    if (!cli?.executable) throw new Error('Package missing executable CLI');
    if (id === 'codegraph') cliHash = cli.sha256;
    if (cli.sha256 !== cliHash) throw new Error('Language package CLI differs from standalone CLI');
    const [archiveName, manifestName] = assetNames(id, target);
    entries.push({ name: archiveName, file: archive, size: manifest.archiveSize, sha256: manifest.archiveSha256 });
    if (manifestName) entries.push({ name: manifestName, file: path.join(directory, 'manifest.json'), size: body.length, sha256: sha256(body) });
    receipts.push({ id, manifestSha256: sha256(body), archiveSha256: manifest.archiveSha256, archiveSize: manifest.archiveSize });
  }
  return { entries, receipts };
}

/** 只有所有串行验收成功且源码干净时才能写可发布证据；focused 验收不会调用此入口。 */
export async function writeVerificationEvidence(artifacts, commands, environment) {
  const identity = await releaseIdentity(), target = nativeTarget();
  if (identity.sourceDirty) throw new Error('Cannot attest a dirty source tree; development tests are not release evidence');
  const recordedCommands = recordVerifiedCommands(commands, environment);
  const { receipts } = await readTarget(artifacts, identity, target, true);
  const semanticTests = commands.flatMap(command => command.requiredTests ?? []).sort();
  const expected = JSON.parse(await fs.readFile(new URL('../../resources/semantic-acceptance.json', import.meta.url))).filter(test => test.platforms.includes(process.platform)).map(test => test.name).sort();
  if (JSON.stringify(semanticTests) !== JSON.stringify(expected)) throw new Error('Incomplete semantic test evidence');
  const evidence = { schemaVersion: 1, status: 'passed', ...identity, target, languages: packIds, semanticTests, commands: recordedCommands, artifacts: receipts };
  await fs.writeFile(path.join(artifacts, `verification-${target}.json`), `${JSON.stringify(evidence, null, 2)}\n`, { flag: 'wx' });
}

/** 精确核对成功状态、全部测试、平台身份和归档 receipt，缺项即关闭发布。 */
export function validateVerificationEvidence(evidence, identity, target, receipts, inventory) {
  const platform = target.includes('windows') ? 'win32' : 'darwin';
  const expectedTests = inventory.filter(test => test.platforms.includes(platform)).map(test => test.name).sort();
  const expectedCommands = verificationCommandIdentities(platform).map(command => ({ ...command, outcome: 'passed' }));
  if (evidence.schemaVersion !== 1 || evidence.status !== 'passed' || evidence.sourceCommit !== identity.sourceCommit || evidence.sourceDirty !== false || evidence.version !== identity.version || evidence.engineVersion !== identity.engineVersion || evidence.protocol !== identity.protocol || evidence.target !== target || JSON.stringify(evidence.languages) !== JSON.stringify(packIds) || JSON.stringify(evidence.semanticTests) !== JSON.stringify(expectedTests) || !isDeepStrictEqual(evidence.commands, expectedCommands) || JSON.stringify(evidence.artifacts) !== JSON.stringify(receipts)) throw new Error(`Missing complete successful verification evidence: ${target}`);
  return evidence;
}

/** 发布必须有双平台完整成功证据，且所有 manifest/归档均属于指定精确提交。 */
export async function readCompleteArtifacts(artifacts, sourceCommit) {
  const identity = await releaseIdentity();
  if (identity.sourceDirty || identity.sourceCommit !== sourceCommit || !/^[0-9a-f]{40}$/.test(sourceCommit)) throw new Error('Release requires a clean checkout of the exact source commit');
  const assets = [];
  const inventory = JSON.parse(await fs.readFile(new URL('../../resources/semantic-acceptance.json', import.meta.url)));
  for (const target of targets) {
    const { entries, receipts } = await readTarget(artifacts, identity, target, true);
    const evidence = JSON.parse(await fs.readFile(path.join(artifacts, `verification-${target}.json`), 'utf8'));
    validateVerificationEvidence(evidence, identity, target, receipts, inventory);
    assets.push(...entries);
  }
  return { ...identity, tag: `v${identity.version}`, assets: assets.sort((a, b) => a.name.localeCompare(b.name, 'en')) };
}

/** 把已验证字节复制为最终公开资产名，并生成覆盖全部三十个资产的 SHA256SUMS。 */
export async function prepareRelease(artifacts, sourceCommit, output) {
  const bundle = await readCompleteArtifacts(artifacts, sourceCommit);
  await fs.mkdir(output, { recursive: true });
  if ((await fs.readdir(output)).length) throw new Error('Release output must be empty');
  for (const asset of bundle.assets) { const destination = path.join(output, asset.name); await fs.copyFile(asset.file, destination); asset.file = destination; }
  const sums = Buffer.from(bundle.assets.map(asset => `${asset.sha256}  ${asset.name}\n`).join(''));
  const file = path.join(output, 'SHA256SUMS');
  await fs.writeFile(file, sums, { flag: 'wx' });
  bundle.assets.push({ name: 'SHA256SUMS', file, size: sums.length, sha256: sha256(sums) });
  return bundle;
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const [artifacts, sourceCommit, output] = process.argv.slice(2);
  if (!artifacts || !sourceCommit || !output) throw new Error('Usage: release-artifacts.mjs <artifacts> <full-source-commit> <empty-output>');
  const bundle = await prepareRelease(path.resolve(artifacts), sourceCommit, path.resolve(output));
  console.log(`Prepared ${bundle.tag}: ${bundle.assets.length} verified assets from ${bundle.sourceCommit}; nothing published`);
}
