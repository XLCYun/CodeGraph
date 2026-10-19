// 受信测试包的真实 Node helper；验证公共 CLI，而不依赖任何真实语言 SDK。
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { spawn } from 'node:child_process';
import readline from 'node:readline';
const root = path.dirname(fileURLToPath(import.meta.url));
const mode = fs.readFileSync(path.join(root,'mode'),'utf8');
fs.writeFileSync(path.join(root,'helper-pid'), String(process.pid));
fs.writeFileSync(path.join(root,'worker-pid'), String(process.ppid));
const protocol = 'narrant.native-analyzer/1';
/** 原子发出完整的 NDJSON 消息。 */
function output(value) { fs.writeSync(1, JSON.stringify(value)+'\n'); }
output({protocol, type:'ready', helper_id:'narrant-typescript', helper_version:mode==='mismatch'?'9.0.0':'0.1.0', projection_revision:'typescript-source-v1', languages:['typescript','javascript'],profiles:['typescript-static-v1']});
const keepAlive=setInterval(()=>{},1000);
readline.createInterface({input:process.stdin}).on('line', line => {
 const request=JSON.parse(line);
 if(request.type==='cancel') {
   if(mode==='ignore-cancel') return;
   output({protocol,type:'completed',id:request.id,outcome:'cancelled'}); process.exit(0);
 }
 fs.writeFileSync(path.join(root,'scratch'),request.environment.scratch_root);
 if(['stall','ignore-cancel'].includes(mode)) {
   const child=spawn(process.execPath,['-e', 'setInterval(()=>{},1000)'],{detached:true,stdio:'ignore'});
   fs.writeFileSync(path.join(root,'descendant-pid'), String(child.pid));
   return;
 }
 const node={category:'callable',id:'typescript:run',language:'typescript',native_kind:'function',name:'run',signature:'function run()',code:'return 1;',source:mode==='large'?'x'.repeat(4*1024*1024):'function run() { return 1; }',contains:[],references:[{semantic:'code.calls',target:{kind:'external',symbol:'typescript:console#log'}}],extensions:{'typescript.future':{nested:['allowed',true]}}};
 const graph={root_contains:['typescript:run'],nodes:[node],external_symbols:[{symbol:'typescript:console#log',name:'console.log',scope:'dependency'}]};
 if(mode==='invalid-graph') graph.root_contains=['typescript:missing'];
 const result={protocol,type:'completed',id:request.id,outcome:'ok',graph,evidence:{configurations:[{id:'one',source_paths:[],platform_ids:[]}],files:request.input.files.map(p=>({path:p,disposition:'analyzed',configuration_ids:['one']})),origins:[{node_id:'typescript:run',selected_paths:request.input.files}]}};
 output(result); clearInterval(keepAlive); process.exit(0);
});
