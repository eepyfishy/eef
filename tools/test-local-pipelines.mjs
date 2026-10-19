// Public command-driven finite dataflow through two separately authenticated
// logical nodes on one PC. No actual LM/media, downloads, UI or physical-network proof.
import assert from 'node:assert/strict';
import {mkdtemp,readFile,writeFile,mkdir,stat,unlink} from 'node:fs/promises';
import {writeFileSync,appendFileSync} from 'node:fs';
import {join,resolve,basename} from 'node:path';
import {spawn} from 'node:child_process';
import {createServer as tcpServer} from 'node:net';
import {createServer} from 'node:http';
import {randomBytes,randomUUID} from 'node:crypto';
import {reviewedFixture} from './reviewed-fixture.mjs';
const root=resolve(import.meta.dirname,'..'),bin=resolve(process.env.EEF_TEST_BINARY_DIR||join(root,'target/debug'));
const helper=join(bin,'examples/process-fixture.exe');await stat(helper);
const scratch=await mkdtemp(join(root,'.validation/local-pipelines-')),children=[],nodes=[],calls=[],gates=new Set();
let phase='fixture-created';
// Native fail-fast exits may never run JS exit handlers. Keep bounded private
// phase evidence as it happens; diagnostic reports must not include env/secrets.
process.report.directory=scratch;process.report.reportOnFatalError=true;
process.report.excludeEnv=true;process.report.excludeNetwork=true;
function checkpoint(next){phase=next;appendFileSync(join(scratch,'PRIVATE-phase-checkpoints.jsonl'),JSON.stringify({phase,at:new Date().toISOString(),runner_version:process.version,runner:process.execPath,runner_pid:process.pid,binary_dir:bin,
 children:children.slice(-16).map(c=>({exe:basename(c.spawnfile),pid:c.pid,code:c.exitCode}))})+'\n');}
checkpoint(phase);
process.on('exit',code=>{try{writeFileSync(join(scratch,'PRIVATE-harness-exit.json'),JSON.stringify({code,phase,children:children.map(c=>({exe:c.spawnfile,code:c.exitCode,...c.logs()}))}));}catch{}});
const eefConfig=join(scratch,'eef.yaml'),work=join(scratch,'work');let eef;
const env={...process.env,EEF_NODE_PSK:'',APPDATA:join(scratch,'appdata'),EEF_DISCOVERY_DIR:join(scratch,'discovery'),
 PATH:join(root,'.tooling/llvm-mingw-20260616-ucrt-x86_64/bin')+';'+process.env.PATH};
const fake=createServer(async(req,res)=>{
 res.setHeader('Content-Type','application/json');if(req.url==='/api/tags')return res.end(JSON.stringify({models:[{name:'selected'}]}));
 if(req.url!=='/api/chat'){res.writeHead(409);return res.end('{}');}
 let body='';for await(const chunk of req){body+=chunk;assert(body.length<65536);}const value=JSON.parse(body);calls.push(value);
 assert.equal(value.model,'selected');const prompt=value.messages.at(-1).content;
 if(prompt==='gated pipeline input'){gates.add(res);res.on('close',()=>gates.delete(res));res.write(' ');return;}
 res.end(JSON.stringify({message:{content:'synthetic local result: '+prompt},done:true,done_reason:'stop'}));
});
function spawnCommand(exe,args,input,extra={},cwd=root) {
 const child=spawn(exe,args,{cwd,windowsHide:true,env:{...env,...extra},stdio:[input===undefined?'ignore':'pipe','pipe','pipe']});
 let out='',err='';child.stdout.on('data',b=>out=(out+b).slice(-1048576));child.stderr.on('data',b=>err=(err+b).slice(-1048576));child.logs=()=>({out,err});
 child.result=new Promise((r,j)=>{child.on('error',j);child.on('close',code=>r({code,out,err}));});children.push(child);
 if(input!==undefined){child.stdin.on('error',()=>{});child.stdin.end(input);}return child;
}
const launch=(name,config,args,input,extra,cwd)=>spawnCommand(join(bin,name+'.exe'),['--config',config,...args],input,extra,cwd);
async function cli(name,config,args,input,ok=true){const child=launch(name,config,['--json',...args],input),timer=setTimeout(()=>child.kill(),70000);
 try{const r=await child.result;assert.equal(r.code,ok?0:1,'CLI failed; private '+scratch);return JSON.parse(r.out);}finally{clearTimeout(timer);}}
