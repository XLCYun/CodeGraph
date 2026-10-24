import { execFileSync } from 'node:child_process';
import { releaseRepository, targets } from '../language-packs/distribution.mjs';
import { fileURLToPath } from 'node:url';
import path from 'node:path';

/** GitHub CLI 访问固定仓库 API；JSON 参数不经 shell 展开，执行器可在进程边界替换。 */
export function githubJson(endpoint, body, run = execFileSync) {
  const args = ['api', endpoint];
  if (body) args.push('--method', 'POST', '--input', '-');
  return JSON.parse(run('gh', args, { encoding: 'utf8', input: body ? JSON.stringify(body) : undefined, maxBuffer: 8 * 1024 * 1024 }));
}

/** 只接受 main push 的完整成功 CI，不能用 PR merge SHA、失败上传或手动子集冒充。 */
export async function verifyEvidenceRun(runId, sourceCommit, api = githubJson) {
  if (!/^[1-9]\d*$/.test(String(runId)) || !/^[0-9a-f]{40}$/.test(sourceCommit)) throw new Error('Invalid evidence run or full source commit');
  const run = await api(`/repos/${releaseRepository}/actions/runs/${runId}`);
  if (run.repository?.full_name !== releaseRepository || run.head_repository?.full_name !== releaseRepository || run.path !== '.github/workflows/ci.yml' || run.event !== 'push' || run.head_branch !== 'main' || run.head_sha !== sourceCommit || run.status !== 'completed' || run.conclusion !== 'success') throw new Error('Evidence must be successful complete main CI at the exact source commit');
  const jobs = await api(`/repos/${releaseRepository}/actions/runs/${runId}/attempts/${run.run_attempt}/jobs?per_page=100`);
  if (jobs.total_count !== jobs.jobs?.length || jobs.jobs.some(job => job.status !== 'completed' || job.conclusion !== 'success')) throw new Error('Evidence contains incomplete, skipped or failed jobs');
  for (const name of ['Complete CI', ...targets.map(target => `Native acceptance (${target})`)]) if (jobs.jobs.filter(job => job.name === name).length !== 1) throw new Error(`Missing complete CI job: ${name}`);
  return run;
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) await verifyEvidenceRun(process.argv[2], process.argv[3]);
