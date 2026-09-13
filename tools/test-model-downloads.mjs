// Command/API-only local transfer tests. Fake Ollama streams, no model weights.
import assert from 'node:assert/strict';
import {mkdtemp,readFile,writeFile,readdir} from 'node:fs/promises';
import {resolve,join} from 'node:path';
import {spawn} from 'node:child_process';
import {createServer} from 'node:http';
import {randomUUID} from 'node:crypto';
const root=resolve(import.meta.dirname,'..');
const scratch=await mkdtemp(join(root,'.validation/model-downloads-'));
const bin=resolve(process.env.EEF_TEST_BINARY_DIR||join(root,'target/debug'));
const configPath=join(scratch,'node.json'),marker=configPath+'.api.json',children=[],pulls=[],streams=new Set();
const env={...process.env,EEF_NODE_PSK:'',APPDATA:join(scratch,'appdata'),EEF_DISCOVERY_DIR:join(scratch,'discovery'),PATH:join(root,'.tooling/llvm-mingw-20260616-ucrt-x86_64/bin')+';'+process.env.PATH};
function launch(args){const child=spawn(join(bin,'eefn.exe'),['--config',configPath,...args],{cwd:root,windowsHide:true,env,stdio:['ignore','pipe','pipe']});let out='',err='';child.stdout.on('data',b=>out=(out+b).slice(-4194304));child.stderr.on('data',b=>err=(err+b).slice(-1048576));child.result=new Promise((resolve,reject)=>{child.on('error',reject);child.on('close',code=>resolve({code,out,err}));});children.push(child);return child;}
async function cli(args,success=true){const child=launch(['models',...args,'--json']);const timer=setTimeout(()=>child.kill(),22000);try{const r=await child.result;assert.equal(r.code,success?0:1,r.out+r.err);return JSON.parse(r.out);}finally{clearTimeout(timer);}}
async function json(url,body,method){const r=await fetch(url,{method:method||(body?'POST':'GET'),headers:{'Content-Type':'application/json'},body:body?JSON.stringify(body):undefined,signal:AbortSignal.timeout(10000)});const value=await r.json();assert(r.ok,JSON.stringify(value));return value;}
async function until(fn,label){const end=Date.now()+45000;while(Date.now()<end){try{if(await fn())return;}catch{}await new Promise(r=>setTimeout(r,100));}throw Error('Timeout: '+label);}
async function listen(server){await new Promise(r=>server.listen(0,'127.0.0.1',r));return server.address().port;}
let tagRequests=0;
const fake=createServer(async(req,res)=>{
 res.setHeader('Content-Type','application/json');
 if(req.url==='/api/tags'){tagRequests++;return res.end(JSON.stringify({models:[{name:'fixture'}]}));}
 if(req.url==='/api/show')return res.end(JSON.stringify({capabilities:['completion']}));
 if(req.url==='/api/pull'){
  let body='';for await(const part of req)body+=part;
  const value=JSON.parse(body);pulls.push(value.model);
  if(value.model==='complete-fixture')return res.end('{"status":"success"}\n');
  res.write('{"status":"downloading","digest":"fixture","total":100,"completed":1}\n');
  streams.add(res);res.on('close',()=>streams.delete(res));return;
 }
 res.writeHead(404);res.end('{}');
});
let node='',dropCount=0,legacyMode=false,legacyRequests=0;
const proxy=createServer(async(req,res)=>{
 try {
  if(legacyMode){legacyRequests++;res.writeHead(404,{'Content-Type':'application/json'});res.end('{}');return;}
  let body='';for await(const b of req)body+=b;
  const payload=JSON.parse(body);
  const upstream=await fetch(node+req.url,{method:'POST',headers:{'Content-Type':'application/json'},body});
  const text=await upstream.text();
  if(payload.command?.operation==='install'){dropCount++;res.destroy();return;}
  res.writeHead(upstream.status,{'Content-Type':'application/json'});res.end(text);
 } catch {res.destroy();}
});
try {
 const fakePort=await listen(fake),proxyPort=await listen(proxy);
 const config=JSON.parse(await readFile(join(root,'config/node.example.json'),'utf8'));
 Object.assign(config,{node_id:'download-fixture-node',name:'Download fixture',auto_local:false,local_pairing:false,endpoints:[],dashboard:{enabled:true,ui_enabled:false,host:'127.0.0.1',port:0},update:{policy:'off'},models:{provider:'ollama',ollama:{base_url:`http://127.0.0.1:${fakePort}`,selected:[]},llamacpp:{slots:[]}}});
 await writeFile(configPath,JSON.stringify(config));
 const offline=await cli(['install','--model','fixture'],false);assert.equal(offline.error_code,'node_not_running');assert.equal(offline.request_sent,false);assert.equal(pulls.length,0);
 launch(['--no-ui']);
 await until(async()=>{const m=JSON.parse(await readFile(marker,'utf8'));node='http://'+m.address;return !!(await json(node+'/api/diagnostics')).runtime_id;},'command API discovery');
 assert.equal((await fetch(node+'/')).status,404);
 const before=(await json(node+'/api/config')).config;
 const untouchedDownload=(await json(node+'/api/status')).download;
 const plan=await cli(['install-plan','--model','gated-fixture']);
 assert.equal(plan.plan.backend,'ollama');assert.equal(plan.plan.can_request,true);
 assert.equal(plan.plan.artifact,null);assert.equal(plan.plan.download_bytes_if_needed,null);
 assert.equal(plan.plan.disk_check,'provider_managed_unknown');assert.equal(plan.plan.download_started,false);
 assert.equal(plan.plan.license_reviewed,false);assert.equal(pulls.length,0);
 assert.deepEqual((await json(node+'/api/status')).download,untouchedDownload,'preflight must not reset transfer status');
 assert.equal((await cli(['show'])).saved_selections.length,0,'old selection commands retained');
 const inventory=await cli(['installed']);assert.equal(inventory.inventory.backend,'ollama');assert.equal(inventory.inventory.storage.free_bytes,null);
 assert.deepEqual((await cli(['inspect','--model','fixture'])).model.capabilities,['llm.infer']);
 const accepted=await cli(['install','--model','gated-fixture']);
 assert.equal(accepted.accepted,true);assert.equal(accepted.completed,false);assert.equal(accepted.selection_changed,false);
 const id=accepted.operation_id;assert.match(id,/^[0-9a-f-]{36}$/);
 await until(async()=>(await cli(['download-status','--id',id])).download.completed===1,'progress through independent command');
 const busy=await cli(['install-plan','--model','gated-fixture']);
 assert.equal(busy.plan.can_request,false);assert.equal(busy.plan.blocker,'download_in_progress');
 const typed=command=>json(node+'/api/commands/model-downloads',{schema_version:1,expected_node_id:config.node_id,command});
 const duplicate=await typed({operation:'install',model:'gated-fixture',operation_id:id});assert.equal(duplicate.success,false);assert.equal(pulls.length,1);
 const wrong=randomUUID();assert.equal((await cli(['cancel-download','--id',wrong],false)).success,false);
 assert.equal((await cli(['download-status','--id',id])).download.cancel_requested,false);
 const canceled=await cli(['cancel-download','--id',id]);assert.equal(canceled.cancel_requested,true);assert.equal(canceled.completed,false);
 await until(async()=>(await cli(['download-status','--id',id])).download.state==='cancelled','cancel acknowledgement is separate from terminal state');
 // Lose exactly one admitted install reply, then use its pre-send receipt.
 const originalMarker=await readFile(marker,'utf8');
 try {
  const altered=JSON.parse(originalMarker);altered.address=`127.0.0.1:${proxyPort}`;await writeFile(marker,JSON.stringify(altered));
  const lost=await cli(['install','--model','lost-reply-fixture'],false);
  assert.equal(lost.error_code,'outcome_unknown');assert.equal(lost.automatically_retried,false);assert.equal(dropCount,1);
  await writeFile(marker,originalMarker);
  await until(()=>pulls.length===2,'exactly one second transfer admitted');
  const recovered=await cli(['download-status','--id',lost.operation_id]);assert.equal(recovered.known,true);assert.equal(recovered.download.model_id,'lost-reply-fixture');
  assert.equal((await cli(['download-status','--id',id],false)).error_code,'operation_not_found');
  assert.equal((await cli(['cancel-download','--id',id],false)).success,false,'stale receipt cannot cancel newer transfer');
  await cli(['cancel-download','--id',lost.operation_id]);
  await until(async()=>(await cli(['download-status','--id',lost.operation_id])).download.state==='cancelled','lost-reply transfer cancels');
 } finally {await writeFile(marker,originalMarker);}
 const complete=await cli(['install','--model','complete-fixture']);
 await until(async()=>(await cli(['download-status','--id',complete.operation_id])).download.state==='installed','provider confirms completion');
 assert.deepEqual((await json(node+'/api/config')).config,before,'downloads do not alter selections, permissions or startup');
 const blocked=await fetch(node+'/api/commands/model-downloads',{method:'POST',headers:{Origin:'https://untrusted.example','Content-Type':'application/json'},body:JSON.stringify({schema_version:1,expected_node_id:config.node_id,command:{operation:'cancel',operation_id:complete.operation_id}})});assert.equal(blocked.status,403);
 assert.equal(pulls.length,3);
 try {
  legacyMode=true;const altered=JSON.parse(originalMarker);altered.address=`127.0.0.1:${proxyPort}`;await writeFile(marker,JSON.stringify(altered));
  assert.equal((await cli(['install','--model','unsupported-fixture'],false)).error_code,'unsupported_command');
  assert.equal(legacyRequests,1);assert.equal(pulls.length,3,'no legacy fallback installation');
 } finally {await writeFile(marker,originalMarker);}
 // GGUF inspection uses saved configuration, without contacting the artifact or
 // the unselected provider. An earlier plan does not skip admission validation.
 const ggufConfig=structuredClone(before);ggufConfig.dashboard.port=Number(new URL(node).port);
 ggufConfig.models.provider='llamacpp';
 ggufConfig.model_catalog=[{id:'gguf-fixture',url:'https://example.invalid/never-fetched',sha256:'c'.repeat(64),bytes:1000,license:'fixture metadata'}];
 await json(node+'/api/config',{config:ggufConfig},'PUT');
 const filesBefore=(await readdir(scratch)).sort(),tagsBefore=tagRequests;
 const ggufPlan=(await cli(['install-plan','--model','gguf-fixture'])).plan;
 assert.equal(ggufPlan.can_request,true);assert.equal(ggufPlan.artifact.schema_version,1);
 assert.equal(ggufPlan.artifact.bytes,1000);assert.equal(ggufPlan.storage.scope,'node');
 assert.equal(ggufPlan.cache_digest_verified,false);assert.equal(ggufPlan.runtime_compatibility_verified,false);
 assert.equal(tagRequests,tagsBefore,'explicit GGUF must not inspect Ollama');
 assert.deepEqual((await readdir(scratch)).sort(),filesBefore,'preflight must not create cache files or directories');
 ggufConfig.model_catalog[0].sha256='invalid';
 await json(node+'/api/config',{config:ggufConfig},'PUT');
 assert.equal((await cli(['install','--model','gguf-fixture'],false)).error_code,'download_command_rejected');
 assert.equal((await cli(['download-status','--id',complete.operation_id])).download.state,'installed','invalid admission preserves retained receipt');
 assert.equal(pulls.length,3);assert.equal(tagRequests,tagsBefore);
 await writeFile(join(scratch,'results.json'),JSON.stringify({passed:true,api_only:true,offline_no_launch:true,installed_and_inspect:true,read_only_install_preflight:true,gguf_preflight_no_provider_query:true,invalid_admission_preserves_receipt:true,provider_storage_unknown:true,busy_preflight:true,progress_and_exact_cancel:true,lost_reply_receipt:true,no_automatic_retry:true,no_legacy_fallback:true,no_selection_changes:true,physical_two_pc:false,real_model_downloads:false},null,2));
 console.log('Model download command checks passed: '+scratch);
} finally {
 for(const stream of streams)stream.destroy();
 for(const child of children)if(child.exitCode===null)child.kill();
 await Promise.allSettled(children.map(c=>c.result));
 for(const server of [fake,proxy]){server.closeAllConnections();await new Promise(r=>server.close(r));}
}
