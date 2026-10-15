// Real headless apps, two logical nodes on one PC. No models or installed apps.
import assert from 'node:assert/strict';
import {mkdtemp,readFile,writeFile} from 'node:fs/promises';
import {join,resolve} from 'node:path';
import {spawn} from 'node:child_process';
import {createServer} from 'node:net';
import {createServer as createHttpServer} from 'node:http';
import {randomUUID} from 'node:crypto';
const root=resolve(import.meta.dirname,'..'),bin=resolve(process.env.EEF_TEST_BINARY_DIR||join(root,'target/debug'));
const scratch=await mkdtemp(join(root,'.validation/local-enrollment-')),children=[];
const env={...process.env,EEF_NODE_PSK:'',APPDATA:join(scratch,'appdata'),EEF_DISCOVERY_DIR:join(scratch,'discovery'),
  PATH:join(root,'.tooling/llvm-mingw-20260616-ucrt-x86_64/bin')+';'+process.env.PATH};
function start(name,args){const child=spawn(join(bin,name+'.exe'),args,{cwd:root,windowsHide:true,env,stdio:['ignore','pipe','pipe']});
 let out='',err='';child.stdout.on('data',b=>out=(out+b).slice(-1048576));child.stderr.on('data',b=>err=(err+b).slice(-1048576));
 child.result=new Promise((r,j)=>{child.on('error',j);child.on('close',code=>r({code,out,err}));});children.push(child);return child;}
async function cli(name,config,args,ok=true,json=true){const c=start(name,['--config',config,...(json?['--json']:[]),...args]);
 const timer=setTimeout(()=>c.kill(),100000);try{const r=await c.result;assert.equal(r.code,ok?0:1,'CLI failed; private evidence '+scratch);
 return json?JSON.parse(r.out):r.out;}finally{clearTimeout(timer);}}
async function port(){const s=createServer();await new Promise(r=>s.listen(0,'127.0.0.1',r));const p=s.address().port;await new Promise(r=>s.close(r));return p;}
async function json(url,body){const r=await fetch(url,{method:body?'POST':'GET',headers:{'Content-Type':'application/json'},
 body:body?JSON.stringify(body):undefined,signal:AbortSignal.timeout(10000)});assert(r.ok,'HTTP failed; private evidence '+scratch);return r.json();}
async function until(fn,label){const deadline=Date.now()+60000;while(Date.now()<deadline){try{if(await fn())return;}catch{}
 await new Promise(r=>setTimeout(r,150));}throw Error('Timeout: '+label);}
