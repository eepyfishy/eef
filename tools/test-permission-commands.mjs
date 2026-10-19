// Isolated command-first permission acceptance, no models or personal config.
import assert from 'node:assert/strict';
import {mkdtemp,readFile,writeFile,mkdir,stat} from 'node:fs/promises';
import {join,resolve} from 'node:path';
import {spawn} from 'node:child_process';
import {createServer as tcpServer} from 'node:net';
import {createServer} from 'node:http';
import {randomBytes,randomUUID} from 'node:crypto';
import {reviewedFixture} from './reviewed-fixture.mjs';
const root=resolve(import.meta.dirname,'..'),bin=resolve(process.env.EEF_TEST_BINARY_DIR||join(root,'target/debug'));
const scratch=await mkdtemp(join(root,'.validation/permission-commands-')),children=[];
const nodeConfig=join(scratch,'node.json'),eefConfig=join(scratch,'eef.yaml'),id='permissions-fixture';let node,eef,proxyMode='drop',writes=0;
const env={...process.env,EEF_NODE_PSK:'',APPDATA:join(scratch,'appdata'),EEF_DISCOVERY_DIR:join(scratch,'discovery'),PATH:join(root,'.tooling/llvm-mingw-20260616-ucrt-x86_64/bin')+';'+process.env.PATH};
const proxy=createServer(async(req,res)=>{
 try{let body='';for await(const part of req)body+=part;const incoming=body&&JSON.parse(body),mutation=['set','configure'].includes(incoming?.command?.operation);
  if(mutation){writes++;if(proxyMode==='old'){res.writeHead(404);return res.end('{}');}}
  const response=await fetch(node+req.url,{method:req.method,headers:{'Content-Type':'application/json'},body:body||undefined,signal:AbortSignal.timeout(10000)});const value=await response.json();
  if(mutation){if(proxyMode==='drop')return res.destroy();if(proxyMode==='corrupt')value.execution_authorized=true;if(proxyMode==='prose')value.note='PRIVATE-MUST-NOT-ECHO';}
  res.writeHead(response.status,{'Content-Type':'application/json'});res.end(JSON.stringify(value));
 }catch{res.destroy();}
});
async function port(){const s=tcpServer();await new Promise(r=>s.listen(0,'127.0.0.1',r));const p=s.address().port;await new Promise(r=>s.close(r));return p;}
function launch(name,args,extra={}){const c=spawn(join(bin,name+'.exe'),args,{cwd:root,windowsHide:true,env:{...env,...extra},stdio:['ignore','pipe','pipe']});let out='',err='';c.stdout.on('data',b=>out=(out+b).slice(-1048576));c.stderr.on('data',b=>err=(err+b).slice(-1048576));c.logs=()=>({out,err});c.result=new Promise((r,j)=>{c.on('error',j);c.on('close',code=>r({code,out,err}));});children.push(c);return c;}
async function cli(name,args,ok=true,plain=false){const c=launch(name,['--config',name==='eefn'?nodeConfig:eefConfig,...(plain?[]:['--json']),...args]),timer=setTimeout(()=>c.kill(),30000);try{const r=await c.result;assert.equal(r.code,ok?0:1,'CLI failed; private '+scratch);return plain?r:JSON.parse(r.out);}finally{clearTimeout(timer);}}
const local=(args,ok=true,plain=false)=>cli('eefn',args,ok,plain),owner=(args,ok=true,plain=false)=>cli('eef',args,ok,plain);
const remote=(args,ok=true,plain=false)=>owner(['node','permissions','--node',id,...args],ok,plain);
async function json(url,body){const r=await fetch(url,{method:body?'POST':'GET',headers:{'Content-Type':'application/json'},body:body?JSON.stringify(body):undefined,signal:AbortSignal.timeout(10000)});const v=await r.json();assert(r.ok,'HTTP failed; private '+scratch);return v;}
async function until(fn,label){const end=Date.now()+30000;while(Date.now()<end){try{if(await fn())return;}catch{}await new Promise(r=>setTimeout(r,100));}throw Error('Timeout: '+label);}
async function exists(path){try{await stat(path);return true;}catch{return false;}}
function feature(report,name){return report.features.find(row=>row.feature===name);}
const show=async()=>(await local(['permissions','show'])).result;
try{
 await new Promise(r=>proxy.listen(0,'127.0.0.1',r));const [api,gateway,web]=await Promise.all([port(),port(),port()]);node=`http://127.0.0.1:${api}`;eef=`http://127.0.0.1:${web}`;
 const config=JSON.parse(await readFile(join(root,'config/node.example.json'),'utf8'));config.node_id=id;config.dashboard.port=api;config.update.policy='off';config.models.provider='llamacpp';config.models.ollama.base_url='http://127.0.0.1:1';config.custom={preserve:true};
 config.permissions.filesystem.max_write_bytes=8192;await writeFile(nodeConfig,JSON.stringify(config));
 const yaml=(await readFile(join(root,'config/default_identity.yaml'),'utf8')).replace('port: 51334',`port: ${web}`).replace('port: 51335',`port: ${gateway}`).replace('policy: prompt','policy: off');await writeFile(eefConfig,yaml);
 launch('eef',['--config',eefConfig,'--database',join(scratch,'eef.db'),'--no-brain','--no-ui'],{EEF_NODE_PSK:randomBytes(32).toString('hex')});launch('eefn',['--config',nodeConfig,'--no-ui']);
 await until(async()=>(await json(node+'/api/status')).connection.state==='connected','connected without any model');
 const initial=await show();assert(initial.features.every(row=>row.saved==='disabled'));assert((await local(['permissions','show'],true,true)).out.includes('Node permissions: '+id));
 const allowed=(await local(['permissions','allow','filesystem-write'])).result;assert.equal(feature(allowed,'filesystem_write').saved,'feature_wide');assert.equal(feature(allowed,'filesystem_write').applied,'disabled');assert.equal(allowed.restart_required,true);assert.equal(allowed.restart_requested,false);
 const noop=(await local(['permissions','allow','filesystem-write'])).result;assert.equal(noop.changed,false);
 let saved=JSON.parse(await readFile(nodeConfig,'utf8'));assert.deepEqual(saved.permissions.filesystem.roots,[]);assert.equal(saved.permissions.filesystem.max_write_bytes,8192);assert.equal(saved.custom.preserve,true);assert.deepEqual(saved.models,config.models);
 const before=await readFile(nodeConfig,'utf8');const denied=await remote(['allow','filesystem-read'],false);assert.equal(denied.success,false);assert.equal(await readFile(nodeConfig,'utf8'),before);
 await local(['permissions','allow','remote-management']);await remote(['allow','filesystem-read']);assert.equal(feature(await show(),'filesystem_read').saved,'feature_wide');
 assert((await remote(['show'],true,true)).out.includes('Node permissions: '+id));
 await remote(['deny','remote-management']);const revoked=await readFile(nodeConfig,'utf8');await remote(['allow','remote-management'],false);assert.equal(await readFile(nodeConfig,'utf8'),revoked);
 await local(['permissions','deny','filesystem-read']);await local(['permissions','allow','remote-management']);
 const stale={schema_version:1,expected_node_id:id,expected_runtime_id:initial.runtime_id,command:{operation:'set',feature:'camera',enabled:true,expected_digest:initial.saved_digest}};
 let r=await fetch(node+'/api/commands/permissions',{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify(stale)});assert.equal(r.status,400);
 const inspected=await show(),request={...stale,expected_runtime_id:inspected.runtime_id,command:{...stale.command,expected_digest:inspected.saved_digest}};
 r=await fetch(node+'/api/commands/permissions',{method:'POST',headers:{'Content-Type':'application/json',Origin:'https://untrusted.example'},body:JSON.stringify(request)});assert.equal(r.status,403);
 r=await fetch(node+'/api/commands/permissions',{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify({...request,padding:'x'.repeat(4096)})});assert.equal(r.status,413);
 for(const mode of ['drop','corrupt','prose','old']){
  await local(['permissions','deny','http-requests']);const marker=nodeConfig+'.api.json',original=await readFile(marker,'utf8'),endpoint=JSON.parse(original);endpoint.address=`127.0.0.1:${proxy.address().port}`;proxyMode=mode;const count=writes;
  await writeFile(marker,JSON.stringify(endpoint));try{const value=await local(['permissions','allow','http-requests'],false);assert.equal(value.outcome_unknown,true);assert.equal(value.automatically_retried,false);assert.equal(writes,count+1);assert(!JSON.stringify(value).includes('PRIVATE-MUST-NOT-ECHO'));}finally{await writeFile(marker,original);}
  assert.equal(feature(await show(),'http_requests').saved,mode==='old'?'disabled':'feature_wide');
 }
 // Relay configuration has the same no-retry rule even though it does not
 // grant permissions or execute work. Corrupt/lost responses must not allocate
 // replacement resources or silently write twice.
 const relaySetup=(await local(['relay','configure','--microphone'])).result;
 const microphoneId=relaySetup.saved_resources.find(row=>row.capability==='audio.capture').id.split('::')[1];
 const relayPermissions=JSON.parse(await readFile(nodeConfig,'utf8')).permissions;
 for(const [index,mode] of ['drop','corrupt','prose','old'].entries()){
  const before=JSON.parse(await readFile(nodeConfig,'utf8')),marker=nodeConfig+'.api.json',original=await readFile(marker,'utf8'),endpoint=JSON.parse(original);endpoint.address=`127.0.0.1:${proxy.address().port}`;proxyMode=mode;const count=writes;
  await writeFile(marker,JSON.stringify(endpoint));
  try{const value=await local(['relay','configure','--microphone','--microphone-device',String(index)],false);assert.equal(value.outcome_unknown,true);assert.equal(value.automatically_retried,false);assert.equal(writes,count+1);assert(!JSON.stringify(value).includes('PRIVATE-MUST-NOT-ECHO'));}
  finally{await writeFile(marker,original);}
  const after=JSON.parse(await readFile(nodeConfig,'utf8'));
  assert.deepEqual(after.permissions,relayPermissions,'uncertain setup never grants permissions');
  assert.deepEqual(after.metadata.resources.map(r=>r.id),before.metadata.resources.map(r=>r.id));
  const microphone=after.metadata.resources.find(r=>r.id===microphoneId),previous=before.metadata.resources.find(r=>r.id===microphoneId);
  assert.equal(microphone.parameters?.device,mode==='old'?previous.parameters?.device:index);
  assert.equal((await local(['relay','show'])).result.runtime_id,relaySetup.runtime_id,'uncertain setup never restarts');
 }
 await local(['permissions','deny','http-requests']);await local(['permissions','allow','process-execution']);await local(['restart']);
 await until(async()=>(await json(node+'/api/status')).connection.state==='connected','applied features reconnected');
 assert.equal(feature(await show(),'filesystem_write').applied,'feature_wide');assert.equal(feature(await show(),'process_execution').applied,'feature_wide');
 const invitation=await owner(['credentials','provision','--node',id,'--address',`127.0.0.1:${gateway}`]);const privateFile=join(scratch,'PRIVATE-pair.json');await writeFile(privateFile,JSON.stringify(invitation));const old=(await json(eef+'/api/diagnostics')).runtime_id;await owner(['restart']);await until(async()=>(await json(eef+'/api/diagnostics')).runtime_id!==old,'coordinator private identity');await local(['connection','pair','--code-file',privateFile]);
 await until(async()=>{const status=await json(node+'/api/status');return status.connection.state==='connected'&&status.connection.transport==='pinned_tls13'&&status.execution_grants.available===true&&status.input_channel==='ready';},'connected private node with ready input and configured receipt support');
 const review=reviewedFixture(eef,node,id),effect=join(scratch,'approved-write.txt');
 const plan=await review.save({template:'local_pipeline',constraints:{node_id:id},params:{pipeline:{schema_version:1,max_retained_bytes:2048,steps:[{id:'write',capability:'filesystem',action:'write',params:{path:effect,content:'owner-approved fixture write'}}]}}});
 await review.start(await review.approve(plan));const report=await review.result(plan.id);assert.equal(report.success,true);assert.equal(await readFile(effect,'utf8'),'owner-approved fixture write');
 if(process.platform==='win32'){
  const helper=join(bin,'examples/process-fixture.exe'),native=join(scratch,'native'),blocked=join(scratch,'revoked-write.txt');await mkdir(native);
  const pending=await review.save({template:'local_pipeline',constraints:{node_id:id},params:{pipeline:{schema_version:1,max_retained_bytes:8192,steps:[
   {id:'gate',capability:'process.exec',action:'run',params:{program:helper,args:['complete',native],timeout_seconds:20}},
   {id:'write',capability:'filesystem',action:'write',params:{path:blocked,content:'must not run after revocation'}}]}}});
  await review.start(await review.approve(pending,{max_execution_ms:10000,max_result_bytes:16384}));await until(()=>exists(join(native,'pids.json')),'owned native stage started');
  const observer=launch('examples/process-fixture',['observe',native]);await until(()=>exists(join(native,'observer-ready')),'native handles held');
  await local(['permissions','deny','filesystem-write']);const observed=await observer.result;assert.equal(observed.code,0);const proof=JSON.parse(observed.out);
  assert.equal(proof.both_exited,true);assert.equal(proof.held_creation_verified_handles,true);assert(proof.observed_after_ms<2000,'Permission loss must clean up before the 10-second grant deadline');
  const deniedReport=await review.result(pending.id);assert.equal(deniedReport.state,'unknown');assert.equal(deniedReport.error_code,'local_policy_changed');assert.equal(await exists(blocked),false);
  await writeFile(join(scratch,'native-revocation-results.json'),JSON.stringify({passed:true,physical_two_pc:false,proof,
   deadline_ms:10000,parent_report_state:deniedReport.state,error_code:deniedReport.error_code,later_write_absent:true,external_effects_stop_confirmed:false},null,2));
 }
 saved=JSON.parse(await readFile(nodeConfig,'utf8'));assert.deepEqual(saved.permissions.filesystem.roots,[]);assert.deepEqual(saved.permissions.process.allowed_executables,[]);assert.equal(saved.permissions.filesystem.max_write_bytes,8192);assert.equal(saved.custom.preserve,true);
 await writeFile(join(scratch,'results.json'),JSON.stringify({passed:true,physical_two_pc:false,no_models:true,no_ui_controller:true,plain_text_and_json:true,
  no_per_object_allowlists:true,byte_limits_and_unrelated_settings_preserved:true,saved_applied_separation:true,remote_owner_approval_and_revocation:true,
  stale_digest_and_cross_origin_and_body_bounds:true,lost_corrupt_unsupported_write_no_retry_or_fallback:true,reviewed_real_filesystem_effect:true,
  native_pending_work_revocation:process.platform==='win32',uncertain_relay_configuration_no_retry_or_resource_reallocation:true,
  administrator_bypass:false,automatic_restart:false,automatic_permissions:false},null,2));
 console.log('Permission command validation passed: '+scratch);
}catch(error){await writeFile(join(scratch,'PRIVATE-failure.json'),JSON.stringify({message:String(error)},null,2));await writeFile(join(scratch,'PRIVATE-logs.json'),JSON.stringify(children.map(c=>({exe:c.spawnfile,code:c.exitCode,...c.logs()})),null,2));throw error;
}finally{for(const c of children)if(c.exitCode===null)c.kill();await Promise.allSettled(children.map(c=>c.result));proxy.closeAllConnections();await new Promise(r=>proxy.close(r));}
