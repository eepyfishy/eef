// Public-command two-logical-node fixture, on one PC. No models or installed nodes.
import assert from 'node:assert/strict';
import {mkdtemp, readFile, writeFile} from 'node:fs/promises';
import {resolve, join} from 'node:path';
import {spawn} from 'node:child_process';
import {createServer} from 'node:net';
import {randomBytes, randomUUID} from 'node:crypto';
const root = resolve(import.meta.dirname, '..');
const scratch = await mkdtemp(join(root, '.validation/node-credentials-'));
const bin = resolve(process.env.EEF_TEST_BINARY_DIR || join(root, 'target/debug'));
const children = [], eefConfig = join(scratch, 'eef.yaml');
let coordinatorProcess;
const secret = randomBytes(24).toString('hex');
const env = {...process.env, EEF_NODE_PSK:'', APPDATA:join(scratch,'appdata'),
  EEF_DISCOVERY_DIR:join(scratch,'discovery'), PATH:join(root,'.tooling/llvm-mingw-20260616-ucrt-x86_64/bin')+';'+process.env.PATH};
function launch(name,args,extra={}) {
  const child = spawn(join(bin,name+'.exe'),args,{cwd:root,windowsHide:true,env:{...env,...extra},stdio:['ignore','pipe','pipe']});
  let out='',err='';child.stdout.on('data',b=>out=(out+b).slice(-1048576));child.stderr.on('data',b=>err=(err+b).slice(-1048576));
  child.result=new Promise((resolve,reject)=>{child.on('error',reject);child.on('close',code=>resolve({code,out,err}));});
  children.push(child);return child;
}
async function cli(name,config,args,success=true) {
  const child=launch(name,['--config',config,'--json',...args]);const timer=setTimeout(()=>child.kill(),30000);
  try {const r=await child.result;assert.equal(r.code,success?0:1,r.err);return JSON.parse(r.out);} finally {clearTimeout(timer);}
}
const coordinator=(args,success=true)=>cli('eef',eefConfig,args,success);
async function until(fn,label) {const deadline=Date.now()+45000;while(Date.now()<deadline){try{if(await fn())return;}catch{}await new Promise(r=>setTimeout(r,150));}throw Error('Timeout: '+label);}
async function port(){const s=createServer();await new Promise(r=>s.listen(0,'127.0.0.1',r));const p=s.address().port;await new Promise(r=>s.close(r));return p;}
async function json(url,body,method){const r=await fetch(url,{method:method||(body?'POST':'GET'),headers:{'Content-Type':'application/json'},body:body?JSON.stringify(body):undefined,signal:AbortSignal.timeout(5000)});assert(r.ok,`${url}: HTTP ${r.status}`);return r.json();}
try {
  const [webPort,gatewayPort]=await Promise.all([port(),port()]);const base=`http://127.0.0.1:${webPort}`;
  const yaml=(await readFile(join(root,'config/default_identity.yaml'),'utf8')).replace('port: 51334',`port: ${webPort}`).replace('port: 51335',`port: ${gatewayPort}`).replace('policy: prompt','policy: off');
  await writeFile(eefConfig,yaml);
  coordinatorProcess=launch('eef',['--config',eefConfig,'--database',join(scratch,'eef.db'),'--no-brain','--no-ui'],{EEF_NODE_PSK:secret});
  await until(async()=>(await json(base+'/api/diagnostics')).initialized,'coordinator startup');
  const nodes=[];
  for(const name of ['a','b']) {
    const config=join(scratch,name+'.json'),apiPort=await port();
    const id=(await cli('eefn',config,['network','set','--name','Credential fixture '+name])).node_id;
    await writeFile(config,JSON.stringify({node_id:id,name:'Credential fixture '+name,psk:'',endpoints:[],auto_local:false,
      local_pairing:false,dashboard:{enabled:true,host:'127.0.0.1',port:apiPort},permissions:{remote_updates:false},
      models:{provider:'llamacpp',llamacpp:{slots:[]}},update:{policy:'off'},custom:{preserve:true}}));
    launch('eefn',['--config',config,'--no-ui']);
    const api=`http://127.0.0.1:${apiPort}`;
    await until(async()=>(await json(api+'/api/status')).node_id===id,'standalone node startup');
    const invitation=await coordinator(['credentials','provision','--node',id,'--address',`127.0.0.1:${gatewayPort}`]);
    const privateFile=join(scratch,name+'-PRIVATE.json');
    await writeFile(privateFile,name==='b'?Buffer.concat([Buffer.from([0xff,0xfe]),Buffer.from(JSON.stringify(invitation),'utf16le')]):'\ufeff'+JSON.stringify(invitation));
    assert.equal(invitation.private,true);assert.equal(invitation.restart_required,true);
    const decoded=JSON.parse(Buffer.from(invitation.code,'base64'));assert.equal(decoded.node_id,id);assert.notEqual(decoded.psk,secret);
    assert.equal(decoded.schema_version,2);assert.equal(typeof decoded.coordinator_certificate,'string');assert(!('private_key' in decoded));
    nodes.push({id,config,api,privateFile,credential:decoded.psk,certificate:decoded.coordinator_certificate});
  }
  assert.notEqual(nodes[0].credential,nodes[1].credential);
  assert.equal(nodes[0].certificate,nodes[1].certificate,'persistent coordinator identity must not change per node');
  const show=await coordinator(['credentials','show']);assert.equal(show.saved_policy.identities.length,2);assert.equal(show.applied_policy.identities.length,0);
  for(const node of nodes) assert(!JSON.stringify(show).includes(node.credential));
  const previous=(await coordinator(['diagnostics'])).runtime_id;
  await coordinator(['restart']);
  await until(async()=>(await coordinator(['diagnostics'])).runtime_id!==previous,'apply provisioned credentials');
  for(const node of nodes) {
    const report=await cli('eefn',node.config,['connection','pair','--code-file',node.privateFile]);
    assert.equal(report.saved_auth_mode,'node_credential');assert.equal(report.restart_requested,true);
    assert(!JSON.stringify(report).includes(node.credential));
    await until(async()=>(await json(node.api+'/api/status')).connection?.state==='connected','credential node connected');
    const config=JSON.parse(await readFile(node.config,'utf8'));
    assert.equal(config.node_id,node.id);assert.equal(config.psk,node.credential);assert.equal(config.auth_mode,'node_credential');
    assert.equal(config.coordinator_certificate,node.certificate);
    assert.equal((await json(node.api+'/api/status')).connection.transport,'pinned_tls13');
    assert.equal(config.custom.preserve,true);assert.equal(config.permissions.remote_updates,false);assert.equal(config.auto_local,false);
    assert.equal((await cli('eefn',node.config,['connection','show'])).saved_auth_mode,'node_credential');
    const proposalFile=join(scratch,node.id+'-proposal.json');
    const proposal={schema_version:1,intent:'information',goal:'Inspect node status',context_hints:[],constraints:[],
      suggested_capabilities:['system.info'],complexity:'simple',workload:'one_shot',needs_clarification:false,clarification:null};
    await writeFile(proposalFile,JSON.stringify({schema_version:1,original_text:'  Inspect status\n',proposal}));
    const reviewed=await cli('eefn',node.config,['requests','review-proposal','--file',proposalFile]);
    assert.equal(reviewed.interpretation_source,'supplied_proposal');assert.equal(reviewed.execution_authorized,false);
    assert.equal(reviewed.result.request_context.origin_node,node.id);assert.equal(reviewed.result.interpretation.original_text,'  Inspect status\n');
    assert.deepEqual(reviewed.result.interpretation.proposal,proposal);assert.equal(reviewed.result.persisted,false);
    assert.equal((await json(node.api+'/api/status')).models.length,0);
    const question={...proposal,needs_clarification:true,clarification:'Which node should be inspected?'};
    await writeFile(proposalFile,JSON.stringify({schema_version:1,original_text:'Inspect that node',proposal:question}));
    const clarification=await cli('eefn',node.config,['requests','review-proposal','--file',proposalFile]);
    assert.equal(clarification.result.interpretation.review_state,'clarification_required');
    assert.equal(clarification.result.interpretation.proposal.clarification,question.clarification);
    const request={schema_version:1,expected_node_id:node.id,request_id:randomUUID(),text:'Check',proposal};
    const endpoint=node.api+'/api/commands/requests/review-proposal';
    assert.equal((await json(endpoint,{...request,expected_node_id:'other'})).error_code,'invalid_proposal_request');
    assert.equal((await json(endpoint,{...request,proposal:{...proposal,suggested_capabilities:['invented.capability']}})).error_code,'invalid_supplied_proposal');
    assert.equal((await fetch(endpoint,{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify({...request,permissions:true})})).status,422);
    assert.equal((await fetch(endpoint,{method:'POST',headers:{'Content-Type':'application/json',Origin:'https://untrusted.example'},body:JSON.stringify(request)})).status,403);
    const jobs=await cli('eefn',node.config,['jobs','list']);assert.equal(jobs.data.total,0);assert.deepEqual(jobs.data.jobs,[]);
    await cli('eefn',node.config,['connection','pair-local'],false);
  }
  await until(async()=>(await coordinator(['diagnostics'])).registered_node_count===2,'two registered nodes');
  for(const node of nodes) assert.equal((await coordinator(['diagnostics','--node',node.id,'--samples','1'])).passed,1);
  // Exercise the actual non-model dispatcher over the pinned TLS sessions.
  for(const node of nodes) {
    const data=await json(base+'/api/assistant/execute',{capability:'system.info',action:'run',params:{},constraints:{node_id:node.id}});
    assert.equal(data.success,true);
  }
  for(const constraints of [false,{node_id:null},{gpu_vram_min_mb:'4096'}]) {
    const r=await fetch(base+'/api/assistant/execute',{method:'POST',headers:{'Content-Type':'application/json'},
      body:JSON.stringify({capability:'system.info',action:'run',params:{},constraints})});
    assert.equal(r.status,400);assert.equal((await r.json()).success,false);
  }
  const providers=(await json(base+'/api/capabilities')).providers.filter(p=>p.node_id===nodes[0].id||p.node_id===nodes[1].id);
  assert(providers.length>0);assert(providers.every(p=>p.in_flight===0));
  const configView=await json(base+'/api/config');
  const privatePolicy=await readFile(eefConfig,'utf8');
  assert(!privatePolicy.includes('__KEEP_EXISTING_SECRET__'),'saved keys must not be replaced with redaction placeholders');
  assert.equal(configView.config.node.authentication.tls.private_key,'__KEEP_EXISTING_SECRET__');
  for(const node of nodes) assert(!JSON.stringify(configView).includes(node.credential));
  // A stale generic configuration client cannot drop the authentication policy.
  delete configView.config.node.authentication;
  await json(base+'/api/config',{config:configView.config},'PUT');
  assert.equal((await coordinator(['credentials','show'])).saved_policy.identities.length,2);
  await coordinator(['credentials','legacy','--allow','false']);
  let old=(await coordinator(['diagnostics'])).runtime_id;await coordinator(['restart']);
  await until(async()=>{const d=await coordinator(['diagnostics']);return d.runtime_id!==old&&d.registered_node_count===2;},'independent reconnect with legacy disabled');
  const policy=await coordinator(['credentials','show']);assert.equal(policy.applied_policy.allow_legacy,false);
  assert.equal(JSON.parse(await readFile(join(scratch,'discovery/local-eef.json'),'utf8')).psk,'','disabled legacy must not advertise a shared fallback');
  // Independent credentials and persisted TLS must boot without an unused legacy PSK/env override.
  const credentialOnly=await json(base+'/api/config');credentialOnly.config.node.psk='';
  await json(base+'/api/config',{config:credentialOnly.config},'PUT');
  coordinatorProcess.kill();await coordinatorProcess.result;
  coordinatorProcess=launch('eef',['--config',eefConfig,'--database',join(scratch,'eef.db'),'--no-brain','--no-ui']);
  await until(async()=>{const d=await coordinator(['diagnostics']);return d.initialized&&d.runtime_id!==old&&d.registered_node_count===2;},'credential-only cold boot without legacy secret');
  assert.equal(JSON.parse(await readFile(join(scratch,'discovery/local-eef.json'),'utf8')).psk,'');
  const beforeEnable=await readFile(eefConfig);
  await coordinator(['credentials','legacy','--allow','true'],false);
  assert.deepEqual(await readFile(eefConfig),beforeEnable,'missing legacy secret must reject enable atomically');
  assert.equal((await coordinator(['credentials','show'])).saved_policy.allow_legacy,false);
  await coordinator(['invite','--address',`127.0.0.1:${gatewayPort}`],false);
  await coordinator(['credentials','revoke','--node',nodes[0].id]);
  assert.equal((await coordinator(['diagnostics','--node',nodes[0].id,'--samples','1'])).passed,1,'saved revocation is not immediate');
  old=(await coordinator(['diagnostics'])).runtime_id;await coordinator(['restart']);
  await until(async()=>{const d=await coordinator(['diagnostics']);return d.runtime_id!==old&&d.registered_node_count===1;},'revocation applied');
  const inventory=await coordinator(['models','list']);assert.deepEqual(inventory.nodes.map(n=>n.node_id),[nodes[1].id]);
  assert.equal((await coordinator(['diagnostics','--node',nodes[1].id,'--samples','1'])).passed,1);
  await coordinator(['credentials','provision','--node',nodes[0].id,'--address',`127.0.0.1:${gatewayPort}`],false);
  // Re-provision is explicit rotation, followed by independent restart/pairing.
  const rotated=await coordinator(['credentials','provision','--node',nodes[0].id,'--address',`127.0.0.1:${gatewayPort}`,'--rotate']);
  const decoded=JSON.parse(Buffer.from(rotated.code,'base64'));assert.notEqual(decoded.psk,nodes[0].credential);
  assert.equal(decoded.coordinator_certificate,nodes[0].certificate,'credential rotation must not replace coordinator trust');
  await writeFile(nodes[0].privateFile,JSON.stringify(rotated));
  old=(await coordinator(['diagnostics'])).runtime_id;await coordinator(['restart']);
  await until(async()=>(await coordinator(['diagnostics'])).runtime_id!==old,'rotation apply');
  await cli('eefn',nodes[0].config,['connection','pair','--code-file',nodes[0].privateFile]);
  await until(async()=>(await coordinator(['diagnostics'])).registered_node_count===2,'rotated node reconnect');
  const results={passed:true,command_pairing:true,two_independent_credentials:true,legacy_disable:true,revocation_and_rotation:true,
    model_free_structured_review:true,
    redacted_inspection:true,generic_save_preserves_policy:true,node_id_and_permissions_preserved:true,physical_two_pc:false,
    execution_grants:false,execution_attempt_replay_fencing:false};
  results.pinned_tls13=true;results.persistent_coordinator_identity=true;
  results.credential_only_boot_without_legacy_secret=true;results.invalid_legacy_reenable_rejected_atomically=true;
  results.non_model_dispatch_and_rejected_constraints=true;
  await writeFile(join(scratch,'results.json'),JSON.stringify(results,null,2));console.log('Node credential validation passed: '+scratch);
} finally {
  for(const child of children)if(child.exitCode===null)child.kill();
  await Promise.allSettled(children.map(c=>c.result));
}
