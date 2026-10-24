// Isolated one-PC protocol test with a fake Ollama HTTP backend. No real inference,
// model downloads, personal configuration, or physical cross-node proof.
import assert from 'node:assert/strict';
import {mkdtemp,readFile,writeFile,mkdir,realpath} from 'node:fs/promises';
import {resolve,join,toNamespacedPath} from 'node:path';
import {spawn} from 'node:child_process';
import {createServer as tcpServer} from 'node:net';
import {createServer} from 'node:http';
import {randomBytes,randomUUID,createHash} from 'node:crypto';
import {reviewedFixture} from './reviewed-fixture.mjs';
const root=resolve(import.meta.dirname,'..');
const scratch=await mkdtemp(join(root,'.validation','model-metadata-'));
const bin=resolve(process.env.EEF_TEST_BINARY_DIR||join(root,'target/debug'));
const children=[],requests=[],gates=new Map();
let tagGate=null,pullRequests=0;
// Optional compatibility run: selected old executable, still isolated config/data.
const legacyNode=process.env.EEF_TEST_LEGACY_NODE_BINARY;
const uiConfig=process.env.EEF_TEST_UI_CONFIG==='1';
const env={...process.env,EEF_NODE_PSK:'',APPDATA:join(scratch,'appdata'),EEF_DISCOVERY_DIR:join(scratch,'discovery'),PATH:join(root,'.tooling/llvm-mingw-20260616-ucrt-x86_64/bin')+';'+process.env.PATH};
function launch(name,args,extra={}) {
 const executable=name==='eefn'&&legacyNode?resolve(legacyNode):join(bin,name+'.exe');
 const child=spawn(executable,args,{cwd:root,windowsHide:true,env:{...env,...extra},stdio:['ignore','pipe','pipe']});
 let out='',err='';child.stdout.on('data',b=>{out=(out+b).slice(-1024*1024);});child.stderr.on('data',b=>{err=(err+b).slice(-1024*1024);});
 child.logs=()=>({out,err});
 child.result=new Promise((resolve,reject)=>{child.on('error',reject);child.on('close',code=>resolve({code,out,err}));});children.push(child);return child;
}
async function port(){const s=tcpServer();await new Promise(r=>s.listen(0,'127.0.0.1',r));const p=s.address().port;await new Promise(r=>s.close(r));return p;}
async function until(fn,label){const end=Date.now()+65000;while(Date.now()<end){try{if(await fn())return;}catch{}await new Promise(r=>setTimeout(r,200));}throw Error('Timeout: '+label);}
async function json(url,body,method){const r=await fetch(url,{method:method||(body?'POST':'GET'),headers:{'Content-Type':'application/json'},body:body?JSON.stringify(body):undefined,signal:AbortSignal.timeout(10000)});const value=await r.json();assert(r.ok,JSON.stringify(value));return value;}
const fake=createServer(async(req,res)=>{
 res.setHeader('Content-Type','application/json');
 if(req.url==='/api/tags'){
  if(tagGate){const gate=tagGate;gate.entered=true;await new Promise(resolve=>gate.release=resolve);}
  return res.end(JSON.stringify({models:[{name:'fixture-text'},{name:'fixture-vision'}]}));
 }
 if(req.url==='/api/pull'){pullRequests++;res.writeHead(409);return res.end(JSON.stringify({error:'test fixture never downloads models'}));}
 if(req.url==='/api/show')return res.end(JSON.stringify({capabilities:['completion','vision']}));
 if(req.url==='/api/chat'){
  let body='';for await(const chunk of req){body+=chunk;if(body.length>65536){res.writeHead(413);return res.end('{}');}}
  const payload=JSON.parse(body);requests.push(payload);
  const prompt=payload.messages?.at(-1)?.content;
  if(typeof prompt==='string'&&prompt.startsWith('gated-job-'))await new Promise(resolve=>gates.set(prompt,resolve));
  if(prompt==='response-fixture-missing')return res.end('{}');
  if(prompt==='response-fixture-length')return res.end(JSON.stringify({message:{content:'partial fixture'},done:true,done_reason:'length'}));
  if(prompt==='response-fixture-unknown')return res.end(JSON.stringify({message:{content:'older fixture'}}));
  if(prompt==='response-fixture-oversize'){const body='x'.repeat(4*1024*1024+1);res.setHeader('Content-Length',Buffer.byteLength(body));return res.end(body);}
  if(prompt==='response-fixture-chunked'){for(let n=0;n<65;n++)res.write('x'.repeat(65536));return res.end();}
  if(prompt==='response-fixture-stalled'){res.write('{"message":');await new Promise(r=>setTimeout(r,2000));return res.end('{} }');}
  return res.end(JSON.stringify({message:{content:'fixture response; not real inference'},done:true,done_reason:'stop'}));
 }
 res.writeHead(404);res.end('{}');
});
try{
 await new Promise(r=>fake.listen(0,'127.0.0.1',r));
 const [eefPort,nodePort,apiPort]=await Promise.all([port(),port(),port()]);
 const eef=`http://127.0.0.1:${eefPort}`;let node=`http://127.0.0.1:${apiPort}`;
 const secret=randomBytes(24).toString('hex'),id='metadata-fixture-node';
 const config=JSON.parse(await readFile(join(root,'config/node.example.json'),'utf8'));
 Object.assign(config,{node_id:id,name:'Metadata fixture',psk:secret,auto_local:false,local_pairing:false,endpoints:[`127.0.0.1:${nodePort}`],heartbeat_seconds:1,dashboard:{enabled:true,host:'127.0.0.1',port:apiPort},update:{policy:'off'},models:{provider:'ollama',ollama:{base_url:`http://127.0.0.1:${fake.address().port}`,selected:[{model_id:'fixture-text',modality:'text'},{model_id:'fixture-vision',modality:'vlm'}]},llamacpp:{slots:[]}}});
 const nodeConfig=join(scratch,'node.json'),eefConfig=join(scratch,'eef.yaml');
 config.request_limits={work:1,control:2};
 if(uiConfig)config.dashboard.ui_enabled=false;
 const nodeCommand=async(args,success=true)=>{const child=launch('eefn',['--config',nodeConfig,'models',...args,'--json']);const timer=setTimeout(()=>child.kill(),20000);try{const result=await child.result;assert.equal(result.code,success?0:1,result.err);return JSON.parse(result.out);}finally{clearTimeout(timer);}};
 const remoteModels=async(args,success=true)=>{const child=launch('eef',['--config',eefConfig,'node','models','--node',id,...args,'--json']);const timer=setTimeout(()=>child.kill(),25000);try{const result=await child.result;assert.equal(result.code,success?0:1,result.out+' '+result.err);return JSON.parse(result.out);}finally{clearTimeout(timer);}};
 const nodeJobs=async(args,success=true)=>{const child=launch('eefn',['--config',nodeConfig,'jobs',...args,'--json']);const timer=setTimeout(()=>child.kill(),25000);try{const result=await child.result;assert.equal(result.code,success?0:1,result.out+' '+result.err);return JSON.parse(result.out);}finally{clearTimeout(timer);}};
 const nodeRequests=async(args,success=true)=>{const child=launch('eefn',['--config',nodeConfig,'requests',...args,'--json']);const timer=setTimeout(()=>child.kill(),25000);try{const result=await child.result;assert.equal(result.code,success?0:1,result.out+' '+result.err);return JSON.parse(result.out);}finally{clearTimeout(timer);}};
 const nodeConnection=async(action,extra=[],success=true)=>{const child=launch('eefn',['--config',nodeConfig,'connection',action,...extra,'--json']);const timer=setTimeout(()=>child.kill(),25000);try{const result=await child.result;assert.equal(result.code,success?0:1,result.out+' '+result.err);return JSON.parse(result.out);}finally{clearTimeout(timer);}};
 const localRestart=async()=>{const child=launch('eefn',['--config',nodeConfig,'restart','--wait-seconds','60','--json']);const timer=setTimeout(()=>child.kill(),70000);try{const result=await child.result;assert.equal(result.code,0,result.out+' '+result.err);const reply=JSON.parse(result.out);assert.equal(reply.completed,true);assert.notEqual(reply.runtime_id,reply.previous_runtime_id);return reply;}finally{clearTimeout(timer);}};
 await writeFile(nodeConfig,JSON.stringify(config));
 let yaml=(await readFile(join(root,'config/default_identity.yaml'),'utf8')).replace('port: 51334',`port: ${eefPort}`).replace('port: 51335',`port: ${nodePort}`).replace('policy: prompt','policy: off');
 if(uiConfig)yaml=yaml.replace('ui_enabled: true','ui_enabled: false');
 await writeFile(eefConfig,yaml);
 launch('eef',['--config',eefConfig,'--database',join(scratch,'eef.db'),'--no-brain',...uiConfig?[]:['--no-ui']],{EEF_NODE_PSK:secret});
 launch('eefn',['--config',nodeConfig,...uiConfig?[]:['--no-ui']]);
 const inventory=()=>json(eef+'/api/commands/models?node_id='+id);
 await until(async()=>(await inventory()).nodes[0]?.models.length===2,'versioned registration');
 for(const base of [eef,node])for(const path of ['/','/app.js','/app.css','/advanced/legacy'])assert.equal((await fetch(base+path)).status,404,'API-only process must not serve browser assets');
 if(!legacyNode)assert.equal((await fetch(node+'/api/pick',{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify({kind:'model'})})).status,404,'API-only mode must not open native UI');
 if(uiConfig&&!legacyNode){
  const initialRuntime=(await json(node+'/api/diagnostics')).runtime_id;
  const visible=(await json(node+'/api/config')).config;visible.dashboard.ui_enabled=true;
  await json(node+'/api/config',{config:visible},'PUT');
  const shown=await localRestart();assert.equal(shown.previous_runtime_id,initialRuntime);
  await until(async()=>(await fetch(node+'/')).status===200,'enable browser without rebinding command port');
  const hidden=(await json(node+'/api/config')).config;hidden.dashboard.ui_enabled=false;
  await json(node+'/api/config',{config:hidden},'PUT');await localRestart();
  assert.equal((await fetch(node+'/')).status,404);assert.equal((await nodeConnection('show')).success,true);
  await until(async()=>(await inventory()).nodes[0]?.models.length===2,'reconnect with browser disabled');
 }
 const models=(await inventory()).nodes[0].models;
 assert(models.every(m=>m.metadata_source===(legacyNode?'legacy_registration':'model_metadata_v1')&&m.lifecycle===null&&m.resource_estimates.ram_mb===null));
 assert.deepEqual(models.find(m=>m.instance.model_id==='fixture-vision').capabilities,['llm.infer','vlm.analyze']);
 assert.equal((await json(eef+'/api/commands/models?capability=vlm.analyze')).nodes[0].models.length,1);
 assert.equal((await fetch(eef+'/api/commands/models',{headers:{Origin:'https://untrusted.example'}})).status,403);
 const owner=launch('eef',['--config',eefConfig,'models','list','--json']);
 const timer=setTimeout(()=>owner.kill(),20000);const cli=await owner.result;clearTimeout(timer);
 assert.equal(cli.code,0,cli.err);assert.equal(JSON.parse(cli.out).nodes[0].models.length,2);
 const ordinaryDenied=async capability=>{
  const response=await fetch(eef+`/api/node/${id}/invoke`,{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify({capability,action:'run',params:{model:'fixture-text',backend:'ollama',prompt:'must not run'},timeout:10})});
  assert.equal(response.status,400);const value=await response.json();
  assert.equal(value.error_code,'execution_grant_required');assert.equal(value.not_started,true);
 };
 await ordinaryDenied('llm.infer');assert.equal(requests.length,0,'legacy RPC cannot execute a workload');
 let reviewed;
 if(!legacyNode){
  const coordinator=async args=>{const c=launch('eef',['--config',eefConfig,'--json',...args]),timer=setTimeout(()=>c.kill(),30000);try{const r=await c.result;assert.equal(r.code,0,r.out+r.err);return JSON.parse(r.out);}finally{clearTimeout(timer);}};
  const privateFile=join(scratch,'PRIVATE-pair.json');
  await writeFile(privateFile,JSON.stringify(await coordinator(['credentials','provision','--node',id,'--address',`127.0.0.1:${nodePort}`])));
  await coordinator(['credentials','legacy','--allow','false']);
  const old=(await json(eef+'/api/diagnostics')).runtime_id;await coordinator(['restart']);
  await until(async()=>(await json(eef+'/api/diagnostics')).runtime_id!==old,'coordinator TLS restart');
  const pairing=launch('eefn',['--config',nodeConfig,'connection','pair','--code-file',privateFile,'--json']);
  const paired=await pairing.result;assert.equal(paired.code,0,paired.out+paired.err);
  await until(async()=>(await json(node+'/api/status')).execution_grants.available&&(await inventory()).nodes[0]?.models.length===2,'pinned grant session');
  reviewed=reviewedFixture(eef,node,id);
 }
 const infer=async(prompt='protocol fixture',limits)=>{
  const plan=await reviewed.save({template:'generate_text',params:{model_id:'fixture-text',prompt},constraints:{backend:'ollama',node_id:id}},prompt);
  await reviewed.start(await reviewed.approve(plan,limits));const report=await reviewed.result(plan.id);
  await reviewed.cleanup(plan.id);return report;
 };
 const rejectedSelection=async args=>{
  const count=requests.length;
  const reply=await nodeJobs(['generate-text','--prompt','must not execute',...args],false);
  assert.equal(reply.execution_authorized,false);assert.equal(reply.dispatched,false);
  const found=await nodeJobs(['find','--operation-id',reply.operation_id]);
  assert.equal(found.data.total,1);const plan=await reviewed.inspect(found.data.jobs[0].id);
  assert.equal(plan.status,'review');assert.equal(plan.execution,undefined);
  assert.equal(requests.length,count,'failed compilation must not fall back');
  await reviewed.cleanup(plan.plan_id);
 };
 if(!legacyNode){
  const reply=await infer();assert.equal(reply.success,true);assert.equal(reply.state,'completed');assert.equal(requests.length,1);
  assert.equal(reply.data.completion_complete,true);assert.equal(reply.data.finish_reason,'stop');
  await rejectedSelection(['--backend','llamacpp','--model','fixture-text']);
 }
 const status=await json(eef+'/api/status');assert.equal(status.models.length,2);assert(status.models.every(m=>m.model_metadata.schema_version===1));
 if(!legacyNode){
  const ownerConfig=(await json(node+'/api/config')).config;
  ownerConfig.management={allow_remote:false};await json(node+'/api/config',{config:ownerConfig},'PUT');
  const remoteShow=await remoteModels(['show']);
  assert.equal(remoteShow.data.saved_selections.length,2);assert.equal(remoteShow.data.remote_management_allowed,false);
  assert.equal(remoteShow.mutation_requested,false);assert(!JSON.stringify(remoteShow).includes(secret));
  const deniedBefore=await readFile(nodeConfig,'utf8');
  const remoteDenied=await remoteModels(['hints','--backend','ollama','--model','fixture-text','--role','node_tool'],false);
  assert.equal(remoteDenied.error_code,'approval_required');assert.equal(remoteDenied.mutation_requested,false);
  assert.equal(await readFile(nodeConfig,'utf8'),deniedBefore);
  const deniedInterpreter=await remoteModels(['interpreter','--backend','ollama','--model','fixture-text','--activate-backend'],false);
  assert.equal(deniedInterpreter.error_code,'approval_required');assert.equal(await readFile(nodeConfig,'utf8'),deniedBefore);
  const deniedClear=await remoteModels(['clear-interpreter'],false);
  assert.equal(deniedClear.error_code,'approval_required');assert.equal(await readFile(nodeConfig,'utf8'),deniedBefore);
  const deniedCache=await remoteModels(['select-installed-gguf','--model','not-installed'],false);
  assert.equal(deniedCache.error_code,'approval_required');assert.equal(deniedCache.mutation_requested,false);
  assert.equal(await readFile(nodeConfig,'utf8'),deniedBefore);
  const approved=(await json(node+'/api/config')).config;approved.management.allow_remote=true;
  await json(node+'/api/config',{config:approved},'PUT');
  const remoteSaved=await remoteModels(['hints','--backend','ollama','--model','fixture-text','--role','node_tool']);
  assert.equal(remoteSaved.acknowledged,true);assert.equal(remoteSaved.data.changed,true);assert.equal(remoteSaved.data.restart_required,true);
  assert.deepEqual(remoteSaved.data.saved_selections.find(m=>m.model_id==='fixture-text').model_metadata.roles,['node_tool']);
  assert.equal(remoteSaved.data.registered_models.find(m=>m.model_id==='fixture-text').model_metadata.roles,null,'saved must not pretend to be applied');
  assert.equal((await remoteModels(['hints','--backend','ollama','--model','fixture-text','--role','node_tool'])).data.changed,false);
  await remoteModels(['provider','auto']);
  const roleMoved=await remoteModels(['interpreter','--backend','ollama','--model','fixture-text','--activate-backend']);
  assert.equal(roleMoved.data.provider,'ollama');
  assert.equal(roleMoved.acknowledged,true);assert.equal(roleMoved.data.changed,true);
  assert.deepEqual(roleMoved.data.saved_selections.find(m=>m.model_id==='fixture-text').model_metadata.roles,['node_tool','request_interpreter']);
  assert.equal(roleMoved.data.registered_models.find(m=>m.model_id==='fixture-text').model_metadata.roles,null,'role assignment is saved, not applied');
  assert.equal((await remoteModels(['interpreter','--backend','ollama','--model','fixture-text'])).data.changed,false);
  const beforeMissing=await readFile(nodeConfig,'utf8');
  await nodeCommand(['interpreter','--backend','ollama','--model','missing'],false);assert.equal(await readFile(nodeConfig,'utf8'),beforeMissing);
  const cleared=await remoteModels(['clear-interpreter']);
  assert.equal(cleared.data.changed,true);assert.equal(cleared.data.saved_selections.length,2);
  assert.deepEqual(cleared.data.saved_selections.find(m=>m.model_id==='fixture-text').model_metadata.roles,['node_tool']);
  assert.equal((await remoteModels(['clear-interpreter'])).data.changed,false);
  // Every owner command is a new explicit operation, never a busy-response
  // retry. Stress sequential replies at the unchanged work capacity of one.
  const beforeReplyBurst=await readFile(nodeConfig,'utf8');
  for(let i=0;i<16;i++){
   const reply=await remoteModels(['clear-interpreter']);
   assert.equal(reply.success,true);assert.equal(reply.acknowledged,true);
   assert.equal(reply.mutation_requested,true);assert.equal(reply.data.changed,false);
  }
  assert.equal(await readFile(nodeConfig,'utf8'),beforeReplyBurst,'reply retirement must not rewrite unchanged settings');
  // Resolve the target node's cache, not a path on the coordinator device.
  const cacheBytes=Buffer.from('GGUF synthetic remote-selection fixture, never loaded');
  const cacheHash=createHash('sha256').update(cacheBytes).digest('hex'),cacheId='cache-selection-fixture';
  await mkdir(join(scratch,'models'),{recursive:true});const cachePath=join(scratch,'models',cacheHash+'.gguf');
  await writeFile(cachePath,cacheBytes);
  const cacheEntry={id:cacheId,name:'Cache selection fixture',modality:'text',url:'https://example.invalid/never-fetched',sha256:cacheHash,bytes:cacheBytes.length};
  await writeFile(join(scratch,'models','installed.json'),JSON.stringify([{...cacheEntry,path:toNamespacedPath(await realpath(cachePath))}]));
  const cacheConfig=(await json(node+'/api/config')).config,priorCatalog=cacheConfig.model_catalog;
  cacheConfig.model_catalog=[cacheEntry];await json(node+'/api/config',{config:cacheConfig},'PUT');
  const runtimeBeforeCache=(await json(node+'/api/diagnostics')).runtime_id,requestsBeforeCache=requests.length;
  const cacheSelected=await remoteModels(['select-installed-gguf','--model',cacheId,'--interpreter']);
  assert.equal(cacheSelected.acknowledged,true);assert.equal(cacheSelected.data.provider,'ollama');
  assert.deepEqual(cacheSelected.data.installed_artifact,{model_id:cacheId,sha256:cacheHash,bytes:cacheBytes.length,verified_at_selection:true});
  assert(cacheSelected.data.saved_selections.some(m=>m.backend==='llamacpp'&&m.model_id===cacheId&&m.model_metadata.roles.includes('request_interpreter')));
  assert.equal(cacheSelected.data.registered_models.length,2);assert.equal(requests.length,requestsBeforeCache);
  assert.equal((await json(node+'/api/diagnostics')).runtime_id,runtimeBeforeCache);
  const cacheActivated=await remoteModels(['select-installed-gguf','--model',cacheId,'--interpreter','--activate-backend']);
  assert.equal(cacheActivated.data.provider,'llamacpp');assert.equal(cacheActivated.data.changed,true);
  assert.equal(cacheActivated.data.restart_required,true);assert.equal(cacheActivated.data.registered_models.length,2);
  assert.equal((await json(node+'/api/diagnostics')).runtime_id,runtimeBeforeCache);
  const beforeCacheFailure=await readFile(nodeConfig,'utf8');
  await writeFile(cachePath,Buffer.alloc(cacheBytes.length,43));
  const cacheRejected=await remoteModels(['select-installed-gguf','--model',cacheId,'--interpreter'],false);
  assert.equal(cacheRejected.error_code,'installed_selection_preflight_rejected');
  assert.equal(cacheRejected.outcome_unknown,false);assert.equal(cacheRejected.configuration_write_attempted,false);
  assert.equal(cacheRejected.data.cache_issue,'checksum_mismatch');assert.equal(cacheRejected.data.automatically_retried,false);
  assert.equal(cacheRejected.data.command.request_id,cacheRejected.command.request_id);
  assert.equal(await readFile(nodeConfig,'utf8'),beforeCacheFailure);await writeFile(cachePath,cacheBytes);
  await remoteModels(['remove','--backend','llamacpp','--model',cacheId]);
  await remoteModels(['provider','ollama']);
  const cacheRestored=(await json(node+'/api/config')).config;
  if(priorCatalog===undefined)delete cacheRestored.model_catalog;else cacheRestored.model_catalog=priorCatalog;
  await json(node+'/api/config',{config:cacheRestored},'PUT');
  const remoteBody={schema_version:1,node_id:id,command:{operation:'show'}};
  assert.equal((await fetch(eef+'/api/commands/node/models',{method:'POST',headers:{'Content-Type':'application/json',Origin:'https://untrusted.example'},body:JSON.stringify(remoteBody)})).status,403);
  assert.equal(requests.length,1,'selection commands must not execute inference');
  tagGate={entered:false,release:null};
  const installation=json(eef+`/api/node/${id}/invoke`,{capability:'node.configure',action:'install',params:{id:'fixture-revoked'},timeout:10});
  await until(()=>tagGate.entered,'remote installation backend inspection');
  const revoked=(await json(node+'/api/config')).config;revoked.management.allow_remote=false;
  await json(node+'/api/config',{config:revoked},'PUT');
  tagGate.release();tagGate=null;
  const deniedInstall=await installation;assert.equal(deniedInstall.success,false);
  assert.equal(pullRequests,0,'revoked approval must block download admission after inspection');
  assert.notEqual((await json(node+'/api/status')).download?.state,'downloading');
  const deniedSave=await json(eef+`/api/node/${id}/invoke`,{capability:'node.configure',action:'save',params:{config:{name:'Unapproved',management:{allow_remote:true}}},timeout:10});
  assert.equal(deniedSave.data.approval_required,true);
  const deniedConfig=(await json(node+'/api/config')).config;
  assert.equal(deniedConfig.management.allow_remote,false);assert.notEqual(deniedConfig.name,'Unapproved');
  deniedConfig.management.allow_remote=true;await json(node+'/api/config',{config:deniedConfig},'PUT');
  const before=await readFile(nodeConfig,'utf8');
  await nodeCommand(['hints','--backend','ollama','--model','fixture-text','--capability','vlm.analyze'],false);
  assert.equal(await readFile(nodeConfig,'utf8'),before);
  await nodeCommand(['hints','--backend','ollama','--model','fixture-vision','--capability','llm.infer']);
  await nodeCommand(['interpreter','--backend','ollama','--model','fixture-text']);
  const saved=await nodeCommand(['interpreter','--backend','ollama','--model','fixture-vision']);
  assert.equal(saved.changed,true);assert.equal(saved.restart_required,true);assert.equal(saved.running,true);
  assert(saved.registered_models.find(m=>m.model_id==='fixture-vision').model_metadata.capabilities.includes('vlm.analyze'),'saved is not applied');
  assert.deepEqual(saved.saved_selections.find(m=>m.model_id==='fixture-vision').model_metadata.roles,['request_interpreter']);
  assert.deepEqual(saved.saved_selections.find(m=>m.model_id==='fixture-text').model_metadata.roles,['node_tool'],'moving the interpreter preserves unrelated roles');
  assert.equal(saved.saved_selections.filter(m=>m.model_metadata.roles?.includes('request_interpreter')).length,1);
  assert.equal((await fetch(node+'/api/commands/models',{method:'POST',headers:{'Content-Type':'application/json',Origin:'https://untrusted.example'},body:JSON.stringify({schema_version:1,expected_node_id:id,command:{operation:'show'}})})).status,403);
  assert.equal((await fetch(node+'/api/commands/models',{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify({schema_version:1,expected_node_id:'wrong',command:{operation:'show'}})})).status,400);
  await localRestart();
  await until(async()=>{const m=(await inventory()).nodes[0]?.models.find(m=>m.instance.model_id==='fixture-vision');return m?.roles?.includes('request_interpreter')&&!m.capabilities.includes('vlm.analyze');},'owner hints apply after restart');
  await ordinaryDenied('vlm.analyze');assert.equal(requests.length,1,'restricted capability must not reach backend');
  assert.equal((await json(eef+'/api/commands/models/route?capability=vlm.analyze')).candidates.length,0);
  const restrictedReceipt=randomUUID();
  const restricted=await json(node+'/api/commands/requests/submit',{schema_version:1,expected_node_id:id,command:{operation_id:restrictedReceipt,request:{schema_version:1,original_text:'Do not execute a removed capability',title:'Restricted vision fixture',input:{schema_version:1,template:'analyze_image',params:{prompt:'must not execute',model_id:'fixture-vision'},constraints:{node_id:id,backend:'ollama'}}}}});
  assert.equal(restricted.success,false);assert.equal(restricted.dispatched,false);
  assert.equal((await nodeJobs(['find','--operation-id',restrictedReceipt])).data.total,0,'undisclosed capability cannot even create a review source');
  assert.equal(requests.length,1,'explicit request cannot widen the owner model capability hints');
  const preview=await json(eef+'/api/commands/models/route?capability=llm.infer&role=request_interpreter');
  assert.equal(preview.execution_authorized,false);assert.equal(preview.candidates.length,1);assert.equal(preview.candidates[0].instance.model_id,'fixture-vision');
  assert.equal((await json(eef+'/api/commands/models/route?capability=llm.infer&role=missing')).candidates.length,0);
  const previewCli=launch('eef',['--config',eefConfig,'models','route','--capability','llm.infer','--role','request_interpreter','--json']);
  const previewTimer=setTimeout(()=>previewCli.kill(),20000);const previewResult=await previewCli.result;clearTimeout(previewTimer);
  assert.equal(previewResult.code,0,previewResult.err);assert.equal(JSON.parse(previewResult.out).candidates[0].instance.model_id,'fixture-vision');
  assert.equal(requests.length,1,'preview must not run inference');
  assert.equal((await fetch(eef+'/api/commands/models/route?capability=llm.infer',{headers:{Origin:'https://untrusted.example'}})).status,403);
  const job=await reviewed.save({template:'generate_text',params:{prompt:'fixture only'},constraints:{model_role:'request_interpreter',node_id:id}});
  assert.equal(requests.length,1,'compilation and review must not run inference');
  await reviewed.start(await reviewed.approve(job));assert.equal((await reviewed.result(job.id)).success,true);
  const assignment=(await json(eef+'/api/jobs/'+job.id)).tasks[0].assignment;
  assert.equal(assignment.node_id,id);assert.equal(assignment.model_id,'fixture-vision');assert.equal(assignment.backend,'ollama');
  assert.equal(requests.length,2);assert.equal(requests.at(-1).model,'fixture-vision');
  assert.equal((await nodeJobs(['get',job.id])).data.request_context.origin_node,id);
  assert.equal((await nodeJobs(['output',job.id])).data.outputs[0].content,'fixture response; not real inference');
  await reviewed.cleanup(job.id);
  await rejectedSelection(['--role','missing','--target-node',id]);
  assert.equal(requests.length,2,'missing role must not use unrelated model');
  for(const prompt of ['response-fixture-missing','response-fixture-oversize','response-fixture-chunked']) {
   const report=await infer(prompt);assert.equal(report.success,false,prompt);assert.equal(report.state,'unknown',prompt);
  }
  const partial=await infer('response-fixture-length');
  assert.equal(partial.success,true);assert.equal(partial.data.completion_complete,false);assert.equal(partial.data.content,'partial fixture');
  const unknown=await infer('response-fixture-unknown');
  assert.equal(unknown.success,true);assert.equal(unknown.data.completion_complete,null);assert.equal(unknown.data.finish_reason,null);
  const timeoutStart=Date.now(),beforeTimeout=requests.length;
  assert.equal((await infer('response-fixture-stalled',{max_execution_ms:1000,max_result_bytes:65536})).success,false);
  assert(Date.now()-timeoutStart<8000,'request timeout covers stalled response bodies');
  assert.equal(requests.length,beforeTimeout+1,'inference timeout never retries the request');
  assert.equal((await nodeJobs(['list'])).data.jobs.length,0,'all isolated sources explicitly cleaned up');
  const limitRuntime=(await json(node+'/api/diagnostics')).runtime_id;
  const limitShow=await nodeConnection('limits');assert.equal(limitShow.report_type,'node_request_limits');
  assert.deepEqual(limitShow.saved_request_limits,{work:1,control:2});
  const limitSave=await nodeConnection('limits',['--work','2']);
  assert.equal(limitSave.changed,true);assert.equal(limitSave.restart_requested,false);assert.equal(limitSave.restart_required,true);
  assert.deepEqual(limitSave.applied_request_limits,{work:1,control:2});assert.equal(limitSave.saved_request_limits.work,2);
  const limitBytes=await readFile(nodeConfig,'utf8');
  await nodeConnection('limits',['--work','0'],false);assert.equal(await readFile(nodeConfig,'utf8'),limitBytes);
  assert.equal((await nodeConnection('limits',['--work','2'])).changed,false);
  await nodeConnection('limits',['--work','1']);
  assert.equal((await json(node+'/api/diagnostics')).runtime_id,limitRuntime,'saving limits must not restart runtime');
  assert.equal((await nodeJobs(['get',job.id],false)).error_code,'job_rejected');
  assert.equal((await nodeJobs(['output',job.id],false)).error_code,'job_rejected');
  for(const action of ['pause','stop']){
   const prompt='gated-job-'+action;
   const submitted=await nodeJobs(['generate-text','--prompt',prompt,'--role','request_interpreter','--target-node',id]);
   const jobId=submitted.data.id;assert.equal(submitted.acknowledged,true);
   assert.equal(submitted.data.status,'awaiting_approval');
   assert.equal(submitted.data.request_context.origin_node,id);
   assert.equal(submitted.creation_correlated,true);
   assert.equal(submitted.data.request_context.request_id,submitted.operation_id);
   const found=await nodeJobs(['find','--operation-id',submitted.operation_id]);
   assert.equal(found.data.total,1);assert.equal(found.data.jobs[0].id,jobId);
   const countBeforeGrant=requests.length;assert(!gates.has(prompt),'saving work cannot execute it');
   await reviewed.start(await reviewed.approve(submitted.data,{max_execution_ms:30000,max_result_bytes:65536}));
   await until(()=>gates.has(prompt),'backend gate for '+action);
   assert.equal(requests.length,countBeforeGrant+1);
   const saturationCount=requests.length;
   await ordinaryDenied('llm.infer');
   assert.equal(requests.length,saturationCount,'ordinary workload refused even while full');
   const controlPing=await json(eef+`/api/node/${id}/invoke`,{capability:'system.ping',action:'run',params:{},timeout:3});
   assert.equal(controlPing.success,true,'work saturation must leave ping capacity');
   const controlStatus=await json(eef+`/api/node/${id}/invoke`,{capability:'node.configure',action:'status',params:{},timeout:3});
   assert.equal(controlStatus.success,true,'work saturation must leave status capacity');
   const beforeRejectedModelSave=await readFile(nodeConfig,'utf8');
   const busyModel=await remoteModels(['provider','auto'],false);
   assert.equal(busyModel.error_code,'node_busy');assert.equal(busyModel.not_started,true);
   assert.equal(busyModel.mutation_requested,false);assert.equal(busyModel.outcome_unknown,false);
   assert.equal(await readFile(nodeConfig,'utf8'),beforeRejectedModelSave,'capacity rejection cannot save models');
   if(action==='pause'){
    const approvedConfig=(await json(node+'/api/config')).config;
    const deniedConfig=structuredClone(approvedConfig);deniedConfig.management.allow_remote=false;
    await json(node+'/api/config',{config:deniedConfig},'PUT');
    const remoteRestart=launch('eef',['--config',eefConfig,'node','restart','--node',id,'--wait-seconds','0','--json']);
    const deniedCommand=await remoteRestart.result;assert.equal(deniedCommand.code,1,deniedCommand.out+deniedCommand.err);
    const deniedReport=JSON.parse(deniedCommand.out);assert.equal(deniedReport.error_code,'approval_required');assert.equal(deniedReport.restart_requested,false);
    const deniedRestart=await json(eef+`/api/node/${id}/invoke`,{capability:'node.configure',action:'restart',params:{},timeout:3});
    assert.equal(deniedRestart.success,false);assert.notEqual(deniedRestart.error_type,'NodeBusy');
    assert.equal((await json(node+'/api/status')).connection.state,'connected','control reserve must not bypass restart approval');
    await json(node+'/api/config',{config:approvedConfig},'PUT');
   }
   const limitReport=await json(node+'/api/diagnostics');
   assert.deepEqual(limitReport.request_limits,{work:1,control:2});assert(limitReport.busy_rejections>=1);
   assert.equal(submitted.data.admission.reused,false);
   const receiptRequest={schema_version:1,expected_node_id:id,operation_id:submitted.operation_id,
    command:{operation:'generate_text',prompt,role:'request_interpreter',target_node:id}};
   const beforeDuplicates=requests.length;
   const duplicates=await Promise.all(Array.from({length:8},()=>json(node+'/api/commands/jobs',receiptRequest)));
   assert(duplicates.every(r=>r.success&&r.creation_correlated&&r.data.id===jobId&&r.data.admission.reused));
   assert.equal(requests.length,beforeDuplicates,'repeated receipt must not dispatch another inference');
   const conflict=await json(node+'/api/commands/jobs',{...receiptRequest,command:{...receiptRequest.command,prompt:'different work'}});
   assert.equal(conflict.success,false);assert.equal(conflict.error_code,'concrete_unconfirmed');
   assert.equal(conflict.outcome_unknown,true,'adapter conservatively requires receipt inspection');
   assert.equal(requests.length,beforeDuplicates,'conflicting receipt must not run different work');
   if(action==='pause'){
    const beforePause=await reviewed.get(jobId);
    assert.equal((await nodeJobs(['pause',jobId],false)).error_code,'job_rejected');
    assert.deepEqual(await reviewed.get(jobId),beforePause,'unsupported pause cannot mutate the grant');
    assert.equal((await nodeJobs(['resume',jobId],false)).error_code,'job_rejected');
   }else{
    assert.equal((await nodeJobs(['stop',jobId])).data.status,'stopping');
    // Keep the backend held until stop has won. A stop intent is not a
    // completed backend result or a physical termination certificate.
    await until(async()=>(await json(node+'/api/jobs/'+jobId)).status==='interrupted','exact stop retained before backend release');
    const report=await reviewed.result(jobId);
    assert.equal(report.state,'unknown');assert.equal(report.success,false);
    assert.equal(report.error_code,'grant_stop_requested_effects_unknown');assert.equal(report.data,null);
    assert(gates.has(prompt),'backend response remains held while stop becomes unknown');
    const snapshot=await reviewed.get(jobId),grant=snapshot.grant_attempts[0].grant_id;
    const stopEvidence=await nodeRequests(['stop-receipt',jobId,'--grant-id',grant]);
    assert.equal(stopEvidence.success,true);assert.equal(stopEvidence.read_only,true);
    assert.equal(stopEvidence.stop_requested,false);assert.equal(stopEvidence.execution_requested,false);
    assert.equal(stopEvidence.automatically_retried,false);assert.equal(stopEvidence.reconciled,false);
    assert.equal(stopEvidence.result.inspection.availability,'retained');
    assert.equal(stopEvidence.result.inspection.scope.grant_id,grant);
    assert.equal(stopEvidence.result.inspection.evidence.stop_requested,true);
    assert.equal(stopEvidence.result.inspection.evidence.execution_stopped,false);
    assert.equal(stopEvidence.result.inspection.evidence.external_effects_stopped,false);
    assert.equal((await reviewed.get(jobId)).revision,snapshot.revision,'inspection does not reconcile or signal stop again');
   }
   gates.get(prompt)();gates.delete(prompt);
   const expectedStatus=action==='pause'?'completed':'interrupted';
   await until(async()=>(await json(node+'/api/jobs/'+jobId)).status===expectedStatus,'settled node job '+action);
   const final=await reviewed.result(jobId);
   assert.equal(final.state,action==='pause'?'completed':'unknown');
   assert.equal((await nodeJobs(['get',jobId])).data.request_context.origin_node,id);
   if(action==='pause'){
    const output=await nodeJobs(['output',jobId]);assert.equal(output.data.outputs[0].content,'fixture response; not real inference');
    assert.equal(output.data.outputs[0].truncated,false);
   }else{
    assert.equal(final.error_code,'grant_stop_requested_effects_unknown');assert.equal(final.data,null,'late shared-provider completion cannot invent output');
    const output=await nodeJobs(['output',jobId]);assert.equal(output.data.status,'interrupted');
    assert.equal(output.data.outputs.length,1);assert.equal(output.data.outputs[0].content,null);
    assert.equal(output.data.outputs[0].truncated,false);
   }
   const beforeFinishedReceipt=requests.length;
   const retained=await json(node+'/api/commands/jobs',receiptRequest);
   assert.equal(retained.data.id,jobId);assert.equal(retained.data.admission.reused,true);
   assert.equal(retained.data.status,expectedStatus);
   assert.equal(requests.length,beforeFinishedReceipt,'finished work must not restart on receipt reuse');
   const recoveredCapacity=await infer('after capacity released');
   assert.equal(recoveredCapacity.success,true,'execution capacity must return after completion');
   assert.equal(requests.length,beforeFinishedReceipt+1);
   if(action==='pause')assert.equal((await nodeJobs(['remove',jobId])).data.removed,true);
   else{
    assert.equal((await nodeJobs(['remove',jobId],false)).error_code,'job_rejected','uncertain work needs explicit abandonment, not deletion');
    await reviewed.cleanup(jobId);
   }
   assert.equal((await nodeJobs(['get',jobId],false)).error_code,'job_rejected');
  }
  assert.equal((await nodeJobs(['list'])).data.jobs.length,0);
  // The full approved restart CLI must not require a free work slot for its
  // permission/runtime checks. Only our disposable fixture app is restarted.
  const restartPrompt='gated-job-reserved-restart',beforeReservedRestart=requests.length;
  const interruptedJob=await reviewed.save({template:'generate_text',params:{prompt:restartPrompt},constraints:{node_id:id,model_role:'request_interpreter'}});
  await reviewed.start(await reviewed.approve(interruptedJob,{max_execution_ms:30000,max_result_bytes:65536}));
  await until(()=>gates.has(restartPrompt),'work occupied before approved remote restart');
  const beforeRemoteRuntime=(await json(node+'/api/diagnostics')).runtime_id;
  const approvedRestart=launch('eef',['--config',eefConfig,'node','restart','--node',id,'--wait-seconds','15','--json']);
  const approvedTimer=setTimeout(()=>approvedRestart.kill(),22000);
  let restarted;
  try{const result=await approvedRestart.result;assert.equal(result.code,0,result.out+result.err);restarted=JSON.parse(result.out);}finally{clearTimeout(approvedTimer);}
  assert.equal(restarted.completed,true);assert.equal(restarted.previous_runtime_id,beforeRemoteRuntime);
  assert.notEqual(restarted.runtime_id,beforeRemoteRuntime);
  gates.get(restartPrompt)();gates.delete(restartPrompt);
  await until(async()=>(await inventory()).nodes[0]?.models.length===2,'reconnect after reserved remote restart');
  assert.equal(requests.length,beforeReservedRestart+1,'restart cannot replay interrupted inference');
  assert.equal((await reviewed.wait(interruptedJob.id)).status,'interrupted');
  await reviewed.cleanup(interruptedJob.id);
  // Capacity-one stress is complete. Test later multi-stage model controls with
  // the normal work budget; a reply may still occupy the final work permit.
  await nodeConnection('limits',['--work','16']);await localRestart();
  await until(async()=>(await inventory()).nodes[0]?.models.length===2,'reconnect with default work capacity');
  // Lose the local HTTP reply after EEF has accepted the creation. The CLI must
  // retain its pre-send receipt and never replay; restore only our fixture marker.
  const marker=nodeConfig+'.api.json',originalMarker=await readFile(marker,'utf8');
  let forwarded=0,forwardError=null,accepted=null,replyMode='drop';
  const dropReply=createServer(async(req,res)=>{
   try{
    if(replyMode==='old'){res.writeHead(404);return res.end('{}');}
    let body='';for await(const chunk of req)body+=chunk;
    forwarded++;
    const response=await fetch(node+req.url,{method:req.method,headers:{'Content-Type':'application/json'},body,signal:AbortSignal.timeout(15000)});
    accepted=await response.json();
    if(replyMode==='cpu-corrupt'){
     const substituted=structuredClone(accepted);
     substituted.saved_selections.find(m=>m.backend==='llamacpp').cpu_threads=7;
     res.setHeader('Content-Type','application/json');return res.end(JSON.stringify(substituted));
    }
    if(replyMode==='memory-corrupt'){
     const substituted=structuredClone(accepted);
     substituted.saved_selections.find(m=>m.backend==='llamacpp').job_memory_limit_mib=256;
     res.setHeader('Content-Type','application/json');return res.end(JSON.stringify(substituted));
    }
    if(replyMode==='role-corrupt'){
     const substituted=structuredClone(accepted);
     substituted.saved_selections[0].model_metadata.roles=['request_interpreter'];
     res.setHeader('Content-Type','application/json');return res.end(JSON.stringify(substituted));
    }
   }catch(error){forwardError=error;}finally{if(replyMode==='drop')res.destroy();}
  });
  try{
   await new Promise(r=>dropReply.listen(0,'127.0.0.1',r));
   await writeFile(marker,JSON.stringify({schema_version:1,node_id:id,address:`127.0.0.1:${dropReply.address().port}`}));
   const unconfirmed=await nodeJobs(['generate-text','--prompt','receipt-loss fixture','--role','request_interpreter'],false);
   assert.equal(forwardError,null);assert.equal(forwarded,1,'lost reply must not replay creation');
   assert.equal(unconfirmed.error_code,'local_job_reply_unconfirmed');assert.equal(unconfirmed.outcome_unknown,true);
   assert.equal(unconfirmed.operation_id,accepted.operation_id);
   await writeFile(marker,originalMarker);
   const found=await nodeJobs(['find','--operation-id',unconfirmed.operation_id]);
   assert.equal(found.data.total,1);assert.equal(found.data.jobs[0].id,accepted.data.id);
   assert.equal(accepted.data.status,'awaiting_approval');
   const count=requests.length;
   assert.equal((await reviewed.get(accepted.data.id)).status,'awaiting_approval');
   assert.equal(requests.length,count,'receipt lookup cannot execute saved work');
   await reviewed.start(await reviewed.approve(accepted.data));
   assert.equal((await reviewed.result(accepted.data.id)).success,true);
   await nodeJobs(['remove',accepted.data.id]);
   assert.equal((await nodeJobs(['find','--operation-id',unconfirmed.operation_id])).data.total,0,'lookup searches retained history only');
   // A role mutation can commit before a lost/substituted reply. Keep its exact
   // target, never retry or switch provider/restart behind the owner's back.
   const roleCount=requests.length,beforeRoleForward=forwarded;
   replyMode='role-corrupt';await writeFile(marker,JSON.stringify({schema_version:1,node_id:id,address:`127.0.0.1:${dropReply.address().port}`}));
   const corruptRole=await nodeCommand(['clear-interpreter'],false);
   assert.equal(corruptRole.error_code,'local_model_role_reply_unconfirmed');assert.equal(corruptRole.outcome_unknown,true);
   assert.deepEqual(corruptRole.command,{operation:'clear_interpreter'});assert.equal(corruptRole.automatically_retried,false);
   assert.equal(forwarded,beforeRoleForward+1);await writeFile(marker,originalMarker);
   const inspectedClear=await nodeCommand(['show']);assert(inspectedClear.saved_selections.every(m=>!m.model_metadata.roles?.includes('request_interpreter')));
   replyMode='drop';await writeFile(marker,JSON.stringify({schema_version:1,node_id:id,address:`127.0.0.1:${dropReply.address().port}`}));
   const lostRole=await nodeCommand(['interpreter','--backend','ollama','--model','fixture-vision'],false);
   assert.equal(lostRole.error_code,'local_model_role_reply_unconfirmed');assert.equal(lostRole.outcome_unknown,true);
   assert.deepEqual(lostRole.command,{operation:'interpreter',backend:'ollama',model_id:'fixture-vision'});assert.equal(forwarded,beforeRoleForward+2);
   await writeFile(marker,originalMarker);
   assert((await nodeCommand(['show'])).saved_selections.find(m=>m.model_id==='fixture-vision').model_metadata.roles.includes('request_interpreter'));
   replyMode='old';await writeFile(marker,JSON.stringify({schema_version:1,node_id:id,address:`127.0.0.1:${dropReply.address().port}`}));
   const oldRole=await nodeCommand(['clear-interpreter'],false);
   assert.equal(oldRole.error_code,'unsupported_command');assert.equal(oldRole.outcome_unknown,false);assert.equal(forwarded,beforeRoleForward+2);
   await writeFile(marker,originalMarker);assert.equal(requests.length,roleCount,'role inspection/uncertainty must not infer or replay');
   // CPU settings use the same owner commands, exact saved-value confirmation,
   // and no-replay boundary. The tiny file is never loaded as a model here.
   const cpuFile=join(scratch,'PRIVATE-cpu-fixture.gguf');await writeFile(cpuFile,'not-a-model');
   await nodeCommand(['select-gguf','--model','cpu-fixture','--file',cpuFile,'--cpu-threads','4']);
   const cpuBefore=JSON.parse(await readFile(nodeConfig,'utf8'));
   cpuBefore.management.allow_remote=false;await json(node+'/api/config',{config:cpuBefore},'PUT');
   const denied=await remoteModels(['cpu-threads','--model','cpu-fixture','--threads','2'],false);
   assert.equal(denied.error_code,'approval_required');
   assert.deepEqual(denied.command,{operation:'cpu_threads',model_id:'cpu-fixture',threads:2});
   assert.equal(JSON.parse(await readFile(nodeConfig,'utf8')).models.llamacpp.slots[0].cpu_threads,4);
   cpuBefore.management.allow_remote=true;await json(node+'/api/config',{config:cpuBefore},'PUT');
   const approved=await remoteModels(['cpu-threads','--model','cpu-fixture','--threads','2']);
   assert.deepEqual(approved.command,denied.command);
   assert.equal(approved.data.saved_selections.find(m=>m.model_id==='cpu-fixture').cpu_threads,2);
   const runtimeBefore=(await json(node+'/api/diagnostics')).runtime_id,cpuForward=forwarded;
   for(const [mode,threads] of [['drop','3'],['cpu-corrupt','4']]){
    replyMode=mode;await writeFile(marker,JSON.stringify({schema_version:1,node_id:id,address:`127.0.0.1:${dropReply.address().port}`}));
    const unconfirmedCpu=await nodeCommand(['cpu-threads','--model','cpu-fixture','--threads',threads],false);
    assert.equal(unconfirmedCpu.error_code,'local_model_cpu_threads_reply_unconfirmed');assert.equal(unconfirmedCpu.outcome_unknown,true);
    assert.equal(unconfirmedCpu.automatically_retried,false);assert.equal(unconfirmedCpu.command.threads,Number(threads));
    await writeFile(marker,originalMarker);
    assert.equal((await nodeCommand(['show'])).saved_selections.find(m=>m.model_id==='cpu-fixture').cpu_threads,Number(threads));
   }
   assert.equal(forwarded,cpuForward+2,'CPU mutations must not replay after lost/substituted replies');
   replyMode='old';await writeFile(marker,JSON.stringify({schema_version:1,node_id:id,address:`127.0.0.1:${dropReply.address().port}`}));
   const oldCpu=await nodeCommand(['cpu-threads','--model','cpu-fixture','--auto'],false);
   assert.equal(oldCpu.error_code,'unsupported_command');assert.equal(oldCpu.outcome_unknown,false);assert.equal(forwarded,cpuForward+2);
   await writeFile(marker,originalMarker);
   const clearedCpu=await remoteModels(['cpu-threads','--model','cpu-fixture','--auto']);
   assert.equal(clearedCpu.data.saved_selections.find(m=>m.model_id==='cpu-fixture').cpu_threads,undefined);
   if(process.platform==='win32'){
    const permission=(await json(node+'/api/config')).config;permission.management.allow_remote=false;await json(node+'/api/config',{config:permission},'PUT');
    const refused=await remoteModels(['memory-limit','--model','cpu-fixture','--mib','128'],false);
    assert.equal(refused.error_code,'approval_required');assert.equal((await nodeCommand(['show'])).saved_selections.find(m=>m.model_id==='cpu-fixture').job_memory_limit_mib,undefined);
    permission.management.allow_remote=true;await json(node+'/api/config',{config:permission},'PUT');
    const limited=await remoteModels(['memory-limit','--model','cpu-fixture','--mib','128']);
    assert.equal(limited.data.saved_selections.find(m=>m.model_id==='cpu-fixture').job_memory_limit_mib,128);
    const memoryForward=forwarded;
    for(const [mode,mib] of [['drop','192'],['memory-corrupt','128']]){
     replyMode=mode;await writeFile(marker,JSON.stringify({schema_version:1,node_id:id,address:`127.0.0.1:${dropReply.address().port}`}));
     const uncertain=await nodeCommand(['memory-limit','--model','cpu-fixture','--mib',mib],false);
     assert.equal(uncertain.error_code,'local_model_memory_limit_reply_unconfirmed');assert.equal(uncertain.outcome_unknown,true);
     assert.equal(uncertain.automatically_retried,false);assert.equal(uncertain.command.mib,Number(mib));
     await writeFile(marker,originalMarker);assert.equal((await nodeCommand(['show'])).saved_selections.find(m=>m.model_id==='cpu-fixture').job_memory_limit_mib,Number(mib));
    }
    assert.equal(forwarded,memoryForward+2,'Memory settings must not replay after lost/substituted replies');
    replyMode='old';await writeFile(marker,JSON.stringify({schema_version:1,node_id:id,address:`127.0.0.1:${dropReply.address().port}`}));
    const old=await nodeCommand(['memory-limit','--model','cpu-fixture','--default'],false);
    assert.equal(old.error_code,'unsupported_command');assert.equal(old.outcome_unknown,false);
    await writeFile(marker,originalMarker);
    assert.equal((await remoteModels(['memory-limit','--model','cpu-fixture','--default'])).data.saved_selections.find(m=>m.model_id==='cpu-fixture').job_memory_limit_mib,undefined);
    await writeFile(join(scratch,'memory-command-results.json'),JSON.stringify({passed:true,physical_two_pc:false,real_model:false,
     remote_owner_refusal:true,approved_remote_setting:true,exact_model_target:true,local_lost_reply_not_replayed:true,
     substituted_limit_reply_refused:true,old_node_command_refused:true,explicit_default_reset:true,runtime_not_restarted:true},null,2));
   }
   assert.equal((await json(node+'/api/diagnostics')).runtime_id,runtimeBefore);
   assert.equal(requests.length,roleCount,'CPU settings must not infer or execute workloads');
   await nodeCommand(['remove','--backend','llamacpp','--model','cpu-fixture']);
  }finally{
   await writeFile(marker,originalMarker);dropReply.closeAllConnections();await new Promise(r=>dropReply.close(r));
  }
  assert.equal((await fetch(node+'/api/commands/jobs',{method:'POST',headers:{'Content-Type':'application/json',Origin:'https://untrusted.example'},body:JSON.stringify({schema_version:1,expected_node_id:id,command:{operation:'list'}})})).status,403);
  const newPort=await port(),pending=(await json(node+'/api/config')).config;
  pending.dashboard.port=newPort;await json(node+'/api/config',{config:pending},'PUT');
  assert.equal((await nodeCommand(['show'])).restart_required,true,'commands must reach actual API despite pending port');
  const oldRuntime=(await json(node+'/api/diagnostics')).runtime_id;
  const changed=await localRestart();assert.equal(changed.previous_runtime_id,oldRuntime);
  node=`http://127.0.0.1:${newPort}`;
  assert.equal((await json(node+'/api/diagnostics')).runtime_id,changed.runtime_id);
  await until(async()=>(await inventory()).nodes[0]?.models.length===2,'reconnect after local API port change');
  const countBeforePause=requests.length;
  assert.equal((await nodeConnection('show')).saved_connection_enabled,true);
  assert.equal((await nodeConnection('pause')).saved_connection_enabled,false);
  await until(async()=>(await json(node+'/api/status')).connection.state==='paused','connection paused while command API stays available');
  assert.equal((await nodeJobs(['list'],false)).error_code,'not_sent');
  assert.equal((await nodeConnection('resume')).saved_connection_enabled,true);
  await until(async()=>(await inventory()).nodes[0]?.models.length===2,'connection resumed through same command API');
  assert.equal(requests.length,countBeforePause,'pause/resume does not execute inference');
  for(const model of ['fixture-text','fixture-vision'])await remoteModels(['remove','--backend','ollama','--model',model]);
  const removed=await nodeCommand(['show']);assert.equal(removed.saved_selections.length,0);assert.equal(removed.registered_models.length,2);
 }else{
  const unsupported=await remoteModels(['provider','auto'],false);
  assert.equal(unsupported.error_code,'model_commands_unavailable');assert.equal(unsupported.mutation_requested,false);
  const current=(await json(node+'/api/config')).config;current.models.ollama.selected=[];
  await json(node+'/api/config',{config:current},'PUT');
 }
 if(legacyNode)await json(node+'/api/restart',{});else await localRestart();
 await until(async()=>(await inventory()).nodes[0]?.models.length===0&&(await json(eef+'/api/status')).models.length===0,'empty snapshot replaces old models');
 assert.equal((await json(node+'/api/diagnostics')).node_id,id);
 if(!legacyNode){
  const healthy=(await json(node+'/api/config')).config;
  const broken=structuredClone(healthy),fakeGguf=join(scratch,'unusable-fixture.gguf');
  await writeFile(fakeGguf,'fixture only: not a model');
  broken.models.provider='llamacpp';
  broken.models.llamacpp={binary:join(scratch,'missing-llama-server.exe'),slots:[{model_id:'unavailable-fixture',port:await port(),model_path:fakeGguf}]};
  broken.python=join(scratch,'missing-python.exe');broken.permissions.media.tts=true;
  await json(node+'/api/config',{config:broken},'PUT');await localRestart();
  await until(async()=>(await json(node+'/api/status')).connection.state==='connected','node reconnects despite missing model and Python runtimes');
  const diagnostics=await json(node+'/api/diagnostics');
  assert.deepEqual(diagnostics.startup_issues.sort(),['llamacpp_startup_failed','python_runtime_unavailable']);
  assert.equal(diagnostics.model_count,1);assert(!JSON.stringify(diagnostics).includes(scratch));
  const unavailable=(await inventory()).nodes[0].models[0];
  assert.equal(unavailable.lifecycle,'error');assert.equal(unavailable.availability,'unavailable');
  const state=await json(node+'/api/status');assert(!state.capabilities.includes('llm.infer'));assert(!state.capabilities.includes('tts.speak'));
  assert.equal((await nodeJobs(['list'])).success,true,'deterministic commands remain connected');
  const ping=await json(eef+`/api/node/${id}/invoke`,{capability:'system.ping',action:'run',params:{},timeout:10});assert.equal(ping.success,true);
  assert.equal((await nodeCommand(['show'])).saved_selections[0].model_id,'unavailable-fixture','failure must not delete owner selections');
  await json(node+'/api/config',{config:healthy},'PUT');
  const restartResponse=await fetch(eef+'/api/commands/node/restart',{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify({node_id:id,wait_seconds:60}),signal:AbortSignal.timeout(70000)});
  assert(restartResponse.ok);assert.equal((await restartResponse.json()).completed,true,'approved remote restart works without optional runtimes');
  await until(async()=>(await json(node+'/api/status')).connection.state==='connected','node recovers after fixing optional configuration');
  assert.deepEqual((await json(node+'/api/diagnostics')).startup_issues,[]);
 }
 await writeFile(join(scratch,'results.json'),JSON.stringify({passed:true,physical_two_pc:false,backend:'fake HTTP Ollama fixture',real_inference:false,model_downloads:false,
  legacy_node:!!legacyNode,pinned_tls:!legacyNode,reviewed_grants:!legacyNode,ordinary_workloads_refused:true,versioned_inventory:!legacyNode,owner_cli:true,api_only_both_roles:true,
  saved_ui_preferences:uiConfig,connection_commands:!legacyNode,origin_guard:true,selection_commands:!legacyNode,remote_selection_commands:!legacyNode,old_node_remote_command_refusal:!!legacyNode,
  node_job_commands:!legacyNode,node_origin_receipts:!legacyNode,explicit_text_output:!legacyNode,unsupported_pause_resume_refused:!legacyNode,
  stop_before_backend_release_retains_unknown:!legacyNode,stop_read_only_intent_not_physical_termination:!legacyNode,late_provider_completion_never_invents_output:!legacyNode,
  uncertain_work_requires_explicit_abandonment:!legacyNode,native_termination_proven:false,local_restart:!legacyNode,pending_api_port_change:!legacyNode,
  role_preview_and_reviewed_jobs:!legacyNode,capability_restrictions_enforced:!legacyNode,explicit_backend_no_fallback:!legacyNode,empty_snapshot_replacement:true,stable_node_identity:true},null,2));
 if(!legacyNode)await writeFile(join(scratch,'reply-ownership-results.json'),JSON.stringify({passed:true,
  work_capacity:1,sequential_noop_commands:16,all_acknowledged_without_retry:true,configuration_unchanged:true,physical_two_pc:false},null,2));
 if(!legacyNode)await writeFile(join(scratch,'installed-selection-results.json'),JSON.stringify({passed:true,
  target_cache_verified:true,remote_approval_required:true,remote_preflight_rejection_confirmed:true,explicit_interpreter_role:true,provider_preserved_without_activation:true,
  explicit_ollama_and_gguf_backend_activation:true,activation_saved_not_applied:true,
  saved_not_applied:true,model_downloads:false,synthetic_cache_only:true,physical_two_pc:false},null,2));
 console.log('Model metadata and remote selection protocol checks passed: '+scratch);
}catch(error){
 await Promise.allSettled([
  writeFile(join(scratch,'PRIVATE-failure.json'),JSON.stringify({name:error?.name,message:String(error)},null,2)),
  writeFile(join(scratch,'PRIVATE-logs.json'),JSON.stringify(children.map(c=>({exe:c.spawnfile,code:c.exitCode,...c.logs()})),null,2)),
 ]);
 throw error;
}finally{
 tagGate?.release?.();tagGate=null;
 for(const release of gates.values())release();gates.clear();
 for(const child of children)if(child.exitCode===null)child.kill();
 await Promise.allSettled(children.map(c=>c.result));
 fake.closeAllConnections();await new Promise(r=>fake.close(r));
}
