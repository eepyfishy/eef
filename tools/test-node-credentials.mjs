// Public-command two-logical-node fixture, on one PC. No models or installed nodes.
import assert from 'node:assert/strict';
import {mkdtemp, readFile, writeFile} from 'node:fs/promises';
import {resolve, join} from 'node:path';
import {spawn} from 'node:child_process';
import {createServer} from 'node:net';
import {createServer as createHttpServer} from 'node:http';
import {randomBytes, randomUUID} from 'node:crypto';
import {reviewedFixture} from './reviewed-fixture.mjs';
const root = resolve(import.meta.dirname, '..');
const scratch = await mkdtemp(join(root, '.validation/node-credentials-'));
const bin = resolve(process.env.EEF_TEST_BINARY_DIR || join(root, 'target/debug'));
const children = [], eefConfig = join(scratch, 'eef.yaml');
let coordinatorProcess;
const secret = randomBytes(24).toString('hex');
const env = {...process.env, EEF_NODE_PSK:'', APPDATA:join(scratch,'appdata'),
  EEF_DISCOVERY_DIR:join(scratch,'discovery'), PATH:join(root,'.tooling/llvm-mingw-20260616-ucrt-x86_64/bin')+';'+process.env.PATH};
function launch(name,args,extra={},input=null) {
  const child = spawn(join(bin,name+'.exe'),args,{cwd:root,windowsHide:true,env:{...env,...extra},stdio:[input===null?'ignore':'pipe','pipe','pipe']});
  if(input!==null){child.stdin.on('error',()=>{});child.stdin.end(input);}
  let out='',err='';child.stdout.on('data',b=>out=(out+b).slice(-1048576));child.stderr.on('data',b=>err=(err+b).slice(-1048576));
  child.result=new Promise((resolve,reject)=>{child.on('error',reject);child.on('close',code=>resolve({code,out,err}));});
  children.push(child);return child;
}
async function cli(name,config,args,success=true,input=null) {
  const child=launch(name,['--config',config,'--json',...args],{},input);const timer=setTimeout(()=>child.kill(),30000);
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
    const beforePlan=await readFile(config),beforeCoordinatorPlan=await readFile(eefConfig);
    const localPlan=await cli('eefn',config,['setup','--local']);
    assert.equal(localPlan.state,'unprovisioned');assert.equal(localPlan.pairing_submitted,false);
    assert.equal(localPlan.changed,false);assert.equal(localPlan.restart_requested,false);
    assert((await readFile(config)).equals(beforePlan),'setup plan must not change node settings');
    assert((await readFile(eefConfig)).equals(beforeCoordinatorPlan),'setup plan must not provision trust');
    const invitation=await coordinator(['credentials','provision','--node',id,'--address',`127.0.0.1:${gatewayPort}`]);
    const beforeExport=await readFile(eefConfig);
    const recovered=await coordinator(['credentials','export','--node',id,'--address',`127.0.0.1:${gatewayPort}`]);
    assert(recovered.code===invitation.code,'export must preserve the private invitation');assert.equal(recovered.private,true);
    assert.equal(recovered.changed,false);assert.equal(recovered.restart_required,true);
    assert.equal(recovered.execution_authorized,false);
    assert((await readFile(eefConfig)).equals(beforeExport),'private export must not write or rotate credentials');
    const privateFile=join(scratch,name+'-PRIVATE.json');
    if(name==='b') await writeFile(privateFile,Buffer.concat([Buffer.from([0xff,0xfe]),Buffer.from(JSON.stringify(invitation),'utf16le')]));
    assert.equal(invitation.private,true);assert.equal(invitation.restart_required,true);
    const decoded=JSON.parse(Buffer.from(invitation.code,'base64'));assert.equal(decoded.node_id,id);assert(decoded.psk!==secret,'node secret must differ from legacy key');
    assert.equal(decoded.schema_version,2);assert.equal(typeof decoded.coordinator_certificate,'string');assert(!('private_key' in decoded));
    nodes.push({id,config,api,privateFile,pipe:name==='a',credential:decoded.psk,certificate:decoded.coordinator_certificate});
  }
  assert(nodes[0].credential!==nodes[1].credential,'node credentials must be independent');
  assert.equal(nodes[0].certificate,nodes[1].certificate,'persistent coordinator identity must not change per node');
  const show=await coordinator(['credentials','show']);assert.equal(show.saved_policy.identities.length,2);assert.equal(show.applied_policy.identities.length,0);
  for(const node of nodes) assert(!JSON.stringify(show).includes(node.credential));
  const previous=(await coordinator(['diagnostics'])).runtime_id;
  await coordinator(['restart']);
  await until(async()=>(await coordinator(['diagnostics'])).runtime_id!==previous,'apply provisioned credentials');
  const credentialRuntime=(await coordinator(['diagnostics'])).runtime_id;
  const exportBody={schema_version:1,expected_runtime_id:credentialRuntime,
    command:{operation:'export',node:nodes[0].id,address:`127.0.0.1:${gatewayPort}`}};
  const beforeRejectedExport=await readFile(eefConfig);
  for (const [body,expectedStatus] of [
    [{...exportBody,expected_runtime_id:previous},400],
    [{...exportBody,command:{...exportBody.command,rotate:true}},422],
    [{...exportBody,command:{...exportBody.command,node:'missing-node'}},400],
  ]) {
    const response=await fetch(base+'/api/commands/credentials',{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify(body)});
    assert.equal(response.status,expectedStatus);
    const text=await response.text();
    for (const node of nodes) assert(!text.includes(node.credential),'rejected exports must not return a credential');
  }
  const crossSite=await fetch(base+'/api/commands/credentials',{method:'POST',headers:{'Content-Type':'application/json',Origin:'https://untrusted.example'},body:JSON.stringify(exportBody)});
  assert.equal(crossSite.status,403);
  assert((await readFile(eefConfig)).equals(beforeRejectedExport),'rejected export must not write');
  for(const node of nodes) {
    const reportExport=await coordinator(['credentials','export','--node',node.id,'--address',`127.0.0.1:${gatewayPort}`]);
    assert.equal(reportExport.changed,false);assert.equal(reportExport.restart_required,false);
    const exported=JSON.parse(Buffer.from(reportExport.code,'base64'));
    assert(exported.psk===node.credential,'export must not rotate');assert.equal(exported.coordinator_certificate,node.certificate);
    // Simulate a lost invitation: pair from the fresh export, not the provisioning reply.
    let report;
    if(node.pipe) {
      report=await cli('eefn',node.config,['connection','pair-stdin'],true,JSON.stringify(reportExport));
      await assert.rejects(readFile(node.privateFile),{code:'ENOENT'},'file-free recovery must not create an invitation file');
    } else {
      await writeFile(node.privateFile,Buffer.concat([Buffer.from([0xff,0xfe]),Buffer.from(JSON.stringify(reportExport),'utf16le')]));
      report=await cli('eefn',node.config,['connection','pair','--code-file',node.privateFile]);
    }
    assert.equal(report.saved_auth_mode,'node_credential');assert.equal(report.restart_requested,true);
    assert(!JSON.stringify(report).includes(node.credential));
    await until(async()=>(await json(node.api+'/api/status')).connection?.state==='connected','credential node connected');
    const config=JSON.parse(await readFile(node.config,'utf8'));
    assert.equal(config.node_id,node.id);assert(config.psk===node.credential,'pairing must save the exact private credential');assert.equal(config.auth_mode,'node_credential');
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
    // Well-shaped unknown names reach eef's fresh disclosure check; malformed
    // names still fail locally before submission. Neither can authorize work.
    assert.equal((await json(endpoint,{...request,proposal:{...proposal,suggested_capabilities:['invented.capability']}})).error_code,'review_rejected');
    assert.equal((await json(endpoint,{...request,proposal:{...proposal,suggested_capabilities:['invalid name']}})).error_code,'invalid_supplied_proposal');
    assert.equal((await fetch(endpoint,{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify({...request,permissions:true})})).status,422);
    assert.equal((await fetch(endpoint,{method:'POST',headers:{'Content-Type':'application/json',Origin:'https://untrusted.example'},body:JSON.stringify(request)})).status,403);
    const jobs=await cli('eefn',node.config,['jobs','list']);assert.equal(jobs.data.total,0);assert.deepEqual(jobs.data.jobs,[]);
    await cli('eefn',node.config,['connection','pair-local'],false);
  }
  await until(async()=>(await coordinator(['diagnostics'])).registered_node_count===2,'two registered nodes');
  for(const node of nodes) assert.equal((await coordinator(['diagnostics','--node',node.id,'--samples','1'])).passed,1);
  // Exercise reviewed, owner-approved non-model grants over pinned TLS.
  for(const node of nodes) {
    const reviewed=reviewedFixture(base,node.api,node.id);
    const plan=await reviewed.save({template:'system_info',params:{},constraints:{node_id:node.id}});
    await reviewed.start(await reviewed.approve(plan));
    assert.equal((await reviewed.wait(plan.id)).status,'completed');
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
  const ownerAdvertisement=JSON.parse(await readFile(join(scratch,'discovery/owner-eef.json'),'utf8'));
  assert.deepEqual(Object.keys(ownerAdvertisement).sort(),['node_address','owner_address','runtime_id','schema_version']);
  assert.equal(ownerAdvertisement.runtime_id,(await coordinator(['diagnostics'])).runtime_id);
  assert.equal((await cli('eefn',nodes[0].config,['setup','--local'])).state,'provisioned','private discovery works with legacy disabled');
  const ownerPath=join(scratch,'discovery/owner-eef.json'),ownerBytes=await readFile(ownerPath);
  try {
    await writeFile(ownerPath,JSON.stringify({...ownerAdvertisement,runtime_id:randomUUID()}));
    const stalePlan=await cli('eefn',nodes[0].config,['setup','--local']);
    assert.equal(stalePlan.state,'stale');assert.equal(stalePlan.coordinator_runtime_id,null);
    assert.equal(stalePlan.coordinator_fingerprint,null);assert.equal(stalePlan.pairing_submitted,false);
    await writeFile(ownerPath,JSON.stringify({...ownerAdvertisement,psk:'PRIVATE-UNEXPECTED-METADATA'}));
    const refused=await cli('eefn',nodes[0].config,['setup','--local'],false);
    assert(!JSON.stringify(refused).includes('PRIVATE-UNEXPECTED-METADATA'));
  } finally {await writeFile(ownerPath,ownerBytes);} // Restore only this fixture's fault injection.
  const planRuntime=(await json(nodes[0].api+'/api/diagnostics')).runtime_id;
  const planRequest={schema_version:1,expected_node_id:nodes[0].id,expected_runtime_id:planRuntime,request_id:randomUUID()};
  for(const [body,expectedStatus,origin] of [
    [{...planRequest,expected_node_id:'other'},400,null],
    [{...planRequest,rotate:true},422,null],
    [planRequest,403,'https://untrusted.example'],
  ]) {
    const response=await fetch(nodes[0].api+'/api/commands/setup/local',{method:'POST',
      headers:{'Content-Type':'application/json',...(origin?{Origin:origin}:{})},body:JSON.stringify(body)});
    assert.equal(response.status,expectedStatus);
  }
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
  assert.equal((await cli('eefn',nodes[0].config,['setup','--local'])).state,'revoked','setup honors saved revocation before restart');
  const savedRevocation=await readFile(eefConfig);
  await coordinator(['credentials','export','--node',nodes[0].id,'--address',`127.0.0.1:${gatewayPort}`],false);
  assert((await readFile(eefConfig)).equals(savedRevocation),'export cannot restore a saved revocation even before restart');
  assert.equal((await coordinator(['diagnostics','--node',nodes[0].id,'--samples','1'])).passed,1,'saved revocation is not immediate');
  old=(await coordinator(['diagnostics'])).runtime_id;await coordinator(['restart']);
  await until(async()=>{const d=await coordinator(['diagnostics']);return d.runtime_id!==old&&d.registered_node_count===1;},'revocation applied');
  const inventory=await coordinator(['models','list']);assert.deepEqual(inventory.nodes.map(n=>n.node_id),[nodes[1].id]);
  assert.equal((await coordinator(['diagnostics','--node',nodes[1].id,'--samples','1'])).passed,1);
  await coordinator(['credentials','provision','--node',nodes[0].id,'--address',`127.0.0.1:${gatewayPort}`],false);
  // Re-provision is explicit rotation, followed by independent restart/pairing.
  const rotated=await coordinator(['credentials','provision','--node',nodes[0].id,'--address',`127.0.0.1:${gatewayPort}`,'--rotate']);
  const decoded=JSON.parse(Buffer.from(rotated.code,'base64'));assert(decoded.psk!==nodes[0].credential,'explicit rotation must change the private credential');
  assert.equal(decoded.coordinator_certificate,nodes[0].certificate,'credential rotation must not replace coordinator trust');
  await writeFile(nodes[0].privateFile,JSON.stringify(rotated));
  old=(await coordinator(['diagnostics'])).runtime_id;await coordinator(['restart']);
  await until(async()=>(await coordinator(['diagnostics'])).runtime_id!==old,'rotation apply');
  await cli('eefn',nodes[0].config,['connection','pair','--code-file',nodes[0].privateFile]);
  await until(async()=>(await coordinator(['diagnostics'])).registered_node_count===2,'rotated node reconnect');

  // A private-export adapter may print only the exact confirmed private report.
  // This loopback mock tests reply substitution, not coordinator authentication.
  const goodExport=await coordinator(['credentials','export','--node',nodes[0].id,'--address',`127.0.0.1:${gatewayPort}`]);
  const state=await coordinator(['credentials','show']);
  let mockReply=goodExport;
  const mock=createHttpServer((req,res)=>{
    res.setHeader('Content-Type','application/json');
    if(req.url!=='/api/commands/credentials'){res.writeHead(404);res.end('{}');return;}
    req.resume();res.end(JSON.stringify(req.method==='GET'?state:mockReply));
  });
  await new Promise(resolve=>mock.listen(0,'127.0.0.1',resolve));
  try {
    const mockConfig=join(scratch,'mock-export.yaml');
    await writeFile(mockConfig,`web:\n  host: 127.0.0.1\n  port: ${mock.address().port}\n`);
    assert((await cli('eef',mockConfig,['credentials','export','--node',nodes[0].id,'--address',`127.0.0.1:${gatewayPort}`])).code===goodExport.code,'confirmed private export must pass unchanged');
    for(const [field,value] of [
      ['runtime_id',randomUUID()],['changed',true],['execution_authorized',true],
      ['unexpected_secret',decoded.psk],['note',goodExport.code],
    ]) {
      mockReply={...goodExport,[field]:value};
      const rejected=await cli('eef',mockConfig,['credentials','export','--node',nodes[0].id,'--address',`127.0.0.1:${gatewayPort}`],false);
      assert(!JSON.stringify(rejected).includes(goodExport.code));
      assert(!JSON.stringify(rejected).includes(decoded.psk));
      assert(!('code' in rejected));
    }
  } finally {await new Promise(resolve=>mock.close(resolve));}
  const results={passed:true,command_pairing:true,two_independent_credentials:true,legacy_disable:true,revocation_and_rotation:true,
    model_free_structured_review:true,
    redacted_inspection:true,generic_save_preserves_policy:true,node_id_and_permissions_preserved:true,physical_two_pc:false,
    execution_grants:true,execution_attempt_replay_fencing:false};
  results.pinned_tls13=true;results.persistent_coordinator_identity=true;
  results.credential_only_boot_without_legacy_secret=true;results.invalid_legacy_reenable_rejected_atomically=true;
  results.non_model_dispatch_and_rejected_constraints=true;
  results.private_export_without_rotation=true;results.export_saved_revocation_and_runtime_guards=true;
  results.export_to_file_free_pairing=true;
  results.export_cli_substituted_reply_redaction=true;
  results.port_free_read_only_local_setup_plan=true;results.secret_free_discovery_with_legacy_disabled=true;
  results.local_setup_stale_corrupt_and_owner_boundary_guards=true;
  await writeFile(join(scratch,'results.json'),JSON.stringify(results,null,2));console.log('Node credential validation passed: '+scratch);
} finally {
  for(const child of children)if(child.exitCode===null)child.kill();
  await Promise.allSettled(children.map(c=>c.result));
}
