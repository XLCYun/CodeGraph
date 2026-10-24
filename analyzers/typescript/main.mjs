import fs from 'node:fs';
import path from 'node:path';
import { decodeRequest, validateInput, MAX_BYTES, AnalysisFailure } from './request.mjs';
import { runtimeContext } from './runtime.mjs';
import { analyze } from './analyze.mjs';

const identity = { protocol: 'codegraph.analyzer/1', engine_version: '0.1.0', pack_id: 'typescript-javascript' };

/** Windows 标准管道的 stream.write 会同步阻塞；回调式 fs.write 在 libuv 工作线程写入。 */
export async function writeWindowsPipe(fd, text) {
  const buffer = Buffer.from(text);
  let offset = 0;
  while (offset < buffer.length) {
    const written = await new Promise((resolve, reject) => {
      fs.write(fd, buffer, offset, buffer.length - offset, null, (error, bytes) => error ? reject(error) : resolve(bytes));
    });
    if (written === 0) throw new Error('标准管道写入未取得进展');
    offset += written;
  }
}

/** POSIX 保留 stream 的非阻塞背压处理；Windows 按字节补齐短写，两个平台都等待 flush。 */
function writeOutput(stream, text) {
  if (process.platform === 'win32') return writeWindowsPipe(stream.fd, text);
  return new Promise((resolve, reject) => stream.write(text, error => error ? reject(error) : resolve()));
}

/** 编译期固定语言的 CLI，只转入一次公开分析。 */
function command(args) {
  if (args.length === 1 && args[0] === '--version') return 'codegraph-typescript-javascript 0.1.0\n';
  if (args.length === 1 && args[0] === '--help' || args.length === 2 && args[0] === 'analyze' && args[1] === '--help')
    return 'Usage: codegraph-typescript-javascript analyze\nOne UTF-8 JSON request on stdin followed by EOF.\n';
  if (args.length === 1 && args[0] === 'analyze') return null;
  throw new Error('Usage: codegraph-typescript-javascript analyze');
}

/** 有界读取整个 stdin；正常 EOF 标记输入完成，不触发取消。 */
async function input() {
  const chunks = [];
  let size = 0;
  for await (const chunk of process.stdin) {
    size += chunk.length;
    if (size > MAX_BYTES) throw new AnalysisFailure('resource_limit', '请求超过 128 MiB');
    chunks.push(chunk);
  }
  try { return new TextDecoder('utf-8', { fatal: true }).decode(Buffer.concat(chunks)); }
  catch { throw new AnalysisFailure('invalid_request', '请求不是 UTF-8'); }
}

/** 主线程直接分析并输出唯一最终结果；调用方拥有取消及整棵进程树。 */
async function main() {
  process.stdout.on('error', () => {});
  process.stderr.on('error', () => {});
  try {
    const text = command(process.argv.slice(2));
    if (text !== null) { await writeOutput(process.stdout, text); return; }
  } catch (error) {
    process.exitCode = 2;
    await writeOutput(process.stderr, `${error.message}\n`).catch(() => {});
    return;
  }
  let request, result;
  try {
    request = decodeRequest(await input());
    request = validateInput(request, path.resolve(import.meta.dirname, '../..'));
    let graph = { root_contains: [], nodes: [], external_symbols: [] };
    if (request.files?.length !== 0) {
      // 这是同进程构图上下文；不编码私有消息，也不创建分析 worker。
      ({ graph } = analyze(runtimeContext(), { id: request.id, input: { repository_root: request.repository_root,
        ...(request.files === undefined ? {} : { files: request.files }), include_tests: request.options.include_tests,
        profile: 'typescript-static-v1' } }));
    }
    result = { ...identity, type: 'completed', id: request.id, outcome: 'ok', graph };
    if (Buffer.byteLength(JSON.stringify(result)) + 1 > MAX_BYTES) throw new AnalysisFailure('resource_limit', '结果超过 128 MiB');
  } catch (error) {
    process.exitCode = 1;
    result = { ...identity, type: 'completed', id: request?.id ?? null, outcome: 'error',
      error: { code: error instanceof AnalysisFailure ? error.code : 'analysis_failed', message: String(error.message).slice(0, 4096) } };
  }
  try { await writeOutput(process.stdout, `${JSON.stringify(result)}\n`); }
  catch (error) {
    process.exitCode = 1;
    await writeOutput(process.stderr, `输出失败：${error.code ?? error.message}\n`).catch(() => {});
  }
}

if (import.meta.main) await main();
