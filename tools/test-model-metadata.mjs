// Isolated one-PC protocol test with a fake Ollama HTTP backend. No real inference,
// model downloads, personal configuration, or physical cross-node proof.
import assert from 'node:assert/strict';
import {mkdtemp,readFile,writeFile} from 'node:fs/promises';
import {resolve,join} from 'node:path';
import {spawn} from 'node:child_process';
import {createServer as tcpServer} from 'node:net';
import {createServer} from 'node:http';
import {randomBytes} from 'node:crypto';
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
  return res.end(JSON.stringify({message:{content:'fixture response; not real inference'}}));
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
 if(uiConfig)config.dashboard.ui_enabled=false;
 const nodeCommand=async(args,success=true)=>{const child=launch('eefn',['--config',nodeConfig,'models',...args,'--json']);const timer=setTimeout(()=>child.kill(),20000);try{const result=await child.result;assert.equal(result.code,success?0:1,result.err);return JSON.parse(result.out);}finally{clearTimeout(timer);}};
 const remoteModels=async(args,success=true)=>{const child=launch('eef',['--config',eefConfig,'node','models','--node',id,...args,'--json']);const timer=setTimeout(()=>child.kill(),25000);try{const result=await child.result;assert.equal(result.code,success?0:1,result.out+' '+result.err);return JSON.parse(result.out);}finally{clearTimeout(timer);}};
 const nodeJobs=async(args,success=true)=>{const child=launch('eefn',['--config',nodeConfig,'jobs',...args,'--json']);const timer=setTimeout(()=>child.kill(),25000);try{const result=await child.result;assert.equal(result.code,success?0:1,result.out+' '+result.err);return JSON.parse(result.out);}finally{clearTimeout(timer);}};
 const nodeConnection=async(action)=>{const child=launch('eefn',['--config',nodeConfig,'connection',action,'--json']);const timer=setTimeout(()=>child.kill(),25000);try{const result=await child.result;assert.equal(result.code,0,result.out+' '+result.err);return JSON.parse(result.out);}finally{clearTimeout(timer);}};
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
 const invoke=params=>json(eef+`/api/node/${id}/invoke`,{capability:'llm.infer',action:'run',params:{model:'fixture-text',prompt:'protocol fixture',...params},timeout:10});
 const reply=await invoke({backend:'ollama'});assert.equal(reply.success,true);assert.equal(requests.length,1);
 if(!legacyNode){const wrong=await invoke({backend:'llamacpp'});assert.equal(wrong.success,false);assert.equal(requests.length,1,'wrong backend must not fall back');}
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
  const approved=(await json(node+'/api/config')).config;approved.management.allow_remote=true;
  await json(node+'/api/config',{config:approved},'PUT');
  const remoteSaved=await remoteModels(['hints','--backend','ollama','--model','fixture-text','--role','node_tool']);
  assert.equal(remoteSaved.acknowledged,true);assert.equal(remoteSaved.data.changed,true);assert.equal(remoteSaved.data.restart_required,true);
  assert.deepEqual(remoteSaved.data.saved_selections.find(m=>m.model_id==='fixture-text').model_metadata.roles,['node_tool']);
  assert.equal(remoteSaved.data.registered_models.find(m=>m.model_id==='fixture-text').model_metadata.roles,null,'saved must not pretend to be applied');
  assert.equal((await remoteModels(['hints','--backend','ollama','--model','fixture-text','--role','node_tool'])).data.changed,false);
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
  const saved=await nodeCommand(['hints','--backend','ollama','--model','fixture-vision','--capability','llm.infer','--role','request_interpreter']);
  assert.equal(saved.changed,true);assert.equal(saved.restart_required,true);assert.equal(saved.running,true);
  assert(saved.registered_models.find(m=>m.model_id==='fixture-vision').model_metadata.capabilities.includes('vlm.analyze'),'saved is not applied');
  assert.deepEqual(saved.saved_selections.find(m=>m.model_id==='fixture-vision').model_metadata.roles,['request_interpreter']);
  assert.equal((await fetch(node+'/api/commands/models',{method:'POST',headers:{'Content-Type':'application/json',Origin:'https://untrusted.example'},body:JSON.stringify({schema_version:1,expected_node_id:id,command:{operation:'show'}})})).status,403);
  assert.equal((await fetch(node+'/api/commands/models',{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify({schema_version:1,expected_node_id:'wrong',command:{operation:'show'}})})).status,400);
  await localRestart();
  await until(async()=>{const m=(await inventory()).nodes[0]?.models.find(m=>m.instance.model_id==='fixture-vision');return m?.roles?.includes('request_interpreter')&&!m.capabilities.includes('vlm.analyze');},'owner hints apply after restart');
  const denied=await json(eef+`/api/node/${id}/invoke`,{capability:'vlm.analyze',action:'run',params:{model:'fixture-vision',backend:'ollama'},timeout:10});
  assert.equal(denied.success,false);assert.equal(requests.length,1,'restricted capability must not reach backend');
  const preview=await json(eef+'/api/commands/models/route?capability=llm.infer&role=request_interpreter');
  assert.equal(preview.execution_authorized,false);assert.equal(preview.candidates.length,1);assert.equal(preview.candidates[0].instance.model_id,'fixture-vision');
  assert.equal((await json(eef+'/api/commands/models/route?capability=llm.infer&role=missing')).candidates.length,0);
  const previewCli=launch('eef',['--config',eefConfig,'models','route','--capability','llm.infer','--role','request_interpreter','--json']);
  const previewTimer=setTimeout(()=>previewCli.kill(),20000);const previewResult=await previewCli.result;clearTimeout(previewTimer);
  assert.equal(previewResult.code,0,previewResult.err);assert.equal(JSON.parse(previewResult.out).candidates[0].instance.model_id,'fixture-vision');
  assert.equal(requests.length,1,'preview must not run inference');
  assert.equal((await fetch(eef+'/api/commands/models/route?capability=llm.infer',{headers:{Origin:'https://untrusted.example'}})).status,403);
  const job=await json(eef+'/api/jobs',{description:'role-routing fixture',template:'generate_text',params:{prompt:'fixture only'},constraints:{model_role:'request_interpreter',node_id:id}});
  await until(async()=>(await json(eef+'/api/jobs/'+job.id)).status==='completed','role-constrained model job');
  assert.equal(requests.length,2);assert.equal(requests.at(-1).model,'fixture-vision');
  const missing=await json(eef+'/api/jobs',{description:'missing role fixture',template:'generate_text',params:{prompt:'must not execute'},constraints:{model_role:'missing',node_id:id}});
  await until(async()=>(await json(eef+'/api/jobs/'+missing.id)).status==='failed','missing role does not fall back');
  assert.equal(requests.length,2,'missing role must not use unrelated model');
  assert.equal((await nodeJobs(['list'])).data.jobs.length,0,'coordinator-owner jobs are not node-origin jobs');
  assert.equal((await nodeJobs(['get',job.id],false)).error_code,'job_rejected');
  assert.equal((await nodeJobs(['output',job.id],false)).error_code,'job_rejected');
  for(const action of ['pause','stop']){
   const prompt='gated-job-'+action;
   const submitted=await nodeJobs(['generate-text','--prompt',prompt,'--role','request_interpreter','--target-node',id]);
   const jobId=submitted.data.id;assert.equal(submitted.acknowledged,true);
   assert.equal(submitted.data.request_context.origin_node,id);
   assert.equal(submitted.creation_correlated,true);
   assert.equal(submitted.data.request_context.request_id,submitted.operation_id);
   const found=await nodeJobs(['find','--operation-id',submitted.operation_id]);
   assert.equal(found.data.total,1);assert.equal(found.data.jobs[0].id,jobId);
   await until(()=>gates.has(prompt),'backend gate for '+action);
   const controls=await nodeJobs([action,jobId]);assert.equal(controls.data.status,action==='pause'?'pausing':'stopping');
   gates.get(prompt)();gates.delete(prompt);
   await until(async()=>(await json(node+'/api/jobs/'+jobId)).status===(action==='pause'?'paused':'cancelled'),'settled node job '+action);
   if(action==='pause'){
    const count=requests.length;
    await nodeJobs(['resume',jobId]);
    await until(async()=>(await json(node+'/api/jobs/'+jobId)).status==='completed','resumed job complete');
    assert.equal(requests.length,count,'resume must not rerun a completed step');
   }
   assert.equal((await nodeJobs(['get',jobId])).data.request_context.origin_node,id);
   const output=await nodeJobs(['output',jobId]);assert.equal(output.data.outputs[0].content,'fixture response; not real inference');
   assert.equal(output.data.outputs[0].truncated,false);
   assert.equal((await nodeJobs(['remove',jobId])).data.removed,true);
   assert.equal((await nodeJobs(['get',jobId],false)).error_code,'job_rejected');
  }
  assert.equal((await nodeJobs(['list'])).data.jobs.length,0);
  // Lose the local HTTP reply after EEF has accepted the creation. The CLI must
  // retain its pre-send receipt and never replay; restore only our fixture marker.
  const marker=nodeConfig+'.api.json',originalMarker=await readFile(marker,'utf8');
  let forwarded=0,forwardError=null,accepted=null;
  const dropReply=createServer(async(req,res)=>{
   try{
    let body='';for await(const chunk of req)body+=chunk;
    forwarded++;
    const response=await fetch(node+req.url,{method:req.method,headers:{'Content-Type':'application/json'},body,signal:AbortSignal.timeout(15000)});
    accepted=await response.json();
   }catch(error){forwardError=error;}finally{res.destroy();}
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
   await until(async()=>(await json(node+'/api/jobs/'+accepted.data.id)).status==='completed','receipt-recovered job completes');
   await nodeJobs(['remove',accepted.data.id]);
   assert.equal((await nodeJobs(['find','--operation-id',unconfirmed.operation_id])).data.total,0,'lookup searches retained history only');
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
 await writeFile(join(scratch,'results.json'),JSON.stringify({passed:true,physical_two_pc:false,backend:'fake HTTP Ollama fixture',real_inference:false,model_downloads:false,legacy_node:!!legacyNode,versioned_inventory:!legacyNode,owner_cli:true,api_only_both_roles:true,saved_ui_preferences:uiConfig,connection_commands:!legacyNode,origin_guard:true,selection_commands:!legacyNode,remote_selection_commands:!legacyNode,old_node_remote_command_refusal:!!legacyNode,node_job_commands:!legacyNode,node_job_origin_scope:!legacyNode,explicit_text_output:!legacyNode,pause_resume_without_reexecution:!legacyNode,local_restart:!legacyNode,pending_api_port_change:!legacyNode,role_preview_and_jobs:!legacyNode,capability_restrictions_enforced:!legacyNode,explicit_backend_no_fallback:legacyNode?'not supported by old node':true,empty_snapshot_replacement:true,stable_node_identity:true},null,2));
 console.log('Model metadata and remote selection protocol checks passed: '+scratch);
}finally{
 tagGate?.release?.();tagGate=null;
 for(const release of gates.values())release();gates.clear();
 for(const child of children)if(child.exitCode===null)child.kill();
 await Promise.allSettled(children.map(c=>c.result));
 fake.closeAllConnections();await new Promise(r=>fake.close(r));
}
