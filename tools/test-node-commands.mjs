// Command-driven one-PC integration. No browser, AI model or personal config.
import {readLoopbackCoordinatorTemplate} from './local-test-network.mjs';
import assert from 'node:assert/strict';
import {mkdtemp,readFile,writeFile} from 'node:fs/promises';
import {resolve,join} from 'node:path';
import {spawn} from 'node:child_process';
import {createServer} from 'node:net';
import {randomBytes,randomUUID} from 'node:crypto';

const root=resolve(import.meta.dirname,'..');
const scratch=await mkdtemp(join(root,'.validation','node-commands-'));
const bin=resolve(process.env.EEF_TEST_BINARY_DIR||join(root,'target/debug'));
const config=join(scratch,'node.json'), children=[];
const env={...process.env,EEF_NODE_PSK:'',APPDATA:join(scratch,'appdata'),EEF_DISCOVERY_DIR:join(scratch,'discovery'),PATH:join(root,'.tooling/llvm-mingw-20260616-ucrt-x86_64/bin')+';'+process.env.PATH};
function launch(name,args,extra={}) {
  const child=spawn(join(bin,name+'.exe'),args,{cwd:root,windowsHide:true,env:{...env,...extra},stdio:['ignore','pipe','pipe']});
  let out='',err='';child.stdout.on('data',b=>out+=b);child.stderr.on('data',b=>err+=b);
  child.result=new Promise((resolve,reject)=>{child.on('error',reject);child.on('close',code=>resolve({code,out,err}));});
  children.push(child);return child;
}
async function command(args,success=true) {
  const child=launch('eefn',['--config',config,'--json','network',...args]);
  const timer=setTimeout(()=>child.kill(),25000);
  try {const r=await child.result;assert.equal(r.code,success?0:1,r.err);const data=JSON.parse(r.out);assert.equal(data.success,success);return data;} finally {clearTimeout(timer);}
}
async function port(){const s=createServer();await new Promise(r=>s.listen(0,'127.0.0.1',r));const p=s.address().port;await new Promise(r=>s.close(r));return p;}
async function until(fn,label){const end=Date.now()+65000;while(Date.now()<end){try{if(await fn())return;}catch{}await new Promise(r=>setTimeout(r,200));}throw Error('Timeout: '+label);}
async function json(url){const r=await fetch(url,{signal:AbortSignal.timeout(2000)});assert(r.ok);return r.json();}
async function nodeCommand(args,expected=0,plain=false){
  const child=launch('eefn',['--config',config,...(plain?[]:['--json']),...args]);
  const timer=setTimeout(()=>child.kill(),30000);
  try{const r=await child.result;assert.equal(r.code,expected,r.err);return plain?r.out:JSON.parse(r.out);}finally{clearTimeout(timer);}
}
try {
  const first=await command(['set','--name','Command node','--advertise-address','26.1.2.3']);
  const id=first.node_id;assert.match(id,/^device-/);assert.equal(first.running,false);
  assert.equal(first.network.advertised_address,'26.1.2.3');
  assert.equal((await command(['show'])).node_id,id);
  const offlineReport=await command(['diagnose']);assert.equal(offlineReport.connection_state,'stopped');assert.equal(offlineReport.metrics_available,false);assert.equal(offlineReport.service_uptime_ms,null);
  const modelCommand=async(args,success=true)=>{const child=launch('eefn',['--config',config,'--json','models',...args]);const timer=setTimeout(()=>child.kill(),20000);try{const result=await child.result;assert.equal(result.code,success?0:1,result.err);return JSON.parse(result.out);}finally{clearTimeout(timer);}};
  const selected=await modelCommand(['select-ollama','--model','offline-fixture','--modality','text','--role','request_interpreter']);
  assert.equal(selected.running,false);assert.equal(selected.restart_required,false);assert.equal(selected.saved_selections.length,1);
  const selectionBefore=await readFile(config,'utf8');
  await modelCommand(['hints','--backend','ollama','--model','offline-fixture','--capability','vlm.analyze'],false);assert.equal(await readFile(config,'utf8'),selectionBefore);
  await modelCommand(['remove','--backend','ollama','--model','offline-fixture']);assert.equal((await modelCommand(['show'])).saved_selections.length,0);
  const before=await readFile(config,'utf8');
  await nodeCommand(['requests','save-input','--text','Cannot save from a stopped node'],1);
  assert.equal(await readFile(config,'utf8'),before);
  await command(['set','--name','Must not save','--advertise-address','http://bad/path'],false);
  assert.equal(await readFile(config,'utf8'),before);
  const [apiPort,nodePort,eefPort]=await Promise.all([port(),port(),port()]);
  const api=`http://127.0.0.1:${apiPort}`,eef=`http://127.0.0.1:${eefPort}`;
  const secret=randomBytes(24).toString('hex');
  const saved=JSON.parse(before);
  Object.assign(saved,{auto_local:false,local_pairing:false,endpoints:[`127.0.0.1:${nodePort}`],psk:secret,dashboard:{enabled:true,host:'127.0.0.1',port:apiPort},update:{policy:'off'},models:{provider:'llamacpp',llamacpp:{slots:[]}}});
  await writeFile(config,JSON.stringify(saved));
  const yaml=(await readLoopbackCoordinatorTemplate()).replace('port: 51334',`port: ${eefPort}`).replace('port: 51335',`port: ${nodePort}`).replace('policy: prompt','policy: off');
  const eefConfig=join(scratch,'eef.yaml');await writeFile(eefConfig,yaml);
  // Exercise ordinary coordinator startup with no models; model-free core
  // acceptance must not depend on the special --no-brain troubleshooting flag.
  const startCoordinator=()=>launch('eef',['--config',eefConfig,'--database',join(scratch,'eef.db')],{EEF_NODE_PSK:secret});
  let coordinator=startCoordinator();
  await until(async()=>(await json(eef+'/api/diagnostics')).initialized===true,'coordinator initialized before node start');
  const emptyReport=await json(eef+'/api/diagnostics');
  const emptyStatus=await json(eef+'/api/status');
  assert.equal(emptyStatus.brain_alive,true,'ordinary startup retains core services without a model');
  assert.equal(emptyStatus.initialized,true);assert.equal(emptyStatus.runtime_id,emptyReport.runtime_id);
  assert.deepEqual(emptyStatus.models,[]);
  for(const field of ['node_event_loss_count','assistant_event_loss_count','world_event_loss_count'])assert.equal(emptyReport[field],0,field);
  assert(emptyReport.node_state_reconciliation_count>=1);
  assert.equal(emptyReport.world_state_may_be_stale,false);assert.equal(emptyReport.world_rules_suspended,false);
  for(const field of ['connected_node_count','registered_node_count','registration_pending_node_count','remote_provider_count','healthy_remote_provider_count','model_count'])assert.equal(emptyReport[field],0,field);
  console.log('Model-free default coordinator startup validation passed');
  launch('eefn',['--config',config,'--no-ui']);
  await until(async()=>(await json(api+'/api/status')).connection?.state==='connected','headless node connected');
  assert.equal((await fetch(api+'/')).status,404);
  assert.equal((await fetch(api+'/app.js')).status,404);
  const online=await command(['show']);assert.equal(online.running,true);assert.equal(online.node_id,id);
  const nodeReport=await command(['diagnose']);assert.equal(nodeReport.report_type,'node_diagnostics');assert.equal(nodeReport.connection_state,'connected');assert(!JSON.stringify(nodeReport).includes(secret));assert(!JSON.stringify(nodeReport).includes('26.1.2.3'));
  assert.equal(nodeReport.node_id,id);assert(nodeReport.successful_connections>=1);
  const coordinatorCommand=async(args,expected=0)=>{const p=launch('eef',['--config',eefConfig,'--json',...args]);const t=setTimeout(()=>p.kill(),30000);try{const r=await p.result;assert.equal(r.code,expected,r.err);return JSON.parse(r.out);}finally{clearTimeout(t);}};
  await until(async()=>{const r=await coordinatorCommand(['diagnostics']);return r.registered_node_count===1&&r.healthy_remote_provider_count>0;},'registration and provider diagnostics visible');
  const coreReport=await coordinatorCommand(['diagnostics']);assert.equal(coreReport.report_type,'coordinator_diagnostics');assert.equal(coreReport.connected_node_count,1);assert(!JSON.stringify(coreReport).includes(secret));
  assert.equal(coreReport.registered_node_count,1);assert.equal(coreReport.registration_pending_node_count,0);
  assert(coreReport.remote_provider_count>0);assert.equal(coreReport.healthy_remote_provider_count,coreReport.remote_provider_count);
  assert.equal(coreReport.model_count,0);assert.match(coreReport.readiness_note,/not a readiness guarantee/);
  assert(!JSON.stringify(coreReport).includes(id));assert(!JSON.stringify(coreReport).includes('26.1.2.3'));
  const inventory=await coordinatorCommand(['models','list']);assert.equal(inventory.scope,'connected_node_registrations');assert.equal(inventory.nodes.length,1);assert.equal(inventory.nodes[0].node_id,id);assert.deepEqual(inventory.nodes[0].models,[]);assert(!JSON.stringify(inventory).includes(secret));assert(!JSON.stringify(inventory).includes('26.1.2.3'));
  assert.equal((await coordinatorCommand(['models','list','--node',id])).nodes.length,1);
  await coordinatorCommand(['models','list','--node','missing-node'],1);
  await coordinatorCommand(['models','list','--limit','33'],1);
  await coordinatorCommand(['models','list','--node',id,'--after',id],1);
  const probe=await coordinatorCommand(['diagnostics','--node',id,'--samples','3']);assert.equal(probe.passed,3);assert.equal(probe.failed,0);assert(probe.samples.every(s=>s.matches_coordinator_version&&s.round_trip_ms>=0));
  await coordinatorCommand(['diagnostics','--node',id,'--samples','11'],1);
  await coordinatorCommand(['diagnostics','--node','missing-node'],1);
  const invite=await coordinatorCommand(['invite','--address','192.0.2.10:51335']);const decoded=JSON.parse(Buffer.from(invite.code,'base64').toString('utf8'));assert.equal(decoded.address,'192.0.2.10:51335');assert.equal(decoded.psk,secret);
  await coordinatorCommand(['invite','--address','http://bad/path'],1);
  // Real shipping raw-input commands, through the running model-free node.
  // Saving has no interpreter, proposal, permission change, grant or execution.
  const operation=randomUUID(),text='Keep this exact input before choosing an interpreter. 👋';
  const normalOperation=randomUUID(),normalText='PRIVATE_MODEL_FREE_NORMAL_INPUT';
  const normalArgs=['requests','input','--text',normalText,'--operation-id',normalOperation];
  const normal=await nodeCommand(normalArgs,1);
  assert.equal(normal.source_confirmed,true);assert.equal(normal.error_code,'interpreter_unavailable');
  assert.equal(normal.outcome_unknown,false);assert.equal(normal.result.input.original_text,normalText);
  assert.equal(normal.interpretation.claim_attempted,false);assert.equal(normal.interpretation.model_inference_requested,false);
  const normalView=(await nodeCommand(['jobs','get',normal.result.id])).data;
  assert.equal(normalView.input_intent.original_text,normalText);assert.equal(normalView.task_count,0);assert.equal(normalView.proposal_count,0);
  assert.equal(normalView.input_interpretation,undefined);
  const normalHistory=await nodeCommand(normalArgs);assert.equal(normalHistory.status,'historical_snapshot');
  assert.equal(normalHistory.result.id,normal.result.id);assert.equal(normalHistory.result.revision,normal.result.revision);
  const normalPlain=await nodeCommand(normalArgs,0,true);assert(normalPlain.includes(normal.result.id));assert(!normalPlain.includes(normalText));
  const plainText='PRIVATE_MODEL_FREE_PLAIN_INPUT';
  const plainOperation=randomUUID();
  const plainFailure=await nodeCommand(['requests','input','--text',plainText,'--operation-id',plainOperation],1,true);
  assert(plainFailure.includes('source request was saved'));assert(plainFailure.includes(plainOperation));
  assert(!plainFailure.includes(plainText));assert(!plainFailure.includes(secret));
  const plainSource=(await nodeCommand(['jobs','find','--operation-id',plainOperation])).data;
  assert.equal(plainSource.total,1);
  const plainRetained=(await nodeCommand(['jobs','get',plainSource.jobs[0].id])).data;
  assert.equal(plainRetained.input_intent.original_text,plainText);assert.equal(plainRetained.task_count,0);
  const askText='PRIVATE_MODEL_FREE_ASK_INPUT';
  const askFailure=await nodeCommand(['--ask',askText],1,true);
  assert(askFailure.includes('source request was saved'));assert(askFailure.includes('interpreter_unavailable'));
  assert(!askFailure.includes(askText));assert(!askFailure.includes(secret));
  console.log('Plain input receipt privacy validation passed');
  console.log('Model-free normal input validation passed');
  const saveArgs=['requests','save-input','--text',text,'--operation-id',operation];
  const inputConfig=await readFile(config,'utf8');
  const input=await nodeCommand(saveArgs);
  assert.equal(input.report_type,'node_request_intent');assert.equal(input.success,true);
  assert.equal(input.result.input.original_text,text);assert.equal(input.result.request_context.origin_node,id);
  assert.equal(input.result.operation_id,operation);assert.equal(input.result.status,'review');
  const noInterpreter=await nodeCommand(['requests','interpret-saved',input.result.id],1);
  assert.equal(noInterpreter.report_type,'node_saved_input_interpretation');assert.equal(noInterpreter.error_code,'interpreter_unavailable');
  assert.equal(noInterpreter.claim_attempted,false);assert.equal(noInterpreter.model_inference_requested,false);
  assert.equal(noInterpreter.source.input.original_text,text);assert.equal(noInterpreter.outcome_unknown,false);
  const missingHint=await nodeCommand(['requests','interpret-saved',input.result.id],1,true);
  assert.match(missingHint,/remains saved/);assert(!missingHint.includes(text));
  const stillRaw=(await nodeCommand(['jobs','get',input.result.id])).data;
  assert.equal(stillRaw.revision,input.result.revision);assert.equal(stillRaw.input_interpretation,undefined);
  for(const field of ['interpretation_requested','execution_authorized','dispatched','automatically_retried'])assert.equal(input[field],false,field);
  const inspect=(await nodeCommand(['jobs','get',input.result.id])).data;
  assert.equal(inspect.review_state,'awaiting_interpretation');assert.equal(inspect.proposal_count,0);
  assert.equal(inspect.can_resume,false);assert.equal(inspect.task_count,0);assert.equal(inspect.input_intent.original_text,text);
  const found=(await nodeCommand(['jobs','find','--operation-id',operation])).data;
  assert.equal(found.total,1);assert.equal(found.jobs[0].id,input.result.id);assert(!('input_intent' in found.jobs[0]));
  const duplicate=await nodeCommand(saveArgs);assert.equal(duplicate.result.reused,true);
  assert.equal(duplicate.result.id,input.result.id);assert.equal(duplicate.result.revision,input.result.revision);
  const conflict=await nodeCommand(['requests','save-input','--text','Changed','--operation-id',operation],1);
  assert.equal(conflict.error_code,'refused');assert.equal(conflict.outcome_unknown,false);
  await nodeCommand(['jobs','resume',input.result.id],1);
  await nodeCommand(['requests','plan',input.result.id],1);
  const plain=await nodeCommand(saveArgs,0,true);assert.match(plain,/awaiting interpretation/);assert(plain.includes(input.result.id));
  assert(!plain.includes(text));assert(!plain.includes(secret));
  const rawRequest={schema_version:1,expected_node_id:id,expected_runtime_id:input.runtime_id,command:{operation_id:randomUUID(),original_text:'Should not save'}};
  let rejected=await fetch(api+'/api/commands/requests/save-input',{method:'POST',headers:{'Content-Type':'application/json',Origin:'https://untrusted.example'},body:JSON.stringify(rawRequest)});
  assert.equal(rejected.status,403);
  rejected=await fetch(api+'/api/commands/requests/save-input',{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify({...rawRequest,expected_node_id:'wrong-node'})});
  assert.equal(rejected.status,400);
  assert.equal(await readFile(config,'utf8'),inputConfig);
  coordinator.kill();await coordinator.result;coordinator=startCoordinator();
  await until(async()=>(await json(eef+'/api/diagnostics')).registered_node_count===1,'node reconnects after fixture coordinator restart');
  const retained=await nodeCommand(saveArgs);
  assert.equal(retained.result.reused,true);assert.equal(retained.result.id,input.result.id);
  assert.equal(retained.result.source_sha256,input.result.source_sha256);assert.equal(retained.result.revision,input.result.revision);
  await nodeCommand(['jobs','stop',input.result.id]);
  const stopped=await nodeCommand(saveArgs);assert.equal(stopped.result.reused,true);assert.equal(stopped.result.status,'cancelled');
  assert.equal((await nodeCommand(['jobs','get',input.result.id])).data.task_count,0);
  assert.equal((await coordinatorCommand(['models','list'])).nodes[0].models.length,0);
  console.log('Durable model-free input command validation passed: '+scratch);
  assert.equal(online.applied_network.advertised_address,'26.1.2.3');
  assert(!JSON.stringify(online).includes(secret));
  const next=await command(['set','--name','Renamed node','--advertise-address','26.4.5.6','--coordinator-id','eef-local','--coordinator-address',`26.4.5.6:${nodePort}`,'--coordinator-state','standby']);
  assert.equal(next.restart_required,true);assert.equal(next.applied_network.advertised_address,'26.1.2.3');
  assert.equal(next.coordinator_endpoints[0],`127.0.0.1:${nodePort}`);
  let r=await fetch(api+'/api/commands/network',{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify({schema_version:1,expected_node_id:'wrong',command:{operation:'show'}})});assert.equal(r.status,400);
  r=await fetch(api+'/api/commands/network',{method:'POST',headers:{'Content-Type':'application/json',Origin:'https://untrusted.example'},body:JSON.stringify({schema_version:1,expected_node_id:id,command:{operation:'show'}})});assert.equal(r.status,403);
  assert((await fetch(api+'/api/restart',{method:'POST'})).ok);
  await until(async()=>{const nodes=(await json(eef+'/api/status')).world.devices;return nodes.some(n=>n.node_id===id&&n.name==='Renamed node'&&n.network?.advertised_address==='26.4.5.6'&&n.network?.coordinator?.state==='standby');},'new advertisement reaches eef');
  const after=JSON.parse(await readFile(config,'utf8'));assert.equal(after.psk,secret);assert.deepEqual(after.permissions,saved.permissions);assert.equal(after.node_id,id);
  await command(['set','--clear-address','--clear-coordinator']);
  const cleared=await command(['show']);assert.equal(cleared.network.advertised_address,null);assert.equal(cleared.network.coordinator,null);
  const results={passed:true,offline_commands_without_models:true,headless_running_commands:true,durable_model_free_input:true,input_reuse_conflict_restart_and_cancellation:true,automatic_remote_interpretation:false,stable_identity:true,distinct_connection_and_advertisement:true,coordinator_metadata_reaches_eef:true,permissions_preserved:true,malformed_address_and_wrong_target_and_origin_rejected:true,physical_two_pc:false};
  await writeFile(join(scratch,'results.json'),JSON.stringify(results,null,2));
  console.log('Node command validation passed: '+scratch);
} finally {
  for(const child of children)if(child.exitCode===null)child.kill();
  await Promise.allSettled(children.map(c=>c.result));
}