const eefConfig=join(scratch,'eef.yaml');let phase='startup';
try{
 const [web,gateway]=await Promise.all([port(),port()]),base=`http://127.0.0.1:${web}`;
 await writeFile(eefConfig,JSON.stringify({web:{host:'127.0.0.1',port:web},node:{host:'127.0.0.1',port:gateway,psk:'',authentication:{allow_legacy:false}},update:{policy:'off'}}));
 start('eef',['--config',eefConfig,'--database',join(scratch,'eef.db'),'--no-brain','--no-ui']);
 await until(async()=>(await json(base+'/api/diagnostics')).initialized,'coordinator startup');
 const owner=args=>cli('eef',eefConfig,args),nodes=[];
 for(const paused of [true,false]){
  phase=paused?'paused_enrollment':'second_enrollment';
  const config=join(scratch,paused?'a.json':'b.json'),api=await port(),id='device-'+randomUUID();
  const initial={node_id:id,name:'Enrollment fixture '+nodes.length,connection_enabled:!paused,psk:'',endpoints:[],auto_local:false,
   dashboard:{enabled:true,host:'127.0.0.1',port:api},permissions:{remote_updates:false,remote_management:false},
   models:{provider:'llamacpp',llamacpp:{slots:[]}},update:{policy:'off'},custom:{keep:true}};
  await writeFile(config,JSON.stringify(initial));start('eefn',['--config',config,'--no-ui']);
  const url=`http://127.0.0.1:${api}`,node={config,id,url};nodes.push(node);
  await until(async()=>(await json(url+'/api/status')).node_id===id,'node owner API');
  if(paused){
   const before=await readFile(config),coordinator=await readFile(eefConfig);
   const refused=await cli('eefn',config,['setup','--enroll-local'],false);
   assert.equal(refused.error_code,'restart_consent_required');assert.equal(refused.outcome_unknown,false);
   assert.equal(refused.coordinator_prepare_attempted,false);assert.equal(refused.node_saved,false);
   assert((await readFile(config)).equals(before),'no-consent enrollment must not write node settings');
   assert((await readFile(eefConfig)).equals(coordinator),'no-consent enrollment must not provision trust');
  }
  const previous=(await json(url+'/api/diagnostics')).runtime_id,oldCoordinator=(await owner(['diagnostics'])).runtime_id;
  const report=await cli('eefn',config,['setup','--enroll-local','--restart-coordinator']);
  assert.equal(report.success,true);assert.equal(report.coordinator_prepared,true);assert.equal(report.coordinator_changed,true);
  assert.equal(report.coordinator_restart_requested,true);assert.equal(report.coordinator_applied_verified,true);
  assert.equal(report.node_saved,true);assert.equal(report.node_restart_requested,true);assert.equal(report.connection_enabled,!paused);
  assert.equal(report.registration_verified,false);assert.equal(report.execution_authorized,false);assert.equal(report.automatically_retried,false);
  assert.notEqual((await owner(['diagnostics'])).runtime_id,oldCoordinator);
  await until(async()=>{const d=await json(url+'/api/diagnostics');return d.runtime_id!==previous&&(paused?d.connection_state==='paused':d.connection_state==='connected');},'node restart');
  const saved=JSON.parse(await readFile(config,'utf8'));assert.equal(saved.node_id,id);assert.equal(saved.auth_mode,'node_credential');
  assert.equal(saved.connection_enabled,!paused);assert.deepEqual(saved.permissions,initial.permissions);
  assert.deepEqual(saved.models,initial.models);assert.deepEqual(saved.custom,initial.custom);
  assert(!JSON.stringify(report).includes(saved.psk),'public report must not expose private credentials');
  node.key=saved.psk;node.certificate=saved.coordinator_certificate;
  if(paused){
   assert.equal((await owner(['diagnostics'])).registered_node_count,0);
   assert.equal((await json(url+'/api/diagnostics')).registration,'not_connected');
   const plain=await cli('eefn',config,['setup'],true,false);assert(plain.includes('paused'));
   await cli('eefn',config,['connection','resume']);
  }
  await until(async()=>(await owner(['diagnostics'])).registered_node_count===nodes.length,'private registration');
  await until(async()=>(await json(url+'/api/diagnostics')).registration==='accepted','node received current registration acknowledgement');
  const setup=await cli('eefn',config,['setup']);assert.equal(setup.registration,'accepted');
  assert.equal(setup.model_inference_requested,false);assert.equal(setup.execution_authorized,false);
 }
 assert(nodes[0].key!==nodes[1].key,'each node needs independent credentials');assert.equal(nodes[0].certificate,nodes[1].certificate);
 phase='existing_trust_reuse';
 const coordinatorRuntime=(await owner(['diagnostics'])).runtime_id;
 const reuse=await cli('eefn',nodes[1].config,['setup','--enroll-local']);
 assert.equal(reuse.success,true);assert.equal(reuse.coordinator_changed,false);assert.equal(reuse.coordinator_restart_requested,false);
 assert.equal((await owner(['diagnostics'])).runtime_id,coordinatorRuntime);
 assert(JSON.parse(await readFile(nodes[1].config,'utf8')).psk===nodes[1].key,'enrollment must not rotate an existing credential');
 await until(async()=>(await owner(['diagnostics'])).registered_node_count===2,'reused node reconnect');
 phase='revocation';await owner(['credentials','revoke','--node',nodes[1].id]);
 const before=await readFile(nodes[1].config),coordinator=await readFile(eefConfig);
 const revoked=await cli('eefn',nodes[1].config,['setup','--enroll-local','--restart-coordinator'],false);
 assert.equal(revoked.error_code,'revoked');assert.equal(revoked.coordinator_prepare_attempted,false);
 assert((await readFile(nodes[1].config)).equals(before));assert((await readFile(eefConfig)).equals(coordinator));
 phase='owner_boundaries';
 const plan=await cli('eefn',nodes[0].config,['setup','--local']);
 const request={schema_version:1,expected_node_id:nodes[0].id,expected_runtime_id:plan.runtime_id,request_id:randomUUID(),
  expected_coordinator_runtime_id:plan.coordinator_runtime_id,expected_fingerprint:plan.coordinator_fingerprint,allow_coordinator_restart:true};
 for(const [body,status,origin] of [[{...request,rotate:true},422,null],[request,403,'https://untrusted.example']]){
  const r=await fetch(nodes[0].url+'/api/commands/setup/enroll',{method:'POST',headers:{'Content-Type':'application/json',...(origin?{Origin:origin}:{})},body:JSON.stringify(body)});
  assert.equal(r.status,status);
 }
 const restart={schema_version:1,expected_runtime_id:coordinatorRuntime,expected_authentication_digest:'0'.repeat(64)};
 assert.equal((await fetch(base+'/api/commands/setup/restart',{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify(restart)})).status,400);
 phase='lost_owner_reply';
 const marker=nodes[0].config+'.api.json',markerBytes=await readFile(marker);
 const markerValue=JSON.parse(markerBytes),previousNode=(await json(nodes[0].url+'/api/diagnostics')).runtime_id;
 let enrollCalls=0,realEnrollmentSucceeded=false;
 const proxy=createHttpServer(async(req,res)=>{
  try{
   let body='';for await(const part of req)body+=part;
   const response=await fetch(nodes[0].url+req.url,{method:req.method,headers:{'Content-Type':'application/json'},
    body:body||undefined,signal:AbortSignal.timeout(75000)});
   const value=await response.json();
   if(req.url==='/api/commands/setup/enroll'){
    enrollCalls++;realEnrollmentSucceeded=value.success===true;res.destroy();return;
   }
   res.writeHead(response.status,{'Content-Type':'application/json'});res.end(JSON.stringify(value));
  }catch{res.destroy();}
 });
 await new Promise(r=>proxy.listen(0,'127.0.0.1',r));
 try{
  await writeFile(marker,JSON.stringify({...markerValue,address:`127.0.0.1:${proxy.address().port}`}));
  const lost=await cli('eefn',nodes[0].config,['setup','--enroll-local','--restart-coordinator'],false);
  assert.equal(realEnrollmentSucceeded,true);assert.equal(enrollCalls,1,'lost reply must not replay enrollment');
  assert.equal(lost.outcome_unknown,true);assert.equal(lost.node_saved,null);assert.equal(lost.node_restart_requested,null);
  assert.equal(lost.coordinator_changed,null);assert.equal(lost.coordinator_restart_requested,null);
  assert.equal(lost.automatically_retried,false);assert(!JSON.stringify(lost).includes(nodes[0].key));
  await until(async()=>{const d=await json(nodes[0].url+'/api/diagnostics');return d.runtime_id!==previousNode&&d.connection_state==='connected';},'actual restart despite dropped reply');
  assert.equal(enrollCalls,1);
 }finally{await writeFile(marker,markerBytes);await new Promise(r=>proxy.close(r));} // Fixture injection only.
 await writeFile(join(scratch,'results.json'),JSON.stringify({passed:true,two_logical_nodes:true,physical_two_pc:false,
  port_free_private_enrollment:true,no_secret_file_handoff:true,explicit_coordinator_restart_consent:true,paused_preference_preserved:true,
  private_registration_observed:true,independent_credentials:true,existing_trust_reused_without_rotation:true,
  revocation_not_restored:true,permissions_models_identity_preserved:true,owner_and_authentication_digest_boundaries:true,
  lost_owner_reply_unknown_without_replay:true,registration_ack_observed_without_inference:true,
  model_downloads:false,real_model_inference:false,installed_apps_changed:false},null,2));
 console.log('Local enrollment validation passed: '+scratch);
}catch(error){await writeFile(join(scratch,'failure.json'),JSON.stringify({passed:false,phase},null,2));throw error;}
finally{for(const child of children)if(child.exitCode===null)child.kill();await Promise.allSettled(children.map(c=>c.result));}