const owner=args=>cli('eef',eefConfig,args),local=(node,args,input)=>cli('eefn',node.config,args,input);
async function json(url,body,method){const r=await fetch(url,{method:method||(body?'POST':'GET'),headers:{'Content-Type':'application/json'},body:body?JSON.stringify(body):undefined,signal:AbortSignal.timeout(15000)});const value=await r.json();assert(r.ok,'HTTP failed; private '+scratch);return value;}
async function until(fn,label){const end=Date.now()+45000;while(Date.now()<end){try{if(await fn())return;}catch{}await new Promise(r=>setTimeout(r,100));}throw Error('Timeout: '+label);}
async function port(){const s=tcpServer();await new Promise(r=>s.listen(0,'127.0.0.1',r));const p=s.address().port;await new Promise(r=>s.close(r));return p;}
const exists=path=>stat(path).then(()=>true,()=>false);
try{
 const python=process.env.EEF_TEST_PYTHON, cameraPlugin=join(scratch,'synthetic-camera.py'),micPlugin=join(scratch,'synthetic-mic.py'),speakerPlugin=join(scratch,'synthetic-speaker.py');
 let audioAddress=null;
 if(python){
  await stat(python);
  assert.equal(await exists(join(bin,'python/plugins/camera.py')),false,'Use isolated development binaries without an installed built-in camera plugin');
  for(const file of ['microphone.py','audio_output.py'])assert.equal(await exists(join(bin,'python/plugins',file)),false,'Audio fixture must not import a real device adapter');
  await writeFile(cameraPlugin,`import os,time
CAPABILITY='camera.capture'
ACTIONS=['capture']
def handle(action,params):
    device=params.get('device',0)
    if device==1: os.write(1,b'PRIVATE_CAPTURE_BYTES'*10000)
    if device==2: os.write(2,b'PRIVATE_CAPTURE_BYTES'*10000)
    if device==3: time.sleep(20)
    return {'data_base64':'x'*(20000 if device==4 else 16),'synthetic':True}
`);
  await writeFile(micPlugin,`import base64,io,json,pathlib,time,wave
CAPABILITY='audio.capture'
ACTIONS=['record']
ROOT=pathlib.Path(__file__).parent
def handle(action,params):
    path=ROOT/'audio-captured.json'
    calls=json.loads(path.read_text())['calls']+1 if path.exists() else 1
    path.write_text(json.dumps({'calls':calls}))
    if (ROOT/'audio-gate').exists():
        (ROOT/'audio-capturing').write_text('ready')
        while not (ROOT/'audio-release').exists(): time.sleep(.01)
    rate=int(params['sample_rate']); channels=int(params['channels']); frames=int(float(params['duration_seconds'])*rate)
    marker=b'PRIVATE_AUDIO_TEST_SAMPLE_'
    data=(marker*(frames*channels*2//len(marker)+1))[:frames*channels*2]
    output=io.BytesIO()
    with wave.open(output,'wb') as wav:
        wav.setnchannels(channels); wav.setsampwidth(2); wav.setframerate(rate); wav.writeframes(data)
    body=output.getvalue()
    if (ROOT/'audio-bad').exists(): body=b'BAD!'+body[4:]
    return {'mime_type':'audio/wav','sample_rate':rate,'channels':channels,'duration_seconds':params['duration_seconds'],'data_base64':base64.b64encode(body).decode('ascii')}
`);
  await writeFile(speakerPlugin,`import base64,hashlib,json,pathlib
CAPABILITY='audio.play'
ACTIONS=['play']
ROOT=pathlib.Path(__file__).parent
def handle(action,params):
    body=base64.b64decode(params['data_base64'],validate=True)
    path=ROOT/'audio-played.json'
    calls=json.loads(path.read_text())['calls']+1 if path.exists() else 1
    path.write_text(json.dumps({'calls':calls,'bytes':len(body),'sha256':hashlib.sha256(body).hexdigest()}))
    return {'played':True,'bytes':len(body)}
`);
 }
 checkpoint('coordinator-start');
 await mkdir(work);await new Promise(r=>fake.listen(0,'127.0.0.1',r));const [api,gateway]=await Promise.all([port(),port()]);eef=`http://127.0.0.1:${api}`;
 await writeFile(eefConfig,(await readFile(join(root,'config/default_identity.yaml'),'utf8')).replace('port: 51334',`port: ${api}`).replace('port: 51335',`port: ${gateway}`).replace('policy: prompt','policy: off'));
 launch('eef',eefConfig,['--database',join(scratch,'eef.db'),'--no-brain','--no-ui'],undefined,{EEF_NODE_PSK:randomBytes(24).toString('hex')});
 await until(async()=>!!(await json(eef+'/api/diagnostics')).initialized,'coordinator');
 for(const role of ['origin','executor']) {
  checkpoint('node-start-'+role);
  const config=join(scratch,role+'.json'),id='pipeline-fixture-'+role,api=await port(),value=JSON.parse(await readFile(join(root,'config/node.example.json'),'utf8'));
  Object.assign(value,{node_id:id,name:'Pipeline fixture '+role,connection_enabled:false,auto_local:false,local_pairing:false,endpoints:[],psk:'',heartbeat_seconds:0.5,
   dashboard:{enabled:true,ui_enabled:false,host:'127.0.0.1',port:api},update:{policy:'off'},
   models:{provider:'ollama',ollama:{base_url:`http://127.0.0.1:${fake.address().port}`,selected:role==='executor'?[{model_id:'selected',modality:'text'}]:[]}}});
  if(role==='executor'){
   Object.assign(value.permissions.filesystem,{read:true,write:true,roots:[work]});
   Object.assign(value.permissions.process,{enabled:true,allowed_executables:[helper],max_seconds:30,max_output_bytes:65536});
   if(python){value.python=python;value.python_plugins=[cameraPlugin];value.permissions.media.camera=true;
    value.python_bridge={timeout_seconds:2,max_request_bytes:65536,max_stdout_bytes:1048576,max_stderr_bytes:1024};}
  }
  if(python){
   value.python=python;value.python_plugins=role==='origin'?[micPlugin]:[cameraPlugin,speakerPlugin];
   value.python_bridge={timeout_seconds:role==='origin'?10:2,max_request_bytes:65536,max_stdout_bytes:1048576,max_stderr_bytes:1024};
   value.permissions.media.microphone=false;value.permissions.media.audio_output=false;
   value.permissions.peer_transfers={enabled:false};
   value.peer_destination_address=null;
   value.metadata={resources:[],extensions:{fixture_keep:true}};
  }
  // With the explicitly supplied interpreter, isolate cwd so built-in plugin
  // lookup cannot import the repository's real camera adapter. The configured
  // synthetic extension still uses the normal loader and permissions.
  await writeFile(config,JSON.stringify(value));launch('eefn',config,['--no-ui'],undefined,{},python?scratch:root);const node={id,config,url:`http://127.0.0.1:${api}`};nodes.push(node);
  await until(async()=>!!(await json(node.url+'/api/diagnostics')).runtime_id,'node API');
  checkpoint('node-pair-'+role);
  const invitation=await owner(['credentials','provision','--node',id,'--address',`127.0.0.1:${gateway}`]);await local(node,['connection','pair-stdin'],JSON.stringify(invitation));
 }
 checkpoint('registration');const [origin,executor]=nodes;
 await owner(['discovery','grant','--requester',origin.id,'--target',executor.id]);
 const old=(await json(eef+'/api/diagnostics')).runtime_id;await owner(['restart']);await until(async()=>(await json(eef+'/api/diagnostics')).runtime_id!==old,'apply pinned credentials/disclosure');
 for(const node of nodes)await local(node,['connection','resume']);
 await until(async()=>{const status=await json(executor.url+'/api/status');return status.execution_grants.available&&status.capabilities.includes('pipeline.run')&&(await json(eef+'/api/diagnostics')).registered_node_count===2;},'pipeline registration');
 if(python){
  checkpoint('typed-relay-setup');
  for(const node of nodes){
   const before=JSON.parse(await readFile(node.config,'utf8')),runtime=(await json(node.url+'/api/diagnostics')).runtime_id;
   const configured=await local(node,['relay','configure',node===origin?'--microphone':'--receive']);
   assert.equal(configured.success,true);assert.equal(configured.result.changed,true);
   assert.equal(configured.result.peer_permission_saved,false,'resource setup never enables permissions');
   assert.equal(configured.result.restart_requested,false);assert.equal(configured.result.execution_authorized,false);
   assert.equal((await json(node.url+'/api/diagnostics')).runtime_id,runtime,'setup never restarts');
   const saved=JSON.parse(await readFile(node.config,'utf8'));
   for(const key of ['node_id','name','psk','coordinator_certificate','permissions','models','endpoints'])assert.deepEqual(saved[key],before[key],`setup preserves ${key}`);
   assert.deepEqual(saved.metadata.extensions,before.metadata.extensions);
   const repeated=await local(node,['relay','configure',node===origin?'--microphone':'--receive','--transport-resource',configured.result.saved_resources.find(r=>r.capability==='peer.transfer').id]);assert.equal(repeated.result.changed,false);
   assert.deepEqual(JSON.parse(await readFile(node.config,'utf8')),saved,'repeat configuration preserves generated resource IDs and route');
   const remoteRead=await owner(['node','relay','--node',node.id,'show']);assert.deepEqual(remoteRead.result.saved_resources,configured.result.saved_resources);
   const denied=await cli('eef',eefConfig,['node','relay','--node',node.id,'configure',node===origin?'--microphone':'--send-only'],undefined,false);
   assert.equal(denied.success,false);assert.deepEqual(JSON.parse(await readFile(node.config,'utf8')),saved,'remote writes need current local consent');
   await local(node,['permissions','allow','peer-transfers']);
   await local(node,['permissions','allow',node===origin?'microphone':'audio-output']);
   assert.equal((await local(node,['relay','show'])).result.peer_permission_applied,false,'saving consent does not apply/restart the runtime');
   node.relay=configured.result;
  }
  await local(executor,['permissions','allow','remote-management']);
  const senderOnly=await owner(['node','relay','--node',executor.id,'configure','--send-only']);assert.equal(senderOnly.result.saved_receive_address,null);
  const receiving=await owner(['node','relay','--node',executor.id,'configure','--receive']);
  assert.equal(receiving.result.changed,true);assert.deepEqual(receiving.result.saved_resources,executor.relay.saved_resources);
  audioAddress=receiving.result.saved_receive_address;assert(audioAddress.startsWith('127.0.0.1:'));
  const plain=await launch('eefn',executor.config,['relay','show']).result;assert.equal(plain.code,0);assert(plain.out.includes('loopback-only'));
  assert.equal(calls.length,0);assert.equal(await exists(join(scratch,'audio-captured.json')),false);assert.equal(await exists(join(scratch,'audio-played.json')),false);
  for(const node of nodes){await local(node,['restart','--wait-seconds','30']);}
  await until(async()=>{const a=await local(origin,['relay','show']),b=await owner(['node','relay','--node',executor.id,'show']);return a.result.session_advertised&&b.result.session_advertised&&!a.result.restart_required&&!b.result.restart_required;},'apply explicit relay setup/consent restart');
 }
 for(const base of [eef,...nodes.map(n=>n.url)])assert.equal((await fetch(base+'/')).status,404);
 const review=reviewedFixture(eef,origin.url,origin.id);
 async function progressReceipt(id,grantId){
  const before=await readFile(executor.config+'.grant-receipts.sqlite3');
  const localReceipt=(await local(executor,['grants','receipt','--id',grantId])).receipt;
  const throughOrigin=(await local(origin,['requests','receipt',id,'--grant-id',grantId])).result;
  const throughOwner=await owner(['requests','receipt',id,'--grant-id',grantId]);
  assert.deepEqual(throughOrigin.receipt,localReceipt);assert.deepEqual(throughOwner.receipt,localReceipt);
  assert.equal(throughOrigin.execution_requested,false);assert.equal(throughOrigin.automatically_retried,false);assert.equal(throughOrigin.reconciled,false);
  assert.deepEqual(await readFile(executor.config+'.grant-receipts.sqlite3'),before,'progress inspection cannot write receipts');
  assert.equal(localReceipt.pipeline_progress.durable,true);assert.equal(localReceipt.pipeline_progress.termination_confirmed,false);
  for(const key of ['params','output','prompt','path'])assert.equal(key in localReceipt.pipeline_progress,false);
  return localReceipt;
 }
 const pipeline=(source,dest)=>({template:'local_pipeline',constraints:{node_id:executor.id},params:{pipeline:{schema_version:1,max_retained_bytes:2048,steps:[
  {id:'read',capability:'filesystem',action:'read',params:{path:source}},
  {id:'infer',capability:'llm.infer',action:'run',model_id:'selected',backend:'ollama',params:{max_tokens:16},inputs:[{parameter:'prompt',source_step:'read',pointer:'/content'}]},
  {id:'write',capability:'filesystem',action:'write',params:{path:dest},inputs:[{parameter:'content',source_step:'infer',pointer:'/content'}]}
 ]}}});
 const source=join(work,'source.txt'),dest=join(work,'answer.txt');await writeFile(source,'public pipeline input');
 // Typed source submission via the existing CLI, no raw RPC or model interpretation.
 const input={schema_version:1,original_text:'Process the selected node input locally.',title:'Local pipeline',input:{schema_version:1,...pipeline(source,dest)}};
 const file=join(scratch,'pipeline-input.json');await writeFile(file,JSON.stringify(input.input));
 checkpoint('finite-submission');const submitted=await local(origin,['requests','submit','--text',input.original_text,'--title',input.title,'--file',file]);assert.equal(submitted.execution_authorized,false);assert.equal(submitted.dispatched,false);
 const plan=submitted.result.plan;assert.equal(plan.status,'awaiting_approval');assert.equal(plan.tasks.length,1);assert.equal(plan.tasks[0].capability,'pipeline.run');
 assert.deepEqual(plan.draft.proposals[0].proposal.suggested_capabilities,['pipeline.run','filesystem','llm.infer']);
 assert.equal(plan.bindings[0].node_id,executor.id);assert.equal(calls.length,0);assert.equal(await exists(dest),false);
 const command=await review.approve(plan,{max_execution_ms:8000,max_result_bytes:8192});assert.equal(calls.length,0);assert.equal(await exists(dest),false);
 checkpoint('finite-execution');await review.start(command);const report=await review.result(plan.id);assert.equal(report.state,'completed');assert.equal(report.success,true);
 assert.equal(report.data.report_type,'local_pipeline_result');assert.deepEqual(report.data.completed_steps,['read','infer','write']);
 assert.equal(await readFile(dest,'utf8'),'synthetic local result: public pipeline input');assert.equal(calls.length,1);
 const saved=await review.inspect(plan.id);assert.equal(saved.tasks.length,1);assert.equal(saved.execution.attempts.length,1);const granted=saved.execution.attempts[0].grant;
 assert.equal(granted.capability,'pipeline.run');assert.equal(granted.executor_node,executor.id);assert.equal(granted.request_context.origin_node,origin.id);
 assert.deepEqual(granted.params,pipeline(source,dest).params);assert.equal(report.data.output.content,undefined,'intermediate model text is not separate coordinator output');
 const completedReceipt=await progressReceipt(plan.id,granted.grant_id);
 assert.equal(completedReceipt.state,'completed');assert.equal(completedReceipt.pipeline_progress.phase,'completed');
 assert.equal(completedReceipt.pipeline_progress.sequence,6);assert.equal(completedReceipt.pipeline_progress.completed_steps,3);assert.equal(completedReceipt.pipeline_progress.current_step,'write');
 assert.equal((await cli('eefn',executor.config,['requests','receipt',plan.id,'--grant-id',granted.grant_id],undefined,false)).success,false,'executor is not the request origin');
 await review.start(command);assert.equal(calls.length,1);assert.equal((await review.inspect(plan.id)).execution.attempts.length,1);
 if(python){
  checkpoint('real-python-bounds');
  await until(async()=>(await json(executor.url+'/api/status')).capabilities.includes('camera.capture'),'synthetic Python capture plugin');
  const bridge=(await json(executor.url+'/api/status')).python_bridge;
  assert.equal(bridge.enabled,true);assert.equal(bridge.restart_required,false);assert.equal(bridge.applied.timeout_seconds,2);
  async function capture(device){
   const p=await review.save({template:'local_pipeline',constraints:{node_id:executor.id},params:{pipeline:{schema_version:1,max_retained_bytes:2048,
    steps:[{id:'capture',capability:'camera.capture',action:'capture',params:{device}}]}}});
   await review.start(await review.approve(p,{max_execution_ms:6000,max_result_bytes:8192}));
   return {id:p.id,report:await review.result(p.id)};
  }
  assert.equal((await capture(0)).report.success,true);
  for(const device of [1,2,3,4]){
   const {id,report}=await capture(device);assert.equal(report.state,'unknown');assert.equal(report.success,false);
   assert.equal(report.error_code,device===2?'python_stderr_byte_limit':device===3?'python_exchange_deadline':'python_stdout_byte_limit');
   assert(!JSON.stringify(report).includes('PRIVATE_CAPTURE_BYTES'));
   const retained=await review.inspect(id);assert.equal(retained.execution.attempts.length,1);assert.equal(retained.status,'interrupted');
   assert.equal((await capture(0)).report.success,true,'bounded plugin failure must not retire core commands or later fresh work');
  }
  const ordinary=await review.save({template:'capture_camera',constraints:{node_id:executor.id},params:{device:4}});
  await review.start(await review.approve(ordinary,{max_execution_ms:6000,max_result_bytes:1024}));
  const ordinaryResult=await review.result(ordinary.id);
  assert.equal(ordinaryResult.state,'unknown','ordinary grant must also tighten the Python result pipe before decoding');
  assert.equal(ordinaryResult.success,false);
  assert.equal(ordinaryResult.error_code,'python_stdout_byte_limit');
  checkpoint('finite-peer-audio');
  await until(async()=>{const a=await json(origin.url+'/api/status'),b=await json(executor.url+'/api/status');return a.peer_advertisement?.finite_audio&&b.peer_advertisement?.finite_audio;},'audio registration');
  const audioArgs=(operation=randomUUID())=>['requests','submit-audio','--to',executor.id,'--duration-ms','500','--operation-id',operation];
  async function audioCounts(){
   return Promise.all(['audio-captured.json','audio-played.json'].map(async name=>
    await exists(join(scratch,name))?JSON.parse(await readFile(join(scratch,name),'utf8')).calls:0));
  }
  async function audioStart(){
   const before=await audioCounts(),args=audioArgs(),submitted=await local(origin,args),p=submitted.result.result.plan;
   assert.equal(submitted.execution_authorized,false);assert.equal(submitted.dispatched,false);assert.equal(p.status,'awaiting_approval');
   assert.deepEqual(p.draft.audio_source,submitted.command.input,'typed intent is retained with concrete resource choices');
   const repeated=await local(origin,args);assert.equal(repeated.result.result.creation_reused,true);assert.equal(repeated.result.result.plan.id,p.id);
   assert.deepEqual(repeated.result.concrete,submitted.result.concrete);assert.equal(repeated.result.result.plan.revision,p.revision);
   assert.deepEqual(await audioCounts(),before,'saving/compiling cannot capture or play');
   const command=await review.approve(p,{max_execution_ms:6000,max_result_bytes:4096});
   assert.deepEqual(await audioCounts(),before,'approval alone cannot capture or play');
   await review.start(command);return {id:p.id,command,args,submitted};
  }
  // Reverse discovery is not implied by the origin -> executor disclosure grant.
  const jobsBeforeDenied=(await json(eef+'/api/jobs')).jobs.length;
  const deniedAuthor=await cli('eefn',executor.config,['requests','submit-audio','--source-node',origin.id,'--to',executor.id,'--duration-ms','500'],undefined,false);
  assert.equal(deniedAuthor.success,false);assert.equal(deniedAuthor.result,null);assert.equal(deniedAuthor.dispatched,false);
  assert.equal((await json(eef+'/api/jobs')).jobs.length,jobsBeforeDenied,'undisclosed authoring cannot create a review');
  // Drop/corrupt real saved replies, or simulate an old unsupported node.
  // None may cause a second send, fallback or source-provenance acceptance.
  for(const authorMode of ['drop','corrupt','prose','old']){
  const lostArgs=audioArgs(),marker=origin.config+'.api.json',originalMarker=await readFile(marker,'utf8');
  let authorWrites=0,lostReport;
  const authorProxy=createServer(async(req,res)=>{
   try{let body='';for await(const chunk of req){body+=chunk;assert(body.length<65536);}
    assert.equal(req.url,'/api/commands/requests/audio');authorWrites++;
    if(authorMode==='old'){res.writeHead(404);return res.end('{}');}
    const response=await fetch(origin.url+req.url,{method:'POST',headers:{'Content-Type':'application/json'},body,signal:AbortSignal.timeout(15000)});
    lostReport=await response.json();assert(response.ok);
    if(authorMode==='drop')return res.destroy();
    const forged=structuredClone(lostReport);
    if(authorMode==='corrupt')forged.result.result.plan.draft.audio_source=null;
    if(authorMode==='prose')forged.note='PRIVATE_AUDIO_FORGED_REPLY_MUST_NOT_ECHO';
    res.writeHead(200,{'Content-Type':'application/json'});res.end(JSON.stringify(forged));
   }catch{res.destroy();}
  });
  await new Promise(r=>authorProxy.listen(0,'127.0.0.1',r));
  const endpoint=JSON.parse(originalMarker);endpoint.address='127.0.0.1:'+authorProxy.address().port;
  const noAudioBefore=await audioCounts();
  await writeFile(marker,JSON.stringify(endpoint));
  try{const lost=await cli('eefn',origin.config,lostArgs,undefined,false);assert.equal(lost.outcome_unknown,authorMode!=='old');assert.equal(lost.automatically_retried,false);assert.equal(authorWrites,1);assert(!JSON.stringify(lost).includes('PRIVATE_AUDIO_FORGED_REPLY_MUST_NOT_ECHO'));}
  finally{await writeFile(marker,originalMarker);authorProxy.closeAllConnections();await new Promise(r=>authorProxy.close(r));}
  assert.deepEqual(await audioCounts(),noAudioBefore);
  const found=await local(origin,['jobs','find','--operation-id',lostArgs.at(-1)]);
  assert.equal(found.data.jobs.length,authorMode==='old'?0:1);
  if(authorMode!=='old'){assert.equal(lostReport.success,true);assert.equal(found.data.jobs[0].id,lostReport.result.result.plan.id);}
  const inspectedAuthor=await local(origin,lostArgs);assert.equal(inspectedAuthor.result.result.creation_reused,authorMode!=='old');
  if(authorMode!=='old'){
   assert.deepEqual(inspectedAuthor.result.concrete,lostReport.result.concrete);
   assert.equal(inspectedAuthor.result.result.plan.revision,lostReport.result.result.plan.revision);
  }
  assert.equal((await owner(['scheduling','show'])).admission.total_in_flight,0,'authoring does not reserve execution slots');
  assert.deepEqual(await audioCounts(),noAudioBefore);
  }
  const completed=await audioStart();await review.wait(completed.id);const audioPlan=await review.inspect(completed.id);
  assert.equal(audioPlan.status,'completed');assert.equal(audioPlan.execution.attempts.length,2);
  const reports=audioPlan.execution.attempts.map(a=>a.report);assert(reports.every(r=>r.success&&r.persisted));
  assert.equal(reports[0].data.payload_sha256,reports[1].data.payload_sha256);assert.equal(reports[0].data.payload_bytes,16044);
  assert.equal(reports[0].data.producer_reported,true);assert.equal(reports[1].data.sink_reported,true);
  assert(reports.every(r=>r.data.payload_retained===false&&r.data.sink_effects_confirmed===false));
  assert(!JSON.stringify(audioPlan).includes('"data_base64"'));assert(!JSON.stringify(audioPlan).includes('PRIVATE_AUDIO_TEST_SAMPLE_'));
  assert.equal(JSON.parse(await readFile(join(scratch,'audio-played.json'),'utf8')).sha256,reports[0].data.payload_sha256);
  await review.start(completed.command);assert.equal(JSON.parse(await readFile(join(scratch,'audio-captured.json'),'utf8')).calls,1);assert.equal(JSON.parse(await readFile(join(scratch,'audio-played.json'),'utf8')).calls,1);
  // A saved speaker revocation must reject destination before source capture.
  const speakerConfig=(await json(executor.url+'/api/config')).config;speakerConfig.permissions.media.audio_output=false;await json(executor.url+'/api/config',{config:speakerConfig},'PUT');
  const denied=await audioStart();await review.wait(denied.id);const deniedPlan=await review.inspect(denied.id);assert.equal(deniedPlan.status,'interrupted');
  assert.equal(deniedPlan.execution.attempts[1].state,'rejected');assert.equal(deniedPlan.execution.attempts[0].acknowledgement,null);
  assert.equal(JSON.parse(await readFile(join(scratch,'audio-captured.json'),'utf8')).calls,1);
  speakerConfig.permissions.media.audio_output=true;await json(executor.url+'/api/config',{config:speakerConfig},'PUT');
  // Revoking the applied receiver route while capture waits must retire that
  // session; never silently dial/bind the replacement address or play its data.
  await writeFile(join(scratch,'audio-gate'),'ready');const gatedAudio=await audioStart();await until(()=>exists(join(scratch,'audio-capturing')),'synthetic audio capture pending');
  const audioAdmission=(await owner(['scheduling','show'])).admission;
  assert.equal(audioAdmission.total_in_flight,2,'one executor future per endpoint, not one per nested operation');
  for(const node of [origin,executor])assert.equal(audioAdmission.node_counts.find(n=>n.node_id===node.id).in_flight,1);
  for(const [node,cap] of [[origin,'audio.capture'],[executor,'audio.play']])assert.equal(audioAdmission.providers.find(p=>p.node_id===node.id&&p.capability===cap).in_flight,1,'media dependencies share existing bundle admission');
  speakerConfig.peer_destination_address=`127.0.0.1:${await port()}`;await json(executor.url+'/api/config',{config:speakerConfig},'PUT');
  await until(async()=>(await review.inspect(gatedAudio.id)).execution.attempts[1].state==='unknown','receiver route revocation');
  await writeFile(join(scratch,'audio-release'),'ready');await review.wait(gatedAudio.id);assert.equal((await review.inspect(gatedAudio.id)).status,'interrupted');
  assert.equal(JSON.parse(await readFile(join(scratch,'audio-played.json'),'utf8')).calls,1);
  await unlink(join(scratch,'audio-gate'));
  speakerConfig.peer_destination_address=audioAddress;await json(executor.url+'/api/config',{config:speakerConfig},'PUT');
  await writeFile(join(scratch,'audio-bad'),'ready');const badAudio=await audioStart();await review.wait(badAudio.id);
  assert.equal((await review.inspect(badAudio.id)).status,'interrupted','malformed capture must not reach playback');
  assert.equal(JSON.parse(await readFile(join(scratch,'audio-played.json'),'utf8')).calls,1);
  const badCaptures=JSON.parse(await readFile(join(scratch,'audio-captured.json'),'utf8')).calls;
  await review.start(badAudio.command);assert.equal(JSON.parse(await readFile(join(scratch,'audio-captured.json'),'utf8')).calls,badCaptures,'consumed malformed capture is never replayed');
  await unlink(join(scratch,'audio-bad'));
  const freshAudio=await audioStart();await review.wait(freshAudio.id);assert.equal((await review.inspect(freshAudio.id)).status,'completed');
  assert.equal(JSON.parse(await readFile(join(scratch,'audio-played.json'),'utf8')).calls,2);
  // Change the live microphone resource after an old request has completed.
  // Reusing its receipt must not discover/select the replacement.
  const originConfig=(await json(origin.url+'/api/config')).config;
  const previousMic=completed.submitted.result.concrete.input.params.transfer.audio.source_resource_id;
  const newMic='changed-'+randomUUID();
  originConfig.metadata.resources.find(r=>r.capability==='audio.capture').id=newMic;
  await json(origin.url+'/api/config',{config:originConfig},'PUT');await local(origin,['restart','--wait-seconds','30']);
  await until(async()=>(await local(origin,['relay','show'])).result.session_advertised,'reconfigured microphone registration');
  const beforeRetained=await audioCounts(),retained=await local(origin,completed.args);
  assert.equal(retained.result.result.creation_reused,true);assert.equal(retained.result.result.plan.id,completed.id);
  assert.equal(retained.result.concrete.input.params.transfer.audio.source_resource_id,previousMic);
  assert.deepEqual(retained.result.concrete,completed.submitted.result.concrete);assert.deepEqual(await audioCounts(),beforeRetained);
  const plainAuthor=await launch('eefn',origin.config,completed.args).result;assert.equal(plainAuthor.code,0);assert(plainAuthor.out.includes('Retained finite audio relay'));assert(!plainAuthor.out.includes('\"schema_version\"'));
  const replacement=await audioStart();assert.equal(replacement.submitted.result.concrete.input.params.transfer.audio.source_resource_id,origin.id+'::'+newMic);
  await review.wait(replacement.id);assert.equal((await review.inspect(replacement.id)).status,'completed');
  assert.equal(JSON.parse(await readFile(join(scratch,'audio-played.json'),'utf8')).calls,3);
  await owner(['discovery','revoke','--requester',origin.id,'--target',executor.id]);
  const beforeRevokeRestart=(await json(eef+'/api/diagnostics')).runtime_id;await owner(['restart']);
  await until(async()=>(await json(eef+'/api/diagnostics')).runtime_id!==beforeRevokeRestart&&(await json(eef+'/api/diagnostics')).registered_node_count===2,'apply directional disclosure revocation');
  const revokedCounts=await audioCounts(),historical=await local(origin,completed.args);
  assert.equal(historical.result.result.plan.id,completed.id);assert.equal(historical.result.result.creation_reused,true);
  assert.deepEqual(historical.result.concrete,completed.submitted.result.concrete,'retained history needs no new peer disclosure or resolution');
  const jobsAfterRevocation=(await json(eef+'/api/jobs')).jobs.length;
  const noDisclosure=await cli('eefn',origin.config,audioArgs(),undefined,false);assert.equal(noDisclosure.success,false);
  assert.equal((await json(eef+'/api/jobs')).jobs.length,jobsAfterRevocation);assert.deepEqual(await audioCounts(),revokedCounts);
  await owner(['discovery','grant','--requester',origin.id,'--target',executor.id]);
  const beforeRestore=(await json(eef+'/api/diagnostics')).runtime_id;await owner(['restart']);
  await until(async()=>(await json(eef+'/api/diagnostics')).runtime_id!==beforeRestore&&(await json(eef+'/api/diagnostics')).registered_node_count===2,'restore fixture disclosure for remaining cases');
 }
 // Later-stage permission loss while the existing model stage is pending.
 checkpoint('permission-loss');
 const gatedSource=join(work,'gated-source.txt'),gatedDest=join(work,'must-not-write.txt');await writeFile(gatedSource,'gated pipeline input');
 const gated=await review.save(pipeline(gatedSource,gatedDest));await review.start(await review.approve(gated,{max_execution_ms:8000,max_result_bytes:8192}));
 await until(()=>gates.size===1,'model stage is pending');
 const gatedGrant=(await review.inspect(gated.id)).execution.attempts[0].grant;
 const pendingReceipt=await progressReceipt(gated.id,gatedGrant.grant_id);
 assert.equal(pendingReceipt.state,'accepted');assert.equal(pendingReceipt.pipeline_progress.phase,'starting');
 assert.equal(pendingReceipt.pipeline_progress.sequence,3);assert.equal(pendingReceipt.pipeline_progress.completed_steps,1);assert.equal(pendingReceipt.pipeline_progress.current_step,'infer');
 const admission=(await owner(['scheduling','show'])).admission;
 assert.equal(admission.total_in_flight,1,'compound work counts one parent future');
 assert.equal(admission.node_counts.find(n=>n.node_id===executor.id).in_flight,1);
 for(const capability of ['pipeline.run','filesystem'])assert.equal(admission.providers.find(p=>p.node_id===executor.id&&p.capability===capability).in_flight,1,'each distinct child provider is reserved once');
 const permissions=(await json(executor.url+'/api/config')).config;
 permissions.permissions.filesystem.write=false;await json(executor.url+'/api/config',{config:permissions},'PUT');
 const unknown=await review.result(gated.id);assert.equal(unknown.state,'unknown');assert.equal(unknown.error_code,'local_policy_changed');assert.equal(await exists(gatedDest),false);
 const interruptedReceipt=await progressReceipt(gated.id,gatedGrant.grant_id);
 assert.equal(interruptedReceipt.state,'unknown');assert.deepEqual(interruptedReceipt.pipeline_progress,pendingReceipt.pipeline_progress);
 await until(async()=>(await owner(['scheduling','show'])).admission.total_in_flight===0,'compound reservations released');
 for(const response of gates)response.destroy();gates.clear();
 permissions.permissions.filesystem.write=true;await json(executor.url+'/api/config',{config:permissions},'PUT');await local(executor,['restart','--wait-seconds','60']);
 await until(async()=>
  (await json(executor.url+'/api/status')).execution_grants.available
  &&(await json(eef+'/api/diagnostics')).registered_node_count===2
  &&(await json(eef+'/api/capabilities')).providers.some(p=>p.node_id===executor.id&&p.capability==='process.exec'),
  'executor restored and native provider registered explicitly');
 assert.equal((await review.inspect(gated.id)).execution.attempts.length,1);assert.equal(await exists(gatedDest),false);
 assert.deepEqual(await progressReceipt(gated.id,gatedGrant.grant_id),interruptedReceipt,'restart preserves the observation without resuming the pipeline');
 // Known unsuccessful native stage stops the graph without a later sink effect.
 checkpoint('native-stage');
 const native=join(scratch,'native-failed'),failedDest=join(work,'after-failure.txt');await mkdir(native);
 const failedInput={template:'local_pipeline',constraints:{node_id:executor.id},params:{pipeline:{schema_version:1,max_retained_bytes:2048,steps:[
  {id:'process',capability:'process.exec',action:'run',params:{program:helper,args:['fail',native],timeout_seconds:20}},
  {id:'write',capability:'filesystem',action:'write',params:{path:failedDest,content:'must not execute'}}
 ]}}};
 const failed=await review.save(failedInput);const failedCommand=await review.approve(failed,{max_execution_ms:8000,max_result_bytes:8192});
 assert.equal(await exists(join(native,'pids.json')),false);await review.start(failedCommand);
 await until(()=>exists(join(native,'pids.json')),'native stage started');const identities=await readFile(join(native,'pids.json'),'utf8');
 const observer=spawnCommand(helper,['observe',native]);await until(()=>exists(join(native,'observer-ready')),'creation-verified native handles held');
 await writeFile(join(native,'release'),'release isolated fixture');const observed=await observer.result;
 assert.equal(observed.code,0);const proof=JSON.parse(observed.out);assert.equal(proof.both_exited,true);assert.equal(proof.held_creation_verified_handles,true);
 const failedReport=await review.result(failed.id);assert.equal(failedReport.state,'completed');assert.equal(failedReport.success,false);
 assert.equal(failedReport.error_code,'pipeline_stage_unsuccessful');assert.deepEqual(failedReport.data.completed_steps,[]);
 assert.equal(failedReport.data.failed_step,'process');assert.equal(failedReport.data.output.exit_code,17);
 assert.equal((await review.get(failed.id)).status,'failed');assert.equal(await exists(failedDest),false);
 const failedGrant=(await review.inspect(failed.id)).execution.attempts[0].grant;
 const failedReceipt=await progressReceipt(failed.id,failedGrant.grant_id);
 assert.equal(failedReceipt.state,'completed');assert.equal(failedReceipt.pipeline_progress.phase,'failed');
 assert.equal(failedReceipt.pipeline_progress.sequence,2);assert.equal(failedReceipt.pipeline_progress.completed_steps,0);assert.equal(failedReceipt.pipeline_progress.current_step,'process');
 await review.start(failedCommand);assert.equal((await review.inspect(failed.id)).execution.attempts.length,1);
 assert.equal(await readFile(join(native,'pids.json'),'utf8'),identities,'failed graph cannot replay native work');
 const failedBefore=JSON.stringify(await review.inspect(failed.id));
 // Explicit bounded recurring graph uses the existing definition/policy/timer.
 // No implicit approval or second workload system; no ambient/continuous claim.
 checkpoint('bounded-recurrence');
 const automaticDest=join(work,'scheduled-copy.txt');
 const automaticInput={template:'local_pipeline',constraints:{node_id:executor.id},params:{pipeline:{schema_version:1,max_retained_bytes:2048,steps:[
  {id:'read',capability:'filesystem',action:'read',params:{path:source}},
  {id:'write',capability:'filesystem',action:'write',params:{path:automaticDest},inputs:[{parameter:'content',source_step:'read',pointer:'/content'}]}
 ]}}};
 const automaticSource=await review.save(automaticInput),automaticSourceBefore=JSON.stringify(await review.inspect(automaticSource.id));
 const definition=randomUUID();
 const defined=await owner(['automations','define','--operation-id',definition,'--from-job',automaticSource.id,'--revision',String(automaticSource.revision),
  '--plan-sha256',automaticSource.plan_sha256,'--name','Bounded local copies','--every-ms','5000','--max-runs','2','--valid-for-ms','60000']);
 assert.equal(await exists(automaticDest),false);
 const authorization=await owner(['automation-policies','approve','--definition',definition,'--revision','1','--definition-sha256',defined.result.definition.definition_sha256,
  '--generation','0','--operation-id',randomUUID(),'--valid-for-ms','60000','--max-runs','2','--max-execution-ms-per-task','5000','--max-result-bytes-per-task','4096',
  '--max-reserved-execution-ms','10000','--max-reserved-result-bytes','8192']);
 assert.equal(authorization.result.execution_requested,false);assert.equal(authorization.result.timers_started,false);assert.equal(await exists(automaticDest),false);
 const policy=authorization.result.authorization,schedule=randomUUID();
 const enableArgs=['automation-schedules','enable','--policy',policy.id,'--policy-sha256',policy.authorization_sha256,'--operation-id',schedule];
 await owner(enableArgs);
 const scheduleDeadline=Date.now()+45000;
 for(;;){
  const current=(await owner(['automation-schedules','show',schedule])).result.schedule;
  if(current.state==='exhausted')break;
  assert(!['interrupted','stopped','resolved'].includes(current.state),'schedule stopped for inspection: '+current.reason+'; private '+scratch);
  assert(Date.now()<scheduleDeadline,'two finite occurrences did not settle');await new Promise(r=>setTimeout(r,200));
 }
 const scheduled=(await owner(['automation-schedules','show',schedule])).result;assert.equal(scheduled.schedule.claimed_runs,2);assert.equal(scheduled.timer_active,false);
 const occurrences=(await owner(['automation-occurrences','list','--policy',policy.id])).result.items;assert.equal(occurrences.length,2);
 const automaticJobs=[];
 for(const occurrence of occurrences){const p=await review.inspect(occurrence.job_id);assert.equal(p.status,'completed');assert.equal(p.execution.attempts.length,1);
  assert.equal(p.execution.attempts[0].grant.capability,'pipeline.run');assert.deepEqual(p.execution.attempts[0].grant.params,automaticInput.params);
  assert.deepEqual(p.request_review.proposals[0].proposal.suggested_capabilities,['pipeline.run','filesystem']);
  assert.deepEqual((await review.result(p.plan_id)).data.completed_steps,['read','write']);automaticJobs.push(p);
 }
 assert.equal(new Set(automaticJobs.map(p=>p.goal.request_context.request_id)).size,2);
 assert(automaticJobs.every(p=>p.goal.request_context.request_id!==JSON.parse(automaticSourceBefore).goal.request_context.request_id));
 assert.equal(JSON.stringify(await review.inspect(automaticSource.id)),automaticSourceBefore);
 assert.equal(await readFile(automaticDest,'utf8'),'public pipeline input');
 assert.equal((await owner(enableArgs)).result.reused,true);assert.equal((await owner(['automation-occurrences','list','--policy',policy.id])).result.items.length,2);
 checkpoint('restart');const completedBefore=JSON.stringify(await review.inspect(plan.id));const coordinatorBefore=(await json(eef+'/api/diagnostics')).runtime_id;
 await owner(['restart']);await until(async()=>(await json(eef+'/api/diagnostics')).runtime_id!==coordinatorBefore&&(await json(eef+'/api/diagnostics')).registered_node_count===2,'coordinator restart');
 assert.equal(JSON.stringify(await review.inspect(plan.id)),completedBefore);assert.equal(calls.length,2);assert.equal(await exists(gatedDest),false);
 assert.equal(JSON.stringify(await review.inspect(failed.id)),failedBefore);assert.equal(await exists(failedDest),false);
 assert.deepEqual(await progressReceipt(plan.id,granted.grant_id),completedReceipt);
 assert.deepEqual(await progressReceipt(gated.id,gatedGrant.grant_id),interruptedReceipt);
 assert.equal((await owner(enableArgs)).result.reused,true);assert.equal((await owner(['automation-schedules','show',schedule])).result.timer_active,false);
 assert.equal((await owner(['automation-occurrences','list','--policy',policy.id])).result.items.length,2);
 for(const p of automaticJobs)assert.equal(JSON.stringify(await review.inspect(p.plan_id)),JSON.stringify(p));
 assert.equal((await json(origin.url+'/api/config')).config.permissions.filesystem.read,false,'origin feature is never widened');
 await writeFile(join(scratch,'results.json'),JSON.stringify({passed:true,command_driven:true,no_ui_controller:true,pinned_tls_two_logical_nodes:true,physical_two_pc:false,synthetic_model:true,real_model_quality:false,
  explicit_source_compilation:true,no_work_before_approval_and_execution:true,one_local_compound_parent:true,atomic_child_provider_accounting:true,exact_stage_definitions:true,local_dataflow:true,one_selected_model_call:true,
  no_intermediate_coordinator_payload:true,one_attempt_no_replay:true,later_permission_loss_stops_pending_work:true,unknown_retained_after_restart:true,completed_history_unchanged:true,origin_permissions_unchanged:true,
  known_failed_native_stage_stops_later_work:true,native_creation_verified_exit_observed:true,failed_history_unchanged:true,
  durable_bounded_pipeline_observations:true,existing_local_origin_and_owner_receipt_commands:true,read_only_progress_inspection:true,
  completed_failed_and_pending_progress_correlated:true,interrupted_progress_survives_restart_without_replay:true,progress_is_not_termination_proof:true,
  bounded_explicit_recurring_graph:true,two_fresh_approved_parent_occurrences:true,no_work_before_policy_and_enable:true,immutable_automatic_source:true,
  exhausted_schedule_no_receipt_rearm_or_restart_replay:true,
  real_python_bridge:!!python,synthetic_finite_peer_audio:!!python,physical_audio:false,
  typed_local_and_remote_relay_setup:!!python,setup_without_json_resources_or_port_numbers:!!python,setup_never_grants_permissions_or_restarts:!!python,
  typed_audio_authoring_without_json_or_model:!!python,disclosure_before_resolution:!!python,lost_authoring_reply_single_send:!!python,
  corrupted_and_unsupported_audio_authoring_no_fallback:!!python,retained_audio_history_after_disclosure_revocation:!!python,
  retained_audio_choices_survive_resource_reconfiguration:!!python,explicit_new_audio_request_can_choose_new_resource:!!python,
  paired_audio_permission_and_route_revocation:!!python,malformed_audio_never_played:!!python,paired_audio_no_replay:!!python,
  continuous_workloads:false,physical_resource_quota_proven:false,strong_stop_acknowledgement:false},null,2));
 checkpoint('passed');console.log('Public local pipeline validation passed: '+scratch);
}catch(error){await writeFile(join(scratch,'PRIVATE-failure.json'),JSON.stringify({message:String(error)},null,2));await writeFile(join(scratch,'PRIVATE-logs.json'),JSON.stringify(children.map(c=>({exe:c.spawnfile,code:c.exitCode,...c.logs()})),null,2));throw error;}
finally{for(const response of gates)response.destroy();for(const child of children)if(child.exitCode===null)child.kill();await Promise.allSettled(children.map(c=>c.result));fake.closeAllConnections();await new Promise(r=>fake.close(r));}
