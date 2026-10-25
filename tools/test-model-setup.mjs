// Native command-first bootstrap, with synthetic HTTP metadata/cache only.
// No real weights, inference, installer, installed-node or physical-device changes.
import assert from 'node:assert/strict';
import {mkdtemp,mkdir,readFile,writeFile,stat} from 'node:fs/promises';
import {join,resolve} from 'node:path';
import {spawn} from 'node:child_process';
import {createServer as tcpServer} from 'node:net';
import {createServer} from 'node:http';
import {createHash,randomBytes,randomUUID} from 'node:crypto';
import {reviewedFixture} from './reviewed-fixture.mjs';
const root=resolve(import.meta.dirname,'..'),bin=resolve(process.env.EEF_TEST_BINARY_DIR||join(root,'target/debug'));
const scratch=await mkdtemp(join(root,'.validation/model-setup-')),children=[],held=[];
const config=join(scratch,'node.json'),identity=join(scratch,'eef.yaml'),id='model-setup-fixture';
const env={...process.env,EEF_NODE_PSK:'',APPDATA:join(scratch,'appdata'),EEF_DISCOVERY_DIR:join(scratch,'discovery'),PATH:join(root,'.tooling/llvm-mingw-20260616-ucrt-x86_64/bin')+';'+process.env.PATH};
let node,eef,installed=false,pulls=0,inferences=0,nodeProcess;
const provider=createServer(async(req,res)=>{
 req.resume();
 if(req.url==='/api/tags')return res.end(JSON.stringify({models:installed?[{name:'fixture:latest'}]:[]}));
 if(req.url==='/api/show')return res.end(JSON.stringify({capabilities:['completion']}));
 if(req.url==='/api/ps')return res.end(JSON.stringify({models:[]}));
 if(req.url==='/api/pull'){
  pulls++;res.writeHead(200,{'Content-Type':'application/x-ndjson'});
  res.write(JSON.stringify({status:'downloading',digest:'synthetic-layer',completed:2,total:4})+'\n');
  held.push(()=>{installed=true;if(!res.destroyed)res.end('{"status":"success"}\n');});return;
 }
 if(req.url==='/api/chat'||req.url==='/api/generate')inferences++;
 res.writeHead(404);res.end();
});
function start(name,args,input,extra={}){
 const child=spawn(join(bin,name+'.exe'),args,{cwd:root,windowsHide:true,env:{...env,...extra},stdio:[input===undefined?'ignore':'pipe','pipe','pipe']});let out='',err='';
 child.stdout.on('data',b=>out=(out+b).slice(-1048576));child.stderr.on('data',b=>err=(err+b).slice(-1048576));child.logs=()=>({out,err});
 child.result=new Promise((r,j)=>{child.on('error',j);child.on('close',code=>r({code,out,err}));});children.push(child);
 if(input!==undefined){child.stdin.on('error',()=>{});child.stdin.end(input);}return child;
}
async function cli(name,args,ok=true,input,configOverride){const c=start(name,['--config',name==='eefn'?(configOverride||config):identity,'--json',...args],input),timer=setTimeout(()=>c.kill(),30000);try{const r=await c.result;assert.equal(r.code,ok?0:1,'CLI failed; private evidence '+scratch);return JSON.parse(r.out);}finally{clearTimeout(timer);}}
const local=(args,ok=true,input)=>cli('eefn',args,ok,input),owner=(args,ok=true)=>cli('eef',args,ok);
async function response(url,body,method){return fetch(url,{method:method||(body?'POST':'GET'),headers:{'Content-Type':'application/json'},body:body?JSON.stringify(body):undefined,signal:AbortSignal.timeout(10000)});}
async function json(url,body,method){const r=await response(url,body,method);assert(r.ok,'HTTP failed; private evidence '+scratch);return r.json();}
async function until(fn,label){const end=Date.now()+30000;while(Date.now()<end){try{if(await fn())return;}catch{}await new Promise(r=>setTimeout(r,100));}throw Error('Timeout: '+label);}
async function port(){const s=tcpServer();await new Promise(r=>s.listen(0,'127.0.0.1',r));const p=s.address().port;await new Promise(r=>s.close(r));return p;}
const saved=async()=>JSON.parse(await readFile(config,'utf8'));
const show=async()=>(await local(['model-bootstrap-policy','show'])).result;
const remote=(args,ok=true)=>owner(['node','model-bootstrap-policy','--node',id,...args],ok);
const launch=()=>nodeProcess=start('eefn',['--config',config,'--no-ui']);
const assertEmpty=async()=>{const v=await saved();assert.deepEqual(v.models.ollama.selected,[]);assert.deepEqual(v.models.llamacpp.slots,[]);};
try{
 await new Promise(r=>provider.listen(0,'127.0.0.1',r));
 const [api,gateway,web]=await Promise.all([port(),port(),port()]);node=`http://127.0.0.1:${api}`;eef=`http://127.0.0.1:${web}`;
 const value=JSON.parse(await readFile(join(root,'config/node.example.json'),'utf8'));
 Object.assign(value,{node_id:id,name:'Setup fixture',connection_enabled:false,auto_local:false,local_pairing:false,endpoints:[],psk:'',
  dashboard:{enabled:true,ui_enabled:false,host:'127.0.0.1',port:api},update:{policy:'off'},
  models:{provider:'auto',ollama:{base_url:`http://127.0.0.1:${provider.address().port}`,selected:[]},llamacpp:{binary:'tools/fixture-no-model-runtime.exe',slots:[]}},
  model_catalog:[{id:'fixture',modality:'text',ollama:'fixture:latest',url:'https://example.invalid/fixture.gguf',sha256:'a'.repeat(64),bytes:4,license:'test-only',source_page:'https://example.invalid/license'}]});
 await writeFile(config,JSON.stringify(value));
 const yaml=(await readFile(join(root,'config/default_identity.yaml'),'utf8')).replace('port: 51334',`port: ${web}`).replace('port: 51335',`port: ${gateway}`).replace('policy: prompt','policy: off');await writeFile(identity,yaml);
 launch();start('eef',['--config',identity,'--database',join(scratch,'eef.db'),'--no-brain','--no-ui'],undefined,{EEF_NODE_PSK:randomBytes(24).toString('hex')});
 await until(async()=>(await json(node+'/api/diagnostics')).runtime_id,'node core startup');
 await local(['model-bootstrap-policy','configure','--mode','auto','--model','fixture']);
 assert.equal((await show()).automatic_setup_available,false);assert.equal(pulls,0);await assertEmpty();
 await until(async()=>(await json(eef+'/api/diagnostics')).initialized,'coordinator startup');
 const invitation=await owner(['credentials','provision','--node',id,'--address',`127.0.0.1:${gateway}`]);await local(['connection','pair-stdin'],true,JSON.stringify(invitation));
 const epoch=(await json(eef+'/api/diagnostics')).runtime_id;await owner(['restart']);await until(async()=>(await json(eef+'/api/diagnostics')).runtime_id!==epoch,'apply independent credentials');
 await local(['connection','resume']);await until(async()=>(await json(eef+'/api/diagnostics')).registered_node_count===1,'pinned registration');
 await remote(['configure','--enable-automation'],false);assert.equal((await show()).saved.policy.automation,undefined);assert.equal(pulls,0);
 const generic=await saved(),before=await readFile(config,'utf8');generic.model_bootstrap.automation=true;
 assert(!(await response(node+'/api/config',{config:generic},'PUT')).ok);assert.equal(await readFile(config,'utf8'),before);
 await local(['permissions','allow','remote-management']);
 await remote(['configure','--enable-automation']);
 await until(async()=>pulls===1&&(await show()).setup.attempt?.phase==='downloading','one backend-owned transfer');
 let report=await show(),first=report.setup.attempt.id;const stale=await saved();
 assert.equal(report.setup.attempt.backend,'ollama');assert.equal(report.setup.attempt.download_admitted,true);
 const beforeSetupInspection=await readFile(config,'utf8');
 const setupDuringDownload=await local(['setup']);
 assert(!setupDuringDownload.next_steps.includes('choose_interpreter'),'setup must not ask for another interpreter while the approved model setup is retained');
 assert(setupDuringDownload.next_steps.includes('inspect_model_setup'));
 assert.equal(setupDuringDownload.model_setup.status.attempt.id,first);
 assert.equal(setupDuringDownload.model_setup.status.attempt.phase,'downloading');
 assert.equal(setupDuringDownload.model_setup.status.readiness_verified,false);
 const remoteSetupDuringDownload=await owner(['node','setup','--node',id]);
 assert.equal(remoteSetupDuringDownload.success,true);
 assert.equal(remoteSetupDuringDownload.result.model_setup.status.attempt.id,first);
 assert.equal(remoteSetupDuringDownload.result.model_setup.status.attempt.phase,'downloading');
 const plainRemoteSetup=await start('eef',['--config',identity,'node','setup','--node',id]).result;
 assert.equal(plainRemoteSetup.code,0);
 assert(plainRemoteSetup.out.includes('Node-local commands below must run on the inspected node'));
 assert(plainRemoteSetup.out.includes('download checkpoint'));
 for(const field of ['changed','model_inference_requested','execution_authorized','restart_requested','automatically_retried'])assert.equal(remoteSetupDuringDownload[field],false);
 assert.equal(await readFile(config,'utf8'),beforeSetupInspection,'local and remote setup inspections never choose or configure models');
 assert.equal((await remote(['show'])).result.setup.attempt.id,first);
 await until(async()=>(await local(['models','download-status','--id',first])).download.completed===2,'existing progress channel');
 await local(['model-bootstrap-policy','configure','--disable-automation']);
 await until(async()=>(await show()).setup.attempt.phase==='stopped','revocation before selection');
 await until(async()=>(await local(['models','download-status','--id',first])).download.state==='cancelled','exact transfer cancellation observation');
 await assertEmpty();assert.equal(pulls,1);
 const disabled=await readFile(config,'utf8');assert(!(await response(node+'/api/config',{config:stale},'PUT')).ok);assert.equal(await readFile(config,'utf8'),disabled);
 await remote(['configure','--enable-automation']);
 await until(async()=>pulls===2&&(await show()).setup.attempt?.phase==='downloading','explicit new setup epoch');
 const second=(await show()).setup.attempt.id,oldRuntime=(await json(node+'/api/diagnostics')).runtime_id;
 assert.notEqual(second,first);
 // Terminate only the native node created by this fixture, while its own
 // synthetic transfer is pending. Never target an installed app/process name.
 nodeProcess.kill();await nodeProcess.result;launch();
 await until(async()=>(await json(node+'/api/diagnostics')).runtime_id!==oldRuntime&&(await show()).setup.attempt?.phase==='unknown','durable unknown after interrupted runtime');
 report=await show();assert.equal(report.setup.attempt.id,second);assert.equal(report.setup.attempt.reason,'runtime_interrupted');assert.equal(pulls,2);await assertEmpty();
 const uncertainSetup=await local(['setup']);assert.equal(uncertainSetup.model_setup.status.attempt.phase,'unknown');
 assert(uncertainSetup.next_steps.includes('inspect_model_setup'));assert(!uncertainSetup.next_steps.includes('choose_interpreter'));
 await until(async()=>(await remote(['show'])).result.runtime_id===(await json(node+'/api/diagnostics')).runtime_id,'reconnected current pinned runtime');
 // A shared provider may finish despite client cancellation/death. That is
 // not permission to resume the old setup attempt or select its model.
 for(const finish of held.splice(0))finish();assert.equal(installed,true);
 await local(['model-bootstrap-policy','configure','--disable-automation']);await remote(['configure','--enable-automation']);
 await until(async()=>(await show()).setup.attempt?.phase==='configured','reuse installed Ollama metadata without another pull');
 report=await show();assert.equal(report.setup.attempt.download_admitted,false);assert.equal(pulls,2);assert.equal(report.setup.readiness_verified,false);assert.equal(report.setup.restart_required,true);
 assert.notEqual(report.setup.attempt.id,second);let selected=await saved();assert.equal(selected.models.provider,'ollama');assert.equal(selected.models.ollama.selected[0].model_id,'fixture:latest');
 assert.deepEqual(selected.models.ollama.selected[0].selection.roles,['request_interpreter']);
 const configuredRuntime=(await json(node+'/api/diagnostics')).runtime_id;
 await local(['model-bootstrap-policy','configure','--disable-automation']);
 await local(['models','remove','--backend','ollama','--model','fixture:latest']);
 const cache=Buffer.from('GGUF synthetic cache metadata only, not inference weights'),hash=createHash('sha256').update(cache).digest('hex');
 await mkdir(join(scratch,'models'),{recursive:true});const cachePath=join(scratch,'models',hash+'.gguf');await writeFile(cachePath,cache);
 selected=await saved();selected.models.provider='llamacpp';selected.model_catalog.push({id:'cpu-fixture',modality:'text',url:'https://example.invalid/cpu.gguf',sha256:hash,bytes:cache.length,license:'test-only',source_page:'https://example.invalid/cpu-license'});
 await json(node+'/api/config',{config:selected},'PUT');
 await local(['model-bootstrap-policy','configure','--mode','auto','--model','cpu-fixture','--enable-automation']);
 await until(async()=>(await show()).setup.attempt?.phase==='configured'&&(await show()).setup.attempt.backend==='llamacpp','verified CPU cache fallback');
 report=await show();selected=await saved();assert.equal(selected.models.llamacpp.slots[0].gpu_layers,0);assert.deepEqual(selected.models.llamacpp.slots[0].selection.roles,['request_interpreter']);
 assert.deepEqual(await readFile(cachePath),cache);assert.equal(pulls,2);assert.equal(inferences,0);
 assert.equal((await json(node+'/api/diagnostics')).runtime_id,configuredRuntime,'configuration is not an automatic restart');
 const review=reviewedFixture(eef,node,id),plan=await review.save({template:'system_info',constraints:{node_id:id},params:{}});await review.start(await review.approve(plan));assert.equal((await review.wait(plan.id)).status,'completed');
 const safe=JSON.stringify(report);assert(!safe.includes('example.invalid'));assert(!safe.includes(cachePath));
 // Prompt consent authorizes one exact attempt, not a permanent move to auto.
 await local(['model-bootstrap-policy','configure','--disable-automation']);
 await local(['models','remove','--backend','llamacpp','--model','cpu-fixture']);
 await local(['models','provider','auto']);
 await remote(['configure','--mode','prompt','--model','fixture','--enable-automation']);
 assert.equal((await show()).setup.state,'awaiting_approval');
 const promptSaved=(await show()).saved,priorAttempt=(await show()).setup.attempt.id;
 await local(['permissions','deny','remote-management']);
 const pendingSetup=await owner(['node','setup','--node',id]);
 assert.equal(pendingSetup.success,true,'read-only setup inspection does not enable or require remote writes');
 assert.equal(pendingSetup.result.model_setup.status.state,'awaiting_approval');
 assert(pendingSetup.result.next_steps.includes('approve_model_setup'));
 assert(!pendingSetup.result.next_steps.includes('choose_interpreter'));
 const inspection={schema_version:1,node_id:id,expected_runtime_id:pendingSetup.result.runtime_id,request_id:randomUUID()};
 const cross=await response(eef+'/api/commands/node/setup',inspection,'POST');assert.equal(cross.status,200);
 assert.equal((await cross.json()).request_id,inspection.request_id);
 const wrongRuntime=await json(eef+'/api/commands/node/setup',{...inspection,expected_runtime_id:randomUUID(),request_id:randomUUID()});
 assert.equal(wrongRuntime.success,false);assert.equal(wrongRuntime.result,null);
 const origin=await fetch(eef+'/api/commands/node/setup',{method:'POST',headers:{'Content-Type':'application/json',Origin:'https://untrusted.example'},body:JSON.stringify(inspection)});assert.equal(origin.status,403);
 const malformed=await response(eef+'/api/commands/node/setup',{...inspection,unexpected:true},'POST');assert.equal(malformed.status,422);
 const oversized=await response(eef+'/api/commands/node/setup',{...inspection,padding:'x'.repeat(8192)},'POST');assert.equal(oversized.status,413);
 const deniedApproval=await remote(['approve'],false);assert.equal(deniedApproval.success,false);
 assert.equal((await show()).setup.attempt.id,priorAttempt);assert.equal(pulls,2);
 await local(['permissions','allow','remote-management']);
 installed=false;
 const beforeApproval=await readFile(config,'utf8'),approval=await remote(['approve']);
 assert.equal(approval.mutation_requested,true);assert.equal(approval.sending_attempted,true);
 assert.deepEqual(approval.result.saved,promptSaved);assert.equal(approval.result.setup.attempt.owner_approved,true);
 await until(async()=>pulls===3&&(await show()).setup.attempt?.phase==='downloading','one explicitly approved prompt transfer');
 assert.equal(await readFile(config,'utf8'),beforeApproval,'approval and pending transfer do not select models');
 const promptDownload=(await show()).setup.attempt.id;
 assert.equal((await local(['models','download-status','--id',promptDownload])).download.state,'downloading');
 for(const finish of held.splice(0))finish();
 await until(async()=>(await show()).setup.attempt?.phase==='configured','one explicitly approved prompt attempt');
 const promptAttempt=(await show()).setup.attempt.id;
 assert.notEqual(promptAttempt,priorAttempt);assert.equal((await show()).saved.policy.mode,'prompt');
 assert.deepEqual((await show()).saved,promptSaved);assert.equal(pulls,3);assert.equal(inferences,0);
 assert.equal((await show()).setup.attempt.download_admitted,true);
 assert.equal((await local(['models','download-status','--id',promptDownload])).download.state,'installed');
 assert.notEqual(await readFile(config,'utf8'),beforeApproval,'only the later runner, not consent, selects the model');
 const repeated=await local(['model-bootstrap-policy','approve'],false);assert.equal(repeated.success,false);
 assert.equal((await show()).setup.attempt.id,promptAttempt);assert.equal(pulls,3);
 assert.equal((await json(node+'/api/diagnostics')).runtime_id,configuredRuntime);
 // A run-only CLI selection must be protected before registration populates
 // model inventory, even though its saved selections are empty.
 await mkdir(join(scratch,'cli'));const cliConfig=join(scratch,'cli','node.json'),cliValue=structuredClone(value),cliPort=await port();
 cliValue.node_id=id+'-cli';cliValue.dashboard.port=cliPort;cliValue.model_bootstrap=structuredClone(stale.model_bootstrap);
 await writeFile(cliConfig,JSON.stringify(cliValue));
 start('eefn',['--config',cliConfig,'--no-ui','--select-model','fixture:latest=text']);
 await until(async()=>(await json(`http://127.0.0.1:${cliPort}/api/diagnostics`)).runtime_id,'run-only CLI selection startup');
 const cliShown=(await cli('eefn',['model-bootstrap-policy','show'],true,undefined,cliConfig)).result;
 assert.equal(cliShown.setup.state,'existing_setup');assert.equal(cliShown.setup.attempt,null);
 assert.deepEqual(JSON.parse(await readFile(cliConfig,'utf8')).models.ollama.selected,[]);
 try{await stat(join(scratch,'cli','node.bootstrap.sqlite3'));assert.fail('CLI choice must prevent setup evidence creation');}catch(error){if(error.code!=='ENOENT')throw error;}
 assert.equal(pulls,3);assert.equal(inferences,0);
 // Stopping one attempt preserves preferences and fences late shared-provider
 // completion. It is not a global cancel, rollback or provider shutdown claim.
 await local(['model-bootstrap-policy','configure','--disable-automation']);
 await local(['models','remove','--backend','ollama','--model','fixture:latest']);
 installed=false;
 await remote(['configure','--mode','auto','--model','fixture','--enable-automation']);
 await until(async()=>pulls===4&&(await show()).setup.attempt?.phase==='downloading','local-stop attempt');
 const localStopId=(await show()).setup.attempt.id;
 await local(['model-bootstrap-policy','stop','--attempt',randomUUID()],false);
 await local(['permissions','deny','remote-management']);
 await remote(['stop','--attempt',localStopId],false);
 assert.equal((await show()).setup.attempt.phase,'downloading','remote denial must not stop a local attempt');
 const beforeStop=await readFile(config,'utf8'),beforeStopPolicy=(await show()).saved;
 const stopped=await local(['model-bootstrap-policy','stop','--attempt',localStopId]);
 assert.equal(stopped.success,true);assert.equal(stopped.result.changed,true);
 assert.deepEqual(stopped.result.saved,beforeStopPolicy);
 assert.equal(stopped.result.setup.attempt.phase,'stopped');
 assert.equal(stopped.result.setup.attempt.reason,'owner_stopped');
 assert.equal(stopped.result.setup_stop.attempt_id,localStopId);
 assert.equal(stopped.result.setup_stop.transfer_cancellation,'requested');
 assert.equal(stopped.result.setup_stop.physical_stop_verified,false);
 assert.equal(stopped.result.setup_stop.configuration_rolled_back,false);
 assert.equal(await readFile(config,'utf8'),beforeStop,'stopping does not toggle policy or save model choices');
 await until(async()=>(await local(['models','download-status','--id',localStopId])).download.state==='cancelled','local exact transfer cancellation');
 for(const finish of held.splice(0))finish();
 await local(['model-bootstrap-policy','stop','--attempt',localStopId],false);
 await local(['model-bootstrap-policy','configure','--mode','auto']);
 assert.equal((await show()).setup.attempt.reason,'owner_stopped');
 assert.equal(pulls,4);await assertEmpty();
 // A new explicit epoch can be stopped through the same authenticated eef
 // adapter, but never without the target node's current management consent.
 await local(['permissions','allow','remote-management']);
 await remote(['configure','--disable-automation']);installed=false;
 await remote(['configure','--enable-automation']);
 await until(async()=>pulls===5&&(await show()).setup.attempt?.phase==='downloading','remote-stop attempt');
 const remoteStopId=(await show()).setup.attempt.id,remoteBefore=await readFile(config,'utf8');
 const remoteStopped=await remote(['stop','--attempt',remoteStopId]);
 assert.equal(remoteStopped.success,true);assert.equal(remoteStopped.mutation_requested,true);
 assert.equal(remoteStopped.result.setup_stop.attempt_id,remoteStopId);
 assert.equal(remoteStopped.result.setup_stop.transfer_cancellation,'requested');
 assert.equal(remoteStopped.result.setup_stop.physical_stop_verified,false);
 assert.equal(remoteStopped.result.setup.attempt.reason,'owner_stopped');
 await until(async()=>(await local(['models','download-status','--id',remoteStopId])).download.state==='cancelled','remote exact transfer cancellation');
 for(const finish of held.splice(0))finish();
 await remote(['stop','--attempt',remoteStopId],false);
 assert.equal(await readFile(config,'utf8'),remoteBefore);await assertEmpty();
 assert.equal(pulls,5);assert.equal(inferences,0);
 await writeFile(join(scratch,'results.json'),JSON.stringify({passed:true,physical_two_pc:false,synthetic_provider_and_cache_only:true,real_weights_downloaded:false,inference_requested:false,
  legacy_preferences_inactive:true,local_and_pinned_remote_commands:true,remote_consent_required:true,generic_activation_and_stale_restore_rejected:true,exact_transfer_revocation:true,
  durable_unknown_after_process_exit:true,no_automatic_replay:true,existing_ollama_reused:true,verified_cpu_cache_configured:true,no_automatic_restart:true,applied_command_line_selection_preserved:true,core_reviewed_work:true,
  explicit_prompt_approval:true,prompt_install_receipt_progress_and_completion:true,prompt_mode_and_policy_epoch_preserved:true,remote_prompt_approval_requires_current_consent:true,repeated_prompt_approval_refused:true,
  shared_setup_download_and_unknown_guidance:true,coordinator_setup_inspection_without_remote_write_consent:true,setup_approval_guidance_without_approval_or_inference:true,setup_runtime_origin_and_body_fences:true,
  attempt_specific_local_and_remote_stop:true,stop_preserves_policy_and_configuration:true,stop_requires_current_remote_consent:true,late_provider_completion_does_not_select_or_replay:true,repeated_stop_refused:true},null,2));
 console.log('Model setup native validation passed: '+scratch);
}catch(error){await writeFile(join(scratch,'PRIVATE-failure.json'),JSON.stringify({message:String(error)},null,2));await writeFile(join(scratch,'PRIVATE-logs.json'),JSON.stringify(children.map(c=>({exe:c.spawnfile,code:c.exitCode,...c.logs()})),null,2));throw error;}
finally{for(const c of children)if(c.exitCode===null)c.kill();await Promise.allSettled(children.map(c=>c.result));for(const finish of held)finish();provider.closeAllConnections();await new Promise(r=>provider.close(r));}
