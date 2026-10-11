// API-only node -> EEF review, using fake inference and isolated local data.
// Review/preparation stay idle; separate owner-approved grant execution uses a fake backend.
import assert from 'node:assert/strict';
import {mkdtemp, readFile, writeFile} from 'node:fs/promises';
import {resolve, join} from 'node:path';
import {spawn} from 'node:child_process';
import {createServer} from 'node:http';
import {randomUUID, randomBytes} from 'node:crypto';
import {DatabaseSync} from 'node:sqlite';
const root = resolve(import.meta.dirname, '..');
const scratch = await mkdtemp(join(root, '.validation/request-review-'));
const bin = resolve(process.env.EEF_TEST_BINARY_DIR || join(root, 'target/debug'));
const configPath = join(scratch, 'node.json'), eefConfig = join(scratch, 'eef.yaml');
const marker = configPath + '.api.json', children = [], calls = [], gates = new Set(), recoveryGates = new Set();
const env = {...process.env, EEF_NODE_PSK: '', APPDATA: join(scratch, 'appdata'),
  EEF_DISCOVERY_DIR: join(scratch, 'discovery'), PATH: join(root, '.tooling/llvm-mingw-20260616-ucrt-x86_64/bin') + ';' + process.env.PATH};
function launch(name, args, extra = {}) {
  const child = spawn(join(bin, name + '.exe'), args, {cwd: root, windowsHide: true,
    env: {...env, ...extra}, stdio: ['ignore', 'pipe', 'pipe']});
  let out = '', err = '';
  child.stdout.on('data', b => out = (out + b).slice(-1048576));
  child.stderr.on('data', b => err = (err + b).slice(-1048576));
  child.result = new Promise((resolve, reject) => {child.on('error', reject); child.on('close', code => resolve({code, out, err}));});
  children.push(child); return child;
}
async function cli(args, success = true, nodeConfig = configPath) {
  const child = launch('eefn', ['--config', nodeConfig, ...args, '--json']);
  const timer = setTimeout(() => child.kill(), 45000);
  try {const result = await child.result; assert.equal(result.code, success ? 0 : 1, result.out + result.err); return JSON.parse(result.out);}
  finally {clearTimeout(timer);}
}
async function coordinator(args) {
  const child=launch('eef',['--config',eefConfig,'--json',...args]),timer=setTimeout(()=>child.kill(),30000);
  try {const r=await child.result;assert.equal(r.code,0,r.out+r.err);return JSON.parse(r.out);}finally {clearTimeout(timer);}
}
const review = (text, success = true) => cli(['requests', 'review', '--text', text], success);
const preview = text => cli(['requests', 'preview', '--text', text]);
async function until(fn, label) {const deadline = Date.now() + 45000; while (Date.now() < deadline) {try {if (await fn()) return;} catch {} await new Promise(r => setTimeout(r, 100));} throw Error('Timeout: ' + label);}
async function json(url, body, method) {
  const response = await fetch(url, {method: method || (body ? 'POST' : 'GET'), headers: {'Content-Type': 'application/json'},
    body: body ? JSON.stringify(body) : undefined, signal: AbortSignal.timeout(10000)});
  const value = await response.json(); assert(response.ok, JSON.stringify(value)); return value;
}
async function listen(server) {await new Promise(r => server.listen(0, '127.0.0.1', r)); return server.address().port;}
async function port() {const server = createServer(); const value = await listen(server); await new Promise(r => server.close(r)); return value;}
let mode = 'valid', node = '', proxyMode = 'drop', forwarded = 0, expectedNodes = 1;
const proposal = () => ({schema_version: 1, intent: 'information', goal: 'Inspect node status', context_hints: ['this node'],
  constraints: [], suggested_capabilities: ['system.info'], complexity: 'simple', workload: 'one_shot', needs_clarification: false, clarification: null});
const fake = createServer(async (req, res) => {
  res.setHeader('Content-Type', 'application/json');
  if (req.url === '/api/tags') return res.end(JSON.stringify({models: [{name: 'fixture'}]}));
  if (req.url === '/api/show') return res.end(JSON.stringify({capabilities: ['completion']}));
  if (req.url === '/api/chat') {
    let body = ''; for await (const chunk of req) body += chunk;
    calls.push(JSON.parse(body));
    if (mode === 'gate') {res.write(' '); gates.add(res); res.on('close', () => gates.delete(res)); return;}
    if (mode === 'recovery-gate') {res.write(' '); recoveryGates.add(res); res.on('close', () => recoveryGates.delete(res)); return;}
    if (mode === 'text-execution') return res.end(JSON.stringify({message:{content:'fixture concrete grant result'},done:true,done_reason:'stop'}));
    if (mode === 'method' || mode === 'method-question' || mode === 'method-invalid' || mode === 'method-remote') {
      const value = mode === 'method-question' ? {schema_version:1,method:null,clarification:'Which node should be inspected?'}
        : {schema_version:1,method:{schema_version:1,template:'system_info',params:{},constraints:{}},clarification:null};
      if (mode === 'method-invalid') value.method.constraints.node_id='invented';
      if (mode === 'method-remote') value.method={schema_version:1,template:'http_request',params:{url:'https://example.invalid/info',method:'GET'},constraints:{}};
      return res.end(JSON.stringify({message:{content:JSON.stringify(value)},done:true,done_reason:'stop'}));
    }
    const value = proposal();
    if (mode === 'remote') Object.assign(value, {goal: 'Fetch information using another node', suggested_capabilities: ['http.request']});
    if (mode === 'clarify') Object.assign(value, {needs_clarification: true, clarification: 'Which node do you mean?'});
    if (mode === 'invalid') value.permissions = ['everything'];
    return res.end(JSON.stringify({message: {content: JSON.stringify(value)}, done: true, done_reason: 'stop'}));
  }
  res.writeHead(404); res.end('{}');
});
const proxy = createServer(async (req, res) => {
  forwarded++;
  if (proxyMode === 'old') {res.writeHead(404, {'Content-Type': 'application/json'}); res.end('{}'); return;}
  try {
    let body = ''; for await (const chunk of req) body += chunk;
    const response = await fetch(node + req.url, {method: 'POST', headers: {'Content-Type': 'application/json'}, body});
    const report = await response.json();
    if (proxyMode === 'corrupt') {
      report.result.request_context.origin_node = 'forged';
      res.setHeader('Content-Type', 'application/json'); res.end(JSON.stringify(report));
    } else if (proxyMode === 'changed-proposal') {
      report.result.interpretation.proposal.goal = 'Different schema-valid goal';
      res.setHeader('Content-Type', 'application/json'); res.end(JSON.stringify(report));
    } else res.destroy();
  } catch {res.destroy();}
});
try {
  const fakePort = await listen(fake), proxyPort = await listen(proxy);
  const [apiPort, eefPort, gatewayPort] = await Promise.all([port(), port(), port()]);
  const eef = `http://127.0.0.1:${eefPort}`, secret = randomBytes(24).toString('hex');
  const config = JSON.parse(await readFile(join(root, 'config/node.example.json'), 'utf8'));
  Object.assign(config, {node_id: 'review-fixture-node', name: 'Review fixture', psk: secret, auto_local: false,
    local_pairing: false, endpoints: [`127.0.0.1:${gatewayPort}`], heartbeat_seconds: 1, connection_enabled: true,
    metadata: {area: ['Home', 'Bedroom']}, update: {policy: 'off'},
    dashboard: {enabled: true, ui_enabled: false, host: '127.0.0.1', port: apiPort},
    models: {provider: 'ollama', ollama: {base_url: `http://127.0.0.1:${fakePort}`,
      selected: [{model_id: 'fixture', modality: 'text', selection: {roles: ['request_interpreter']}}]}, llamacpp: {slots: []}}});
  await writeFile(configPath, JSON.stringify(config));
  assert.equal((await review('Check status', false)).error_code, 'node_not_running'); assert.equal(calls.length, 0);
  const notRunningBytes=await readFile(configPath,'utf8');
  assert.equal((await cli(['--ask','Input before starting node'],false)).error_code,'node_not_running');
  assert.equal(await readFile(configPath,'utf8'),notRunningBytes);assert.equal(calls.length,0);
  const yaml = (await readFile(join(root, 'config/default_identity.yaml'), 'utf8'))
    .replace('port: 51334', `port: ${eefPort}`).replace('port: 51335', `port: ${gatewayPort}`).replace('policy: prompt', 'policy: off');
  await writeFile(eefConfig, yaml);
  launch('eef', ['--config', eefConfig, '--database', join(scratch, 'eef.db'), '--no-brain', '--no-ui'], {EEF_NODE_PSK: secret});
  launch('eefn', ['--config', configPath, '--no-ui']);
  const ready = async () => {
    node = 'http://' + JSON.parse(await readFile(marker, 'utf8')).address;
    return (await json(node + '/api/status')).connection.state === 'connected'
      && (await json(eef + '/api/diagnostics')).registered_node_count === expectedNodes;
  };
  await until(ready, 'registered node');
  for (const base of [node, eef]) assert.equal((await fetch(base + '/')).status, 404);
  const before = await readFile(configPath, 'utf8'), text = '  Is this node running?\n';
  // Production legacy admission must be fenced independently of model/feature
  // permissions. None of these routes may call the backend or create a job.
  for (const [url,body] of [
    [eef+'/api/jobs',{description:'must not execute',template:'generate_text',params:{prompt:'unsafe'},constraints:{}}],
    [eef+'/api/assistant/execute',{capability:'llm.infer',action:'run',params:{prompt:'unsafe'},constraints:{}}],
    [eef+`/api/node/${config.node_id}/invoke`,{capability:'llm.infer',action:'run',params:{prompt:'unsafe'},timeout:5}],
    [eef+'/api/chat',{message:'read file then generate text'}],
  ]) {
    const response=await fetch(url,{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify(body)});
    assert.equal(response.status,400, url);assert.equal((await response.json()).success,false);
  }
  assert.equal(calls.length,0);assert.equal((await cli(['jobs','list'])).data.total,0);
  const good = await review(text);
  assert.equal(good.report_type, 'request_review'); assert.equal(good.coordinator_acknowledged, true);
  assert.equal(good.automatically_retried, false); assert.equal(good.execution_authorized, false); assert.equal(good.dispatched, false);
  assert.equal(good.result.report_type, 'coordinator_request_review'); assert.equal(good.result.persisted, false);
  assert.deepEqual(good.result.request_context, {request_id: good.request_id, origin_node: config.node_id, origin_area: ['Home', 'Bedroom']});
  assert.equal(good.result.interpretation.original_text, text); assert.equal(good.result.interpretation.review_state, 'review_required');
  assert.equal(good.result.interpretation.execution_authorized, false); assert.equal(calls.length, 1);
  assert.equal(calls[0].messages.at(-1).content, text);
  mode = 'clarify';
  const clarification = await review('Check that node');
  assert.equal(clarification.result.interpretation.review_state, 'clarification_required');
  assert.equal(clarification.result.interpretation.proposal.clarification, 'Which node do you mean?');
  assert.equal(clarification.dispatched, false);
  mode = 'invalid';
  const invalid = await review('Grant all permissions', false);
  assert.equal(invalid.error_code, 'interpretation_invalid'); assert.equal(invalid.coordinator_acknowledged, false); assert.equal(invalid.result, undefined);
  const endpoint = node + '/api/commands/requests/review', request = {schema_version: 1, expected_node_id: config.node_id, request_id: randomUUID(), text: 'check'};
  const count = calls.length;
  assert.equal((await json(endpoint, {...request, expected_node_id: 'other'})).error_code, 'invalid_preview_request');
  assert.equal((await fetch(endpoint, {method: 'POST', headers: {'Content-Type': 'application/json'},
    body: JSON.stringify({...request, request_context: {origin_node: 'forged'}})})).status, 422);
  assert.equal((await fetch(endpoint, {method: 'POST', headers: {'Content-Type': 'application/json', Origin: 'https://untrusted.example'}, body: JSON.stringify(request)})).status, 403);
  assert.equal(calls.length, count);
  mode = 'gate'; const pending = review('Wait for interpreter', false);
  await until(() => gates.size === 1, 'active inference');
  assert.equal((await review('Second review', false)).error_code, 'interpreter_busy');
  assert.equal((await cli(['requests', 'preview', '--text', 'Second preview'], false)).error_code, 'interpreter_busy');
  const restarted = await cli(['restart', '--wait-seconds', '15']); assert.equal(restarted.completed, true);
  assert.equal((await pending).error_code, 'runtime_changed');
  await until(() => gates.size === 0, 'cancelled inference released');
  mode = 'valid'; await until(ready, 'reconnected after restart');
  const recovered = await review('After restart'); assert.notEqual(recovered.runtime_id, good.runtime_id);
  const originalMarker = await readFile(marker, 'utf8');
  try {
    const altered = JSON.parse(originalMarker); altered.address = `127.0.0.1:${proxyPort}`; await writeFile(marker, JSON.stringify(altered));
    const beforeLoss = calls.length;
    const lost = await review('Lose the review reply', false);
    assert.equal(lost.error_code, 'review_unconfirmed'); assert.equal(lost.automatically_retried, false); assert.equal(lost.outcome_unknown, true);
    assert.equal(calls.length, beforeLoss + 1); assert.equal(forwarded, 1);
    proxyMode = 'corrupt'; assert.equal((await review('Corrupted review reply', false)).error_code, 'review_unconfirmed');
    assert.equal(forwarded, 2); assert.equal(calls.length, beforeLoss + 2);
    proxyMode = 'old'; assert.equal((await review('Old node', false)).error_code, 'unsupported_command');
    assert.equal(calls.length, beforeLoss + 2); assert.equal(forwarded, 3);
    const proposalFile=join(scratch,'supplied.json');
    await writeFile(proposalFile,JSON.stringify({schema_version:1,original_text:'Original supplied goal',proposal:proposal()}));
    proxyMode='changed-proposal';
    const changed=await cli(['requests','review-proposal','--file',proposalFile],false);
    assert.equal(changed.error_code,'review_unconfirmed');assert.equal(changed.automatically_retried,false);
    assert.equal(calls.length,beforeLoss+2,'supplied review cannot call the interpreter');assert.equal(forwarded,4);
  } finally {await writeFile(marker, originalMarker);}
  await cli(['connection', 'pause']); await until(async () => (await json(node + '/api/status')).connection.state === 'paused', 'paused connection');
  const disconnected = await review('No coordinator', false);
  assert.equal(disconnected.error_code, 'coordinator_context_unavailable'); assert.equal(disconnected.outcome_unknown, false);
  assert.equal(disconnected.coordinator_acknowledged, false); assert.equal(disconnected.result, undefined);
  assert.equal((await preview('Local preview still works')).success, true);
  await cli(['connection', 'resume']); await until(ready, 'resumed connection');
  const jobs = await cli(['jobs', 'list']); assert.deepEqual(jobs.data.jobs, []); assert.equal(jobs.data.total, 0);
  // --ask never creates a competing node and /api/chat is only a durable-review
  // adapter. Neither infers a keyword task or dispatches an unreviewed workload.
  mode='valid';
  for (const input of [await cli(['--ask','  Normal input through running node\n']),
    await json(node+'/api/chat',{message:'  HTTP input through node\n',request_id:randomUUID()})]) {
    assert.equal(input.report_type,'request_draft');assert.equal(input.success,true);
    assert.equal(input.execution_authorized,false);assert.equal(input.dispatched,false);
    assert.equal(input.result.request_context.origin_node,config.node_id);
    assert.equal(input.result.status,'review');
    await cli(['jobs','stop',input.result.id]);await cli(['jobs','remove',input.result.id]);
  }
  assert.equal((await json(eef+'/api/diagnostics')).registered_node_count,1);
  const generated=await cli(['jobs','generate-text','--prompt','  Text creation is review only\n','--model','fixture','--backend','ollama']);
  assert.equal(generated.submission_mode,'reviewed_concrete_request');
  assert.equal(generated.data.status,'awaiting_approval');assert.equal(generated.dispatched,false);
  await cli(['jobs','stop',generated.data.id]);await cli(['jobs','remove',generated.data.id]);
  // The optional natural-language adapter can retain a proposal, never execute it.
  mode='clarify';
  const durable=await cli(['requests','open','--text','  Inspect that node\n']);
  assert.equal(durable.report_type,'request_draft');assert.equal(durable.coordinator_acknowledged,true);
  assert.equal(durable.result.status,'review');assert.equal(durable.result.review_state,'clarification_required');
  assert.equal(durable.result.draft.original_text,'  Inspect that node\n');
  assert.equal(durable.result.draft.proposals[0].proposal.clarification,'Which node do you mean?');
  assert.equal(durable.execution_authorized,false);assert.equal(durable.dispatched,false);
  const uncertainMarker=await readFile(marker,'utf8'),beforeDurableLoss=calls.length,beforeForwarded=forwarded;
  let lostDraft;
  try {
    const altered=JSON.parse(uncertainMarker);altered.address=`127.0.0.1:${proxyPort}`;await writeFile(marker,JSON.stringify(altered));
    proxyMode='drop';lostDraft=await cli(['requests','open','--text','Lost durable reply'],false);
    assert.equal(lostDraft.error_code,'review_unconfirmed');assert.equal(lostDraft.outcome_unknown,true);
    assert.equal(lostDraft.automatically_retried,false);assert.equal(calls.length,beforeDurableLoss+1);assert.equal(forwarded,beforeForwarded+1);
  } finally {await writeFile(marker,uncertainMarker);}
  const found=(await cli(['jobs','find','--operation-id',lostDraft.request_id])).data;
  assert.equal(found.total,1);assert.equal(found.jobs[0].status,'review');assert.equal(found.jobs[0].review_only,true);
  const retained=(await cli(['requests','draft',found.jobs[0].id])).result;
  assert.equal(retained.draft.original_text,'Lost durable reply');assert.equal(retained.request_context.request_id,lostDraft.request_id);
  assert.deepEqual((await cli(['jobs','get',found.jobs[0].id])).data.tasks,[]);
  assert.equal(calls.length,beforeDurableLoss+1,'receipt inspection must not infer or resubmit');
  // Concrete model binding uses live advertised roles; compiling cannot infer.
  const modelProposalFile=join(scratch,'model-proposal.json');
  await writeFile(modelProposalFile,JSON.stringify({schema_version:1,original_text:'Summarize this text',proposal:{...proposal(),suggested_capabilities:['llm.infer']}}));
  const modelDraft=await cli(['requests','open-proposal','--file',modelProposalFile]);
  const runtime=(await json(eef+'/api/diagnostics')).runtime_id;
  const planInput={schema_version:1,template:'generate_text',params:{prompt:'Summarize this text',model_id:'fixture',max_tokens:64},
    constraints:{node_id:config.node_id,backend:'ollama',model_role:'request_interpreter'}};
  const planRequest={schema_version:1,expected_runtime_id:runtime,command:{operation:'compile',id:modelDraft.result.id,
    expected_revision:1,operation_id:randomUUID(),input:planInput}};
  const planUrl=eef+'/api/commands/requests/planning';
  const beforeCompile=calls.length;
  const wrongRole=await fetch(planUrl,{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify({...planRequest,
    command:{...planRequest.command,input:{...planInput,constraints:{...planInput.constraints,model_role:'unselected_role'}}}})});
  assert.equal(wrongRole.status,400);
  const modelPlan=await json(planUrl,planRequest);
  assert.equal(modelPlan.plan.status,'awaiting_approval');assert.equal(modelPlan.execution_authorized,false);assert.equal(modelPlan.dispatched,false);
  assert.equal(modelPlan.plan.tasks[0].params.model_id,'fixture');assert.equal(modelPlan.plan.tasks[0].constraints.backend,'ollama');
  assert.deepEqual(modelPlan.plan.planning.bindings[0],{task_id:modelPlan.plan.tasks[0].task_id,node_id:config.node_id,resource_id:null,model_id:'fixture',backend:'ollama'});
  const nodeModelView=await cli(['requests','plan',modelDraft.result.id]);
  assert.equal(nodeModelView.result.plan_sha256,modelPlan.plan.planning.plan_sha256);
  assert.deepEqual(nodeModelView.result.bindings,modelPlan.plan.planning.bindings);
  assert.deepEqual(nodeModelView.result.input,planInput);
  assert.equal(nodeModelView.execution_authorized,false);assert.equal(nodeModelView.dispatched,false);
  assert.equal(calls.length,beforeCompile,'compilation must not invoke inference');
  const modelReused=await json(planUrl,planRequest);assert.equal(modelReused.reused,true);assert.deepEqual(modelReused.plan,modelPlan.plan);
  // A legacy shared-secret route cannot satisfy the owner approval/grant contract.
  const legacyApproval=await fetch(planUrl,{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify({
    schema_version:1,expected_runtime_id:runtime,command:{operation:'approve',id:modelDraft.result.id,expected_revision:2,
      operation_id:randomUUID(),plan_sha256:modelPlan.plan.planning.plan_sha256,valid_for_ms:60000,
      limits:{max_execution_ms:1000,max_result_bytes:16384}}})});
  assert.equal(legacyApproval.status,400);
  const afterDenied=await json(planUrl,{schema_version:1,expected_runtime_id:runtime,command:{operation:'inspect',id:modelDraft.result.id}});
  assert.deepEqual(afterDenied.plan,modelPlan.plan);assert.equal(calls.length,beforeCompile,'denied approval must not infer or execute');
  assert.deepEqual(JSON.parse(await readFile(configPath, 'utf8')), JSON.parse(before), 'review leaves config unchanged');
  // Explicitly migrate this isolated fixture to pinned TLS, then approve/execute
  // the already frozen model binding. Credential changes are intentional here.
  const invitation=await coordinator(['credentials','provision','--node',config.node_id,'--address',`127.0.0.1:${gatewayPort}`]);
  const privateCode=join(scratch,'PRIVATE-pairing.json');await writeFile(privateCode,JSON.stringify(invitation));
  await coordinator(['credentials','legacy','--allow','false']);await coordinator(['restart']);
  await until(async()=>(await json(eef+'/api/diagnostics')).runtime_id!==runtime,'apply fixture TLS issuer');
  await cli(['connection','pair','--code-file',privateCode]);
  await until(async()=>await ready()&&(await json(node+'/api/status')).connection.transport==='pinned_tls13','paired model node');
  assert.equal((await json(node+'/api/status')).execution_grants.timing_mode,'issuer_lease_challenge_v1');
  let activeRuntime=(await json(eef+'/api/diagnostics')).runtime_id;
  const approval=await json(planUrl,{schema_version:1,expected_runtime_id:activeRuntime,command:{operation:'approve',id:modelDraft.result.id,
    expected_revision:2,operation_id:randomUUID(),plan_sha256:modelPlan.plan.planning.plan_sha256,valid_for_ms:60000,
    limits:{max_execution_ms:5000,max_result_bytes:16384}}});
  assert.equal(approval.plan.status,'approved');assert.equal(calls.length,beforeCompile);
  const execution={id:modelDraft.result.id,expected_revision:3,operation_id:randomUUID(),approval_sha256:approval.plan.approval.approval_sha256};
  await cli(['models','hints','--backend','ollama','--model','fixture','--role','unselected_role']);
  await cli(['restart','--wait-seconds','15']);
  await until(async()=>await ready()&&(await json(eef+'/api/commands/models/route?capability=llm.infer&role=request_interpreter')).candidates.length===0,'withdraw approved role');
  const denied=await json(node+'/api/commands/requests/execute',{schema_version:1,expected_node_id:config.node_id,command:execution});
  assert.equal(denied.success,false);assert.equal(calls.length,beforeCompile);
  const unchanged=await json(planUrl,{schema_version:1,expected_runtime_id:activeRuntime,command:{operation:'inspect',id:modelDraft.result.id}});
  assert.deepEqual(unchanged.plan,approval.plan,'missing role must not save issuance or rebind');
  await cli(['models','hints','--backend','ollama','--model','fixture','--role','request_interpreter']);
  await cli(['restart','--wait-seconds','15']);
  await until(async()=>await ready()&&(await json(eef+'/api/commands/models/route?capability=llm.infer&role=request_interpreter')).candidates.length===1,'restore exact owner role');
  const executing=await cli(['requests','execute',execution.id,'--revision','3','--approval-digest',execution.approval_sha256,'--operation-id',execution.operation_id]);
  assert.equal(executing.success,true);
  await until(async()=>(await cli(['jobs','get',execution.id])).data.status==='completed','granted model completion');
  assert.equal(calls.length,beforeCompile+1,'one granted inference, no route fallback or extra planning call');
  assert.equal(calls.at(-1).model,'fixture');assert.equal(calls.at(-1).messages.at(-1).content,'Summarize this text');
  const completed=await json(planUrl,{schema_version:1,expected_runtime_id:activeRuntime,command:{operation:'inspect',id:execution.id}});
  const modelAttempt=completed.plan.execution.attempts[0];assert.equal(modelAttempt.state,'completed');
  assert.equal(modelAttempt.grant.model_id,'fixture');assert.equal(modelAttempt.grant.backend,'ollama');assert.equal(modelAttempt.grant.model_role,'request_interpreter');
  assert.deepEqual(modelAttempt.acknowledgement.state,'accepted');assert.equal(modelAttempt.report.success,true);
  const output=(await cli(['jobs','output',execution.id])).data;assert.equal(output.outputs.length,1);assert.equal(output.outputs[0].content,modelAttempt.report.data.content);
  const repeated=await cli(['requests','execute',execution.id,'--revision','3','--approval-digest',execution.approval_sha256,'--operation-id',execution.operation_id]);
  assert.equal(repeated.result.reused,true);assert.equal(calls.length,beforeCompile+1);
  await cli(['jobs','remove',modelDraft.result.id]);
  for(const id of [durable.result.id,found.jobs[0].id]) {await cli(['jobs','stop',id]);await cli(['jobs','remove',id]);}
  assert.equal((await cli(['jobs','list'])).data.total,0);
  // Explicit clarification resolution needs no hand-edited proposal file. It
  // pins the full source, uses one interpreter call, and saves only review data.
  mode='clarify';const resolutionOriginal='  Inspect that node after clarification\n';
  const resolvingDraft=await cli(['requests','open','--text',resolutionOriginal]);
  const resolutionId=resolvingDraft.result.id,resolveId=randomUUID();
  const resolveArgs=(id,revision,operationId)=>['requests','resolve',id,'--revision',String(revision),'--operation-id',operationId];
  let beforeResolution=calls.length;
  assert.equal((await cli(resolveArgs(resolutionId,1,randomUUID()),false)).error_code,'clarification_answer_required');
  const answeredDraft=await cli(['requests','answer',resolutionId,'--revision','1','--text','The bedroom node, keep the original request']);
  assert.equal((await cli(resolveArgs(resolutionId,1,randomUUID()),false)).error_code,'draft_revision_changed');
  assert.equal(calls.length,beforeResolution,'no answer or stale source must not invoke inference');
  const resolveEndpoint=node+'/api/commands/requests/resolve';
  const resolveRequest={schema_version:1,expected_node_id:config.node_id,command:{id:resolutionId,expected_revision:2,operation_id:resolveId}};
  assert.equal((await json(resolveEndpoint,{...resolveRequest,expected_node_id:'other'})).error_code,'invalid_resolution_request');
  assert.equal((await fetch(resolveEndpoint,{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify({...resolveRequest,proposal:{}})})).status,422);
  assert.equal((await fetch(resolveEndpoint,{method:'POST',headers:{'Content-Type':'application/json',Origin:'https://untrusted.example'},body:JSON.stringify(resolveRequest)})).status,403);
  mode='invalid';const invalidResolution=await cli(resolveArgs(resolutionId,2,randomUUID()),false);
  assert.equal(invalidResolution.error_code,'interpretation_invalid');assert.equal(invalidResolution.saving_attempted,false);
  assert.deepEqual((await cli(['requests','draft',resolutionId])).result.draft,answeredDraft.result.draft);
  mode='valid';const resolved=await cli(resolveArgs(resolutionId,2,resolveId));
  assert.equal(resolved.report_type,'request_resolution');assert.equal(resolved.proposal_saved,true);
  assert.equal(resolved.result.revision,3);assert.equal(resolved.result.status,'review');assert.equal(resolved.result.review_state,'review_required');
  assert.equal(resolved.result.draft.original_text,resolutionOriginal);
  assert.deepEqual(resolved.result.draft.answers,answeredDraft.result.draft.answers);
  assert.deepEqual(resolved.result.request_context,resolvingDraft.result.request_context);
  assert.equal(resolved.result.draft.proposals[1].resolution.source_revision,2);
  assert.equal(resolved.result.draft.proposals[1].resolution.backend,'ollama');
  assert.equal(resolved.result.draft.proposals[1].resolution.model_id,'fixture');
  assert.equal(calls.length,beforeResolution+2,'one invalid attempt and one explicit successful reinterpretation');
  assert.equal(calls.at(-1).messages[1].content,resolutionOriginal);
  assert(calls.at(-1).messages[2].content.includes('The bedroom node, keep the original request'));
  assert(calls.at(-1).messages[2].content.includes('Which node do you mean?'));
  assert.deepEqual((await cli(['jobs','get',resolutionId])).data.tasks,[]);
  assert.equal(resolved.execution_authorized,false);assert.equal(resolved.dispatched,false);
  const reusedResolution=await cli(resolveArgs(resolutionId,2,resolveId));
  assert.equal(reusedResolution.reused,true);assert.equal(reusedResolution.model_inference_requested,false);
  assert.equal(reusedResolution.saving_attempted,false);assert.equal(calls.length,beforeResolution+2);
  assert.equal((await cli(resolveArgs(resolutionId,3,resolveId),false)).error_code,'resolution_receipt_conflict');
  // A concurrent answer invalidates the exact source, not the newly saved fact.
  mode='clarify';const raceDraft=await cli(['requests','open','--text','Resolve an answer race']);
  await cli(['requests','answer',raceDraft.result.id,'--revision','1','--text','First answer']);
  mode='gate';const race=cli(resolveArgs(raceDraft.result.id,2,randomUUID()),false);
  await until(()=>gates.size===1,'clarification interpretation in flight');
  const newerAnswer=await cli(['requests','answer',raceDraft.result.id,'--revision','2','--text','Changed answer while inference is running']);
  for(const res of gates)res.end(JSON.stringify({message:{content:JSON.stringify(proposal())},done:true,done_reason:'stop'}));
  const raced=await race;assert.equal(raced.error_code,'resolution_rejected');assert.equal(raced.automatically_retried,false);
  assert.deepEqual((await cli(['requests','draft',raceDraft.result.id])).result.draft,newerAnswer.result.draft);
  await until(()=>gates.size===0,'raced inference released');
  // Read-only saved receipts bypass interpreter capacity and missing roles.
  mode='gate';const occupying=review('Occupy interpretation capacity',false);
  await until(()=>gates.size===1,'occupied interpreter');
  assert.equal((await cli(resolveArgs(resolutionId,2,resolveId))).reused,true);
  const restartResolution=await cli(['restart','--wait-seconds','15']);assert.equal(restartResolution.completed,true);
  assert.equal((await occupying).error_code,'runtime_changed');await until(()=>gates.size===0,'occupied inference cancelled');
  mode='valid';await until(ready,'reconnected after interpretation restart');
  // Restart during resolver inference cannot append a stale proposal.
  mode='gate';const interruptedResolution=cli(resolveArgs(raceDraft.result.id,3,randomUUID()),false);
  await until(()=>gates.size===1,'resolver awaiting backend');
  await cli(['restart','--wait-seconds','15']);
  const interrupted=await interruptedResolution;assert.equal(interrupted.error_code,'runtime_changed');assert.equal(interrupted.saving_attempted,false);
  await until(()=>gates.size===0,'resolver backend wait cancelled');mode='valid';await until(ready,'resolver restart recovery');
  assert.deepEqual((await cli(['requests','draft',raceDraft.result.id])).result.draft,newerAnswer.result.draft);
  // A lost local HTTP reply can hide a saved resolution. Same operation ID
  // inspects that retained record without asking the model or saving again.
  const lostResolveId=randomUUID(),resolveMarker=await readFile(marker,'utf8');beforeResolution=calls.length;
  try {
    const altered=JSON.parse(resolveMarker);altered.address=`127.0.0.1:${proxyPort}`;await writeFile(marker,JSON.stringify(altered));proxyMode='drop';
    const lostResolution=await cli(resolveArgs(raceDraft.result.id,3,lostResolveId),false);
    assert.equal(lostResolution.error_code,'resolution_unconfirmed');assert.equal(lostResolution.outcome_unknown,true);
    assert.equal(lostResolution.command.operation_id,lostResolveId);
  }finally{await writeFile(marker,resolveMarker);}
  assert.equal(calls.length,beforeResolution+1);
  await cli(['models','hints','--backend','ollama','--model','fixture','--role','unselected_role']);await cli(['restart','--wait-seconds','15']);
  await until(ready,'missing interpreter role applied');
  const inspectedLost=await cli(resolveArgs(raceDraft.result.id,3,lostResolveId));
  assert.equal(inspectedLost.reused,true);assert.equal(inspectedLost.result.revision,4);
  assert.equal(calls.length,beforeResolution+1,'lost receipt inspection needs no interpreter or inference');
  await cli(['models','hints','--backend','ollama','--model','fixture','--role','request_interpreter']);await cli(['restart','--wait-seconds','15']);
  await until(ready,'restore interpreter after receipt inspection');
  for(const id of [resolutionId,raceDraft.result.id]){await cli(['jobs','stop',id]);await cli(['jobs','remove',id]);}
  assert.equal((await cli(['jobs','list'])).data.total,0);
  // The input node has no HTTP feature. Online hints may use a fresh peer's
  // features only after the owner grants directional discovery; never execution.
  const peerConfigPath=join(scratch,'peer.json'),peerPort=await port();
  const peerConfig=structuredClone(config);
  Object.assign(peerConfig,{node_id:'review-fixture-peer',name:'Peer fixture',metadata:{area:['Home','Living room']},
    dashboard:{enabled:true,ui_enabled:false,host:'127.0.0.1',port:peerPort},
    models:{provider:'llamacpp',llamacpp:{slots:[]}}});
  peerConfig.permissions.http={enabled:true,allowed_hosts:['example.invalid'],methods:['GET'],allow_private_networks:false};
  await writeFile(peerConfigPath,JSON.stringify(peerConfig));
  const peerInvitation=await coordinator(['credentials','provision','--node',peerConfig.node_id,'--address',`127.0.0.1:${gatewayPort}`]);
  const peerPrivateCode=join(scratch,'PRIVATE-peer-pairing.json');await writeFile(peerPrivateCode,JSON.stringify(peerInvitation));
  const beforePeerRuntime=(await coordinator(['diagnostics'])).runtime_id;await coordinator(['restart']);
  await until(async()=>(await coordinator(['diagnostics'])).runtime_id!==beforePeerRuntime&&await ready(),'apply independent peer credential');
  expectedNodes=2;const peerProcess=launch('eefn',['--config',peerConfigPath,'--no-ui']);
  await until(async()=>(await json(`http://127.0.0.1:${peerPort}/api/status`)).node_id===peerConfig.node_id,'peer owner API');
  await cli(['connection','pair','--code-file',peerPrivateCode],true,peerConfigPath);
  await until(ready,'second independent logical node registered');
  assert.equal((await json(`http://127.0.0.1:${peerPort}/api/status`)).connection.transport,'pinned_tls13');
  const self=(await cli(['network','peers'])).nodes.find(n=>n.node_id===config.node_id);
  assert(self && !self.capabilities.includes('http.request'),'origin must not have the peer feature');
  mode='remote';const beforeNetwork=calls.length;
  assert.equal((await review('Fetch that information',false)).error_code,'interpretation_invalid');
  assert(!calls.at(-1).messages[0].content.includes('http.request'),'undisclosed peer must not enter the prompt');
  await coordinator(['discovery','grant','--requester',config.node_id,'--target',peerConfig.node_id]);
  let contextRuntime=(await coordinator(['diagnostics'])).runtime_id;
  await coordinator(['restart']);
  await until(async()=>(await coordinator(['diagnostics'])).runtime_id!==contextRuntime&&await ready(),'apply directional disclosure');
  const cross=await cli(['requests','open','--text','  Fetch information from the other node\n']);
  assert.equal(cross.interpretation_context,'coordinator_disclosed_network');
  assert.equal(cross.result.request_context.origin_node,config.node_id);
  assert.deepEqual(cross.result.request_context.origin_area,['Home','Bedroom']);
  assert.deepEqual(cross.result.draft.proposals[0].proposal.suggested_capabilities,['http.request']);
  assert(cross.result.draft.proposals[0].allowed_capabilities.includes('http.request'));
  assert(calls.at(-1).messages[0].content.includes('http.request'),'network vocabulary must reach the actual interpreter prompt');
  assert.equal(cross.dispatched,false);assert.equal(cross.execution_authorized,false);
  assert.deepEqual((await cli(['jobs','get',cross.result.id])).data.tasks,[]);
  const localOnly=await cli(['requests','preview','--text','Fetch information'],false);
  assert.equal(localOnly.error_code,'interpretation_invalid');
  assert.equal(localOnly.interpretation_context,'local_node');
  assert(!calls.at(-1).messages[0].content.includes('http.request'),'offline preview must remain local');
  const remoteFile=join(scratch,'remote-proposal.json');
  await writeFile(remoteFile,JSON.stringify({schema_version:1,original_text:'Remote capability hint',proposal:{...proposal(),suggested_capabilities:['http.request']}}));
  const suppliedCross=await cli(['requests','review-proposal','--file',remoteFile]);
  assert.equal(suppliedCross.coordinator_acknowledged,true);
  assert.equal(calls.length,beforeNetwork+3,'supplied proposals do not invoke a model');
  const peerPlanDraft=await cli(['requests','open-proposal','--file',remoteFile]);
  const peerPlanFile=join(scratch,'peer-plan.json');
  await writeFile(peerPlanFile,JSON.stringify({schema_version:1,template:'http_request',params:{url:'https://example.invalid/info',method:'GET'},constraints:{}}));
  const beforePeerPlanning=calls.length;
  const peerPlanned=await cli(['requests','compile',peerPlanDraft.result.id,'--revision','1','--file',peerPlanFile]);
  assert.equal(peerPlanned.coordinator_acknowledged,true);assert.equal(peerPlanned.result.status,'awaiting_approval');
  assert.equal(peerPlanned.result.bindings[0].node_id,peerConfig.node_id,'origin without HTTP must compile against its disclosed live peer');
  assert.equal(peerPlanned.result.tasks[0].capability,'http.request');assert.equal(peerPlanned.result.tasks[0].status,'pending');
  assert.equal(peerPlanned.execution_authorized,false);assert.equal(peerPlanned.dispatched,false);
  assert.equal(calls.length,beforePeerPlanning,'deterministic compilation must not invoke the interpreter/model');
  const deniedPeerInspection=await cli(['requests','plan',peerPlanDraft.result.id],false,peerConfigPath);
  assert.equal(deniedPeerInspection.success,false,'selected executor cannot inspect another origin private plan');
  const withdrawPlanDraft=await cli(['requests','open-proposal','--file',remoteFile]);
  const revisionFile=join(scratch,'remote-revision.json');
  await writeFile(revisionFile,JSON.stringify({schema_version:1,original_text:cross.result.draft.original_text,
    proposal:{...proposal(),goal:'Revised remote hint',suggested_capabilities:['http.request']}}));
  const revisedCross=await cli(['requests','revise-proposal',cross.result.id,'--revision','1','--file',revisionFile]);
  assert.equal(revisedCross.result.revision,2);
  assert(revisedCross.result.draft.proposals[1].allowed_capabilities.includes('http.request'));
  // Disconnect cannot leave cached availability behind, even while disclosure
  // remains applied. Reconnection restores vocabulary, not execution authority.
  peerProcess.kill();await peerProcess.result;expectedNodes=1;await until(ready,'peer disconnected');
  const historicalPeerPlan=await cli(['requests','plan',peerPlanDraft.result.id]);
  assert.equal(historicalPeerPlan.result.plan_sha256,peerPlanned.result.plan_sha256,'history remains inspectable without fresh executor lookup');
  const withdrawnCompilation=await cli(['requests','compile',withdrawPlanDraft.result.id,'--revision','1','--file',peerPlanFile],false);
  assert.equal(withdrawnCompilation.success,false);assert.equal(withdrawnCompilation.outcome_unknown,true);
  const withdrawnHistory=(await cli(['requests','plan',withdrawPlanDraft.result.id])).result;
  assert.equal(withdrawnHistory.status,'review');assert.equal(withdrawnHistory.revision,1);assert.deepEqual(withdrawnHistory.tasks,[]);
  assert.equal((await cli(['requests','review-proposal','--file',remoteFile],false)).error_code,'review_rejected');
  expectedNodes=2;const reconnectedPeer=launch('eefn',['--config',peerConfigPath,'--no-ui']);
  await until(ready,'peer reconnected');
  assert.equal((await cli(['requests','review-proposal','--file',remoteFile])).coordinator_acknowledged,true);
  // A prompt formed before policy revocation cannot grandfather its hints into
  // EEF after inference. Finish the fake backend exactly once, then reject review.
  mode='gate';const outdated=review('Fetch using the peer after a delayed interpretation',false);
  await until(()=>gates.size===1,'network interpretation in flight');
  assert(calls.at(-1).messages[0].content.includes('http.request'));
  await coordinator(['discovery','revoke','--requester',config.node_id,'--target',peerConfig.node_id]);
  contextRuntime=(await coordinator(['diagnostics'])).runtime_id;await coordinator(['restart']);
  await until(async()=>(await coordinator(['diagnostics'])).runtime_id!==contextRuntime&&await ready(),'apply disclosure revocation');
  for(const res of gates) res.end(JSON.stringify({message:{content:JSON.stringify({...proposal(),suggested_capabilities:['http.request']})},done:true,done_reason:'stop'}));
  assert.equal((await outdated).error_code,'review_rejected');
  await until(()=>gates.size===0,'delayed interpreter released');mode='remote';
  const revoked=await cli(['requests','review-proposal','--file',remoteFile],false);
  assert.equal(revoked.error_code,'review_rejected');
  assert.equal((await cli(['requests','revise-proposal',cross.result.id,'--revision','2','--file',revisionFile],false)).error_code,'draft_rejected');
  assert.equal(calls.length,beforeNetwork+4,'EEF must independently reject the no-longer-disclosed hint without inference replay');
  const history=(await cli(['requests','draft',cross.result.id])).result;
  assert.deepEqual(history.draft,revisedCross.result.draft,'historical inspection cannot reinterpret a saved snapshot');
  await cli(['jobs','stop',cross.result.id]);await cli(['jobs','remove',cross.result.id]);
  // Semantic preparation also uses EEF's disclosed cross-node compiler, never
  // direct peer execution or hidden-provider fallback.
  await coordinator(['discovery','grant','--requester',config.node_id,'--target',peerConfig.node_id]);
  contextRuntime=(await coordinator(['diagnostics'])).runtime_id;await coordinator(['restart']);
  await until(async()=>(await coordinator(['diagnostics'])).runtime_id!==contextRuntime&&await ready(),'restore method peer disclosure');
  mode='remote';const peerMethodDraft=await cli(['requests','open','--text','Fetch information using the peer']);
  const peerMethodOperation=randomUUID(),peerMethodArgs=['requests','prepare',peerMethodDraft.result.id,'--revision','1','--operation-id',peerMethodOperation];
  mode='method-remote';const peerMethod=await cli(peerMethodArgs);
  assert.equal(peerMethod.result.bindings[0].node_id,peerConfig.node_id);assert.equal(peerMethod.result.status,'awaiting_approval');
  assert.equal(peerMethod.result.tasks[0].status,'pending');assert.equal(peerMethod.execution_authorized,false);assert.equal(peerMethod.dispatched,false);
  assert.equal((await cli(peerMethodArgs,false,peerConfigPath)).error_code,'source_unavailable');
  mode='remote';const revokedMethodDraft=await cli(['requests','open','--text','Peer method revoked during inference']);mode='gate';
  const revokedMethodArgs=['requests','prepare',revokedMethodDraft.result.id,'--revision','1','--operation-id',randomUUID()];
  const revokedMethod=cli(revokedMethodArgs,false);await until(()=>gates.size===1,'peer method inference gated');
  await coordinator(['discovery','revoke','--requester',config.node_id,'--target',peerConfig.node_id]);
  contextRuntime=(await coordinator(['diagnostics'])).runtime_id;await coordinator(['restart']);
  await until(async()=>(await coordinator(['diagnostics'])).runtime_id!==contextRuntime&&await ready(),'method disclosure revoked');
  for(const res of gates)res.end(JSON.stringify({message:{content:JSON.stringify({schema_version:1,
    method:{schema_version:1,template:'http_request',params:{url:'https://example.invalid/info',method:'GET'},constraints:{}},clarification:null})},done:true,done_reason:'stop'}));
  assert.equal((await revokedMethod).success,false);
  const deniedMethodHistory=(await cli(['requests','plan',revokedMethodDraft.result.id])).result;
  assert.equal(deniedMethodHistory.revision,1);assert.equal(deniedMethodHistory.input,null);assert.deepEqual(deniedMethodHistory.tasks,[]);
  await until(()=>gates.size===0,'peer method inference released');
  reconnectedPeer.kill();await reconnectedPeer.result;expectedNodes=1;await until(ready,'peer disconnected again');
  const peerMethodCalls=calls.length;assert.equal((await cli(peerMethodArgs)).inspection_only,true);
  assert.equal(calls.length,peerMethodCalls,'historical method receipt requires neither fresh peer lookup nor inference');
  // Complete preparation is a backend command, not browser logic or execution.
  const prepareArgs=(id,revision,operation)=>['requests','prepare',id,'--revision',String(revision),'--operation-id',operation];
  const methodResult={schema_version:1,method:{schema_version:1,template:'system_info',params:{},constraints:{}},clarification:null};
  mode='valid';const methodDraft=await cli(['requests','open','--text','  Inspect this node\n']);
  const methodId=methodDraft.result.id,methodOperation=randomUUID();mode='method';let beforeMethods=calls.length;
  const prepared=await cli(prepareArgs(methodId,1,methodOperation));
  assert.equal(calls.length,beforeMethods+1);assert.equal(prepared.report_type,'request_preparation');
  assert.equal(prepared.checkpoint_saved,true);assert.equal(prepared.inspection_only,false);
  assert.equal(prepared.saving_attempted,true);assert.equal(prepared.model_inference_requested,true);
  assert.equal(prepared.execution_authorized,false);assert.equal(prepared.dispatched,false);
  assert.equal(prepared.result.status,'awaiting_approval');assert.equal(prepared.result.revision,2);
  assert.equal(prepared.result.preparation.evidence.source_revision,1);
  assert.equal(prepared.result.preparation.evidence.job_id,methodId);
  assert.equal(prepared.result.preparation.evidence.backend,'ollama');
  assert.deepEqual(prepared.result.request_context,methodDraft.result.request_context);
  assert.deepEqual(prepared.result.draft,methodDraft.result.draft);
  assert.equal(prepared.result.bindings[0].node_id,config.node_id);
  assert.equal(prepared.result.tasks[0].status,'pending');assert.equal(calls.at(-1).options.num_predict,1024);
  assert.equal(calls.at(-1).messages[1].content,methodDraft.result.draft.original_text);
  const preparedOwner=(await coordinator(['requests','inspect',methodId])).plan;
  assert.equal(preparedOwner.approval ?? null,null);assert.equal(preparedOwner.execution ?? null,null);
  assert.equal((await cli(prepareArgs(methodId,1,methodOperation))).inspection_only,true);
  assert.equal(calls.length,beforeMethods+1);
  assert.equal((await cli(prepareArgs(methodId,1,randomUUID()),false)).error_code,'source_revision_changed');
  assert.equal((await cli(prepareArgs(methodId,2,methodOperation),false)).error_code,'preparation_receipt_conflict');
  const prepareUrl=node+'/api/commands/requests/prepare';
  const body={schema_version:1,expected_node_id:config.node_id,command:{id:methodId,expected_revision:1,operation_id:methodOperation}};
  assert.equal((await fetch(prepareUrl,{method:'POST',headers:{'Content-Type':'application/json',Origin:'https://untrusted.example'},body:JSON.stringify(body)})).status,403);
  assert.equal((await fetch(prepareUrl,{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify({...body,command:{...body.command,approved:true}})})).status,422);
  assert.equal((await json(prepareUrl,{...body,expected_node_id:'wrong-node'})).error_code,'invalid_preparation_request');
  // A method clarification is durably answerable, not a stateless suggestion.
  mode='valid';const questionDraft=await cli(['requests','open','--text','Inspect status with the node I mean']);
  const questionId=questionDraft.result.id,questionOperation=randomUUID();mode='method-question';
  const asked=await cli(prepareArgs(questionId,1,questionOperation));
  assert.equal(asked.result.status,'review');assert.equal(asked.result.revision,2);assert.equal(asked.result.input,null);
  assert.equal(asked.result.draft.proposals[1].proposal.clarification,'Which node should be inspected?');
  assert.equal(asked.result.draft.proposals[1].preparation.evidence.source_revision,1);
  const answeredMethod=await cli(['requests','answer',questionId,'--revision','2','--text','This node']);
  assert.equal(answeredMethod.result.revision,3);assert.equal(answeredMethod.result.draft.answers[0].question,'Which node should be inspected?');
  mode='valid';await cli(['requests','resolve',questionId,'--revision','3','--operation-id',randomUUID()]);mode='method';
  const answeredPlan=await cli(prepareArgs(questionId,4,randomUUID()));
  assert.equal(answeredPlan.result.revision,5);assert.equal(answeredPlan.result.preparation.evidence.source_answer_count,1);
  const fullHistory=JSON.parse(calls.at(-1).messages[2].content).retained_review;
  assert.deepEqual(fullHistory,answeredPlan.result.draft);assert.equal(fullHistory.answers[0].text,'This node');
  const oldQuestion=await cli(prepareArgs(questionId,1,questionOperation));
  assert.equal(oldQuestion.inspection_only,true);assert.equal(oldQuestion.result.status,'awaiting_approval');
  // Invalid output and stale source cannot mutate a review or dispatch anything.
  mode='valid';const invalidMethod=await cli(['requests','open','--text','Invalid candidate check']);mode='method-invalid';
  const untouched=(await cli(['requests','plan',invalidMethod.result.id])).result;
  const rejectedMethod=await cli(prepareArgs(invalidMethod.result.id,1,randomUUID()),false);
  assert.equal(rejectedMethod.error_code,'method_proposal_invalid');assert.equal(rejectedMethod.saving_attempted,false);
  assert.deepEqual((await cli(['requests','plan',invalidMethod.result.id])).result.draft,untouched.draft);
  mode='gate';const racingOperation=randomUUID(),racing=cli(prepareArgs(invalidMethod.result.id,1,racingOperation),false);
  await until(()=>gates.size===1,'method inference gated');beforeMethods=calls.length;
  assert.equal((await cli(prepareArgs(invalidMethod.result.id,1,randomUUID()),false)).error_code,'interpreter_busy');
  assert.equal(calls.length,beforeMethods);
  const methodRevision=join(scratch,'method-revision.json');await writeFile(methodRevision,JSON.stringify({schema_version:1,
    original_text:untouched.draft.original_text,proposal:{...proposal(),goal:'Explicit concurrent revision'}}));
  await cli(['requests','revise-proposal',invalidMethod.result.id,'--revision','1','--file',methodRevision]);
  for(const res of gates)res.end(JSON.stringify({message:{content:JSON.stringify(methodResult)},done:true,done_reason:'stop'}));
  const racedMethod=await racing;assert.equal(racedMethod.success,false);assert.equal(racedMethod.saving_attempted,true);assert.equal(racedMethod.outcome_unknown,true);
  const afterRace=(await cli(['requests','plan',invalidMethod.result.id])).result;
  assert.equal(afterRace.revision,2);assert.equal(afterRace.input,null);assert.deepEqual(afterRace.tasks,[]);
  await until(()=>gates.size===0,'method race released');mode='gate';
  const restartMethod=cli(prepareArgs(invalidMethod.result.id,2,randomUUID()),false);
  await until(()=>gates.size===1,'method restart gated');await cli(['restart','--wait-seconds','15']);
  const invalidated=await restartMethod;assert.equal(invalidated.error_code,'runtime_changed');assert.equal(invalidated.saving_attempted,false);
  for(const res of gates)res.destroy();await until(ready,'method runtime replaced');
  assert.equal((await cli(['requests','plan',invalidMethod.result.id])).result.revision,2);
  // A lost local reply is recovered by receipt inspection, without a selected role.
  mode='valid';const lostMethodDraft=await cli(['requests','open','--text','Lost method reply']);mode='method';
  const lostMethodOperation=randomUUID(),methodMarker=await readFile(marker,'utf8');beforeMethods=calls.length;
  try {
    const altered=JSON.parse(methodMarker);altered.address=`127.0.0.1:${proxyPort}`;await writeFile(marker,JSON.stringify(altered));proxyMode='drop';
    const lost=await cli(prepareArgs(lostMethodDraft.result.id,1,lostMethodOperation),false);
    assert.equal(lost.error_code,'preparation_unconfirmed');assert.equal(lost.outcome_unknown,true);
  }finally{await writeFile(marker,methodMarker);}
  assert.equal(calls.length,beforeMethods+1);
  const beforePrepareReopen=(await coordinator(['diagnostics'])).runtime_id;await coordinator(['restart']);
  await until(async()=>(await coordinator(['diagnostics'])).runtime_id!==beforePrepareReopen&&await ready(),'prepared plans reopened');
  await cli(['models','hints','--backend','ollama','--model','fixture','--role','unselected_role']);await cli(['restart','--wait-seconds','15']);
  await until(ready,'missing method interpreter role applied');
  const lostInspection=await cli(prepareArgs(lostMethodDraft.result.id,1,lostMethodOperation));
  assert.equal(lostInspection.inspection_only,true);assert.equal(lostInspection.model_inference_requested,false);assert.equal(lostInspection.saving_attempted,false);
  assert.equal(calls.length,beforeMethods+1);assert.equal(lostInspection.result.status,'awaiting_approval');
  assert.equal((await cli(prepareArgs(methodId,1,methodOperation))).result.plan_sha256,prepared.result.plan_sha256);
  assert.equal((await cli(prepareArgs(questionId,1,questionOperation))).inspection_only,true);
  // Explicit caller inputs require neither interpreter selection nor inference.
  // They retain source data and compile through the same EEF approval/grant path.
  const beforeConcrete=calls.length,concreteOperation=randomUUID();
  const concreteArgs=['requests','submit','--text','  Show system status\n','--title','Explicit status',
    '--template','system_info','--operation-id',concreteOperation];
  const concrete=await cli(concreteArgs);
  assert.equal(concrete.report_type,'node_concrete_request');assert.equal(concrete.result.plan.status,'awaiting_approval');
  assert.equal(concrete.result.plan.draft.original_text,'  Show system status\n');
  assert.deepEqual(concrete.result.plan.draft.concrete_input,{schema_version:1,template:'system_info',params:{},constraints:{}});
  assert.equal(concrete.result.plan.request_context.request_id,concreteOperation);
  assert.notEqual(concrete.result.compilation_operation_id,concreteOperation);
  assert.equal(concrete.dispatched,false);assert.equal(concrete.execution_authorized,false);assert.equal(calls.length,beforeConcrete);
  assert.equal((await cli(concreteArgs)).result.creation_reused,true);assert.equal(calls.length,beforeConcrete);
  assert.equal((await cli(prepareArgs(concrete.result.plan.id,2,randomUUID()),false)).error_code,'source_revision_changed');
  const concreteUrl=node+'/api/commands/requests/submit';
  const concreteBody={schema_version:1,expected_node_id:config.node_id,command:{operation_id:concreteOperation,request:{schema_version:1,
    original_text:'  Show system status\n',title:'Explicit status',input:{schema_version:1,template:'system_info',params:{},constraints:{}}}}};
  assert.equal((await fetch(concreteUrl,{method:'POST',headers:{'Content-Type':'application/json',Origin:'https://untrusted.example'},body:JSON.stringify(concreteBody)})).status,403);
  assert.equal((await fetch(concreteUrl,{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify({...concreteBody,command:{...concreteBody.command,approved:true}})})).status,422);
  assert.equal((await fetch(concreteUrl,{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify({...concreteBody,expected_node_id:'wrong'})})).status,400);
  const textOperation=randomUUID(),textPrompt='  Exact submitted text\n';
  const requestedTextArgs=['jobs','request-text','--prompt',textPrompt,'--role','unselected_role','--backend','ollama',
    '--model','fixture','--target-node',config.node_id,'--operation-id',textOperation];
  const requestedText=await cli(requestedTextArgs);const textId=requestedText.data.id;
  assert.equal(requestedText.submission_mode,'reviewed_concrete_request');assert.equal(requestedText.creation_correlated,true);
  assert.equal(requestedText.data.status,'awaiting_approval');assert.equal(requestedText.data.tasks[0].status,'pending');
  assert.equal(requestedText.data.review_only,true);assert.equal(requestedText.data.can_resume,false);
  assert.equal(requestedText.data.request_context.request_id,textOperation);assert.equal(requestedText.dispatched,false);
  assert.equal(calls.length,beforeConcrete,'request-text must not call a backend before approval/execution');
  const textPlan=(await cli(['requests','plan',textId])).result;
  assert.equal(textPlan.draft.original_text,textPrompt);assert.equal(textPlan.draft.concrete_input.params.prompt,textPrompt);
  assert.equal(textPlan.input.params.model_id,'fixture');assert.equal(textPlan.input.constraints.model_role,'unselected_role');
  assert.equal(textPlan.bindings[0].node_id,config.node_id);
  const textBody={schema_version:1,expected_node_id:config.node_id,operation_id:textOperation,
    command:{operation:'request_text',prompt:textPrompt,role:'unselected_role',backend:'ollama',model:'fixture',target_node:config.node_id}};
  const duplicates=await Promise.all(Array.from({length:6},()=>json(node+'/api/commands/jobs',textBody)));
  assert(duplicates.every(r=>r.success&&r.data.id===textId&&r.data.admission.reused));assert.equal(calls.length,beforeConcrete);
  assert.equal((await fetch(node+'/api/commands/jobs',{method:'POST',headers:{'Content-Type':'application/json'},
    body:JSON.stringify({...textBody,command:{...textBody.command,operation_id:randomUUID()}})})).status,400);
  const conflict=await json(node+'/api/commands/jobs',{...textBody,command:{...textBody.command,prompt:'Changed same receipt'}});
  assert.equal(conflict.success,false);assert.equal(conflict.outcome_unknown,true);assert.equal(calls.length,beforeConcrete);
  // A compiler failure can leave the source checkpoint. Exact retries do not
  // replace that input; jobs find + requests plan exposes partial admission.
  const missingOperation=randomUUID();
  const missingArgs=['jobs','request-text','--prompt','Missing selected model','--model','not-selected','--operation-id',missingOperation];
  const missing=await cli(missingArgs,false);assert.equal(missing.outcome_unknown,true);
  const partial=(await cli(['jobs','find','--operation-id',missingOperation])).data;
  assert.equal(partial.total,1);const partialId=partial.jobs[0].id;
  const partialPlan=(await cli(['requests','plan',partialId])).result;
  assert.equal(partialPlan.status,'review');assert.equal(partialPlan.revision,1);assert.equal(partialPlan.input,null);
  assert.equal(partialPlan.draft.concrete_input.params.model_id,'not-selected');
  await cli(missingArgs,false);assert.equal((await cli(['jobs','find','--operation-id',missingOperation])).data.total,1);
  const changedMissingArgs=[...missingArgs];changedMissingArgs[5]='fixture';await cli(changedMissingArgs,false);
  assert.equal((await cli(['requests','plan',partialId])).result.draft.concrete_input.params.model_id,'not-selected');
  const changedInput=join(scratch,'changed-explicit-input.json');
  await writeFile(changedInput,JSON.stringify({...partialPlan.draft.concrete_input,params:{prompt:'Missing selected model',model_id:'fixture'}}));
  await cli(['requests','compile',partialId,'--revision','1','--file',changedInput],false);
  assert.equal((await cli(['requests','plan',partialId])).result.revision,1);await cli(['jobs','stop',partialId]);
  // Lost reply + restart returns the original retained request, not a new job.
  const lostConcreteOperation=randomUUID(),lostConcreteMarker=await readFile(marker,'utf8');
  const lostConcreteArgs=['requests','submit','--text','Lost explicit reply','--template','system_info','--operation-id',lostConcreteOperation];
  try {
    const altered=JSON.parse(lostConcreteMarker);altered.address=`127.0.0.1:${proxyPort}`;await writeFile(marker,JSON.stringify(altered));proxyMode='drop';
    const lost=await cli(lostConcreteArgs,false);assert.equal(lost.error_code,'concrete_unconfirmed');assert.equal(lost.outcome_unknown,true);
  }finally{await writeFile(marker,lostConcreteMarker);}
  const foundConcrete=(await cli(['jobs','find','--operation-id',lostConcreteOperation])).data;assert.equal(foundConcrete.total,1);
  const beforeConcreteRestart=(await coordinator(['diagnostics'])).runtime_id;await coordinator(['restart']);
  await until(async()=>(await coordinator(['diagnostics'])).runtime_id!==beforeConcreteRestart&&await ready(),'explicit requests reopened');
  assert.equal((await cli(lostConcreteArgs)).result.plan.id,foundConcrete.jobs[0].id);
  assert.equal((await cli(requestedTextArgs)).data.id,textId);assert.equal(calls.length,beforeConcrete);
  // The same explicit text job can execute once via separate owner approval and
  // an origin request, with a fixed model and durable nonce/lease receipts.
  const textApproval=await coordinator(['requests','approve',textId,'--revision','2','--digest',textPlan.plan_sha256,
    '--valid-for-ms','60000','--max-execution-ms','5000','--max-result-bytes','16384','--operation-id',randomUUID()]);
  assert.equal(textApproval.plan.status,'approved');assert.equal(calls.length,beforeConcrete);mode='text-execution';
  const textExecution=await cli(['requests','execute',textId,'--revision','3','--approval-digest',textApproval.plan.approval.approval_sha256,'--operation-id',randomUUID()]);
  assert.equal(textExecution.success,true);
  await until(async()=>(await cli(['jobs','get',textId])).data.status==='completed','granted explicit text complete');
  assert.equal(calls.length,beforeConcrete+1);assert.equal(calls.at(-1).messages[0].content,textPrompt);
  assert.equal((await cli(['jobs','output',textId])).data.outputs[0].content,'fixture concrete grant result');
  assert.equal((await cli(requestedTextArgs)).data.status,'completed');assert.equal(calls.length,beforeConcrete+1);
  // Final node output is durably separate from EEF's journal. Read it explicitly
  // through the originating node or owner; routine receipts/jobs remain redacted.
  const textCompleted=(await cli(['jobs','get',textId])).data,textGrant=textCompleted.grant_attempts[0].grant_id;
  const resultArgs=['requests','result',textId,'--grant-id',textGrant];
  const savedResult=await cli(resultArgs);
  assert.equal(savedResult.result.result.availability,'retained_final_report');
  assert.equal(savedResult.result.result.report.data.content,'fixture concrete grant result');
  assert.equal(savedResult.reconciled,false);assert.equal(savedResult.result.reconciled,false);
  assert.equal((await coordinator(resultArgs)).result.report.data.content,'fixture concrete grant result');
  const localResult=await cli(['grants','result','--id',textGrant]);
  assert.deepEqual(localResult.result,savedResult.result.result);
  assert.equal((await cli(['grants','receipt','--id',textGrant])).receipt.report,undefined);
  assert(!JSON.stringify(await cli(['grants','receipt','--id',textGrant])).includes('fixture concrete grant result'));
  assert.deepEqual((await cli(['jobs','get',textId])).data,textCompleted,'result reads never change coordinator tracking');
  expectedNodes=2;launch('eefn',['--config',peerConfigPath,'--no-ui']);await until(ready,'independent origin for result authorization denial');
  assert.equal((await cli(resultArgs,false,peerConfigPath)).error_code,'result_unavailable');
  const resultBody={schema_version:1,expected_node_id:config.node_id,request_id:randomUUID(),command:{id:textId,grant_id:textGrant}};
  const resultUrl=node+'/api/commands/requests/result';
  assert.equal((await fetch(resultUrl,{method:'POST',headers:{'Content-Type':'application/json',Origin:'https://untrusted.example'},body:JSON.stringify(resultBody)})).status,403);
  assert.equal((await fetch(resultUrl,{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify({...resultBody,retry:true})})).status,422);
  assert.equal((await fetch(resultUrl,{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify({...resultBody,expected_node_id:'other'})})).status,400);
  const localResultBody={schema_version:1,expected_node_id:config.node_id,grant_id:textGrant};
  assert.equal((await fetch(node+'/api/commands/grants/result',{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify({...localResultBody,execute:true})})).status,422);
  assert.deepEqual((await json(node+'/api/commands/grants/result',localResultBody)).result,localResult.result);
  const resultRuntime=(await coordinator(['diagnostics'])).runtime_id;await coordinator(['restart']);
  await until(async()=>(await coordinator(['diagnostics'])).runtime_id!==resultRuntime&&await ready(),'result evidence after coordinator restart');
  assert.deepEqual((await cli(resultArgs)).result.result,savedResult.result.result);
  await cli(['restart','--wait-seconds','15']);await until(ready,'result evidence after executor session replacement');
  assert.deepEqual((await cli(resultArgs)).result.result,savedResult.result.result);
  assert.deepEqual((await cli(['jobs','get',textId])).data,textCompleted);
  assert.equal(calls.length,beforeConcrete+1,'result inspection must never repeat inference');
  // A real disconnect cancels the node's in-flight Rust future. It is not a
  // completed result or evidence the external backend stopped. Preserve unknown
  // consumption, deny result recovery, and close tracking only by explicit ack.
  {
    const count=calls.length,request=await cli(['jobs','request-text','--prompt','Disconnect while running','--role','unselected_role',
      '--backend','ollama','--model','fixture','--target-node',config.node_id,'--operation-id',randomUUID()]);
    const id=request.data.id,plan=(await cli(['requests','plan',id])).result;
    const approval=await coordinator(['requests','approve',id,'--revision','2','--digest',plan.plan_sha256,
      '--valid-for-ms','120000','--max-execution-ms','30000','--max-result-bytes','16384']);
    mode='recovery-gate';await cli(['requests','execute',id,'--revision','3','--approval-digest',approval.plan.approval.approval_sha256,'--operation-id',randomUUID()]);
    await until(async()=>recoveryGates.size===1&&(await cli(['jobs','get',id])).data.grant_attempts[0].state==='accepted','in-flight request before disconnect');
    const runtime=(await coordinator(['diagnostics'])).runtime_id;await coordinator(['restart']);
    await until(async()=>(await coordinator(['diagnostics'])).runtime_id!==runtime&&await ready(),'disconnect while request runs');
    const source=(await cli(['jobs','get',id])).data,grant=source.grant_attempts[0].grant_id;
    const result=(await cli(['requests','result',id,'--grant-id',grant])).result.result;
    assert.equal(result.receipt.state,'unknown');assert.equal(result.report.error_code,'grant_runtime_dropped_effects_unknown');
    assert.equal(result.report.success,false);assert.equal(result.report.data,null);
    const denied=await cli(['requests','recover-result',id,'--revision',String(source.revision),'--grant-id',grant,'--operation-id',randomUUID()],false);
    assert.equal(denied.error_code,'recovery_unconfirmed');assert.deepEqual((await cli(['jobs','get',id])).data,source);
    const closed=await cli(['requests','abandon',id,'--revision',String(source.revision),'--grant-id',grant,
      '--operation-id',randomUUID(),'--acknowledge-effects-may-continue']);
    assert.equal(closed.result.status,'abandoned');assert.equal(closed.result.remote_stop_confirmed,false);
    assert.equal(closed.result.remote_effects_unknown,true);assert.equal(calls.length,count+1);
    mode='text-execution';
  }
  // Deliberately fail only this isolated EEF job's completion checkpoint. The
  // node finishes/checkpoints once; EEF stays interrupted until explicit recovery.
  for (const recoveryActor of ['origin','owner']) {
    const count=calls.length,operation=randomUUID();
    const request=await cli(['jobs','request-text','--prompt','Recover '+recoveryActor,'--role','unselected_role',
      '--backend','ollama','--model','fixture','--target-node',config.node_id,'--operation-id',operation]);
    const id=request.data.id,plan=(await cli(['requests','plan',id])).result;
    const approved=await coordinator(['requests','approve',id,'--revision','2','--digest',plan.plan_sha256,
      '--valid-for-ms','120000','--max-execution-ms','30000','--max-result-bytes','16384']);
    const fault=new DatabaseSync(join(scratch,'eef.db'));
    try {fault.exec(`CREATE TRIGGER eef_fixture_block_completion BEFORE UPDATE OF plan_json ON durable_jobs
      WHEN json_extract(NEW.plan_json,'$.plan_id')='${id}' AND json_extract(NEW.plan_json,'$.execution.attempts[0].state')='completed'
      BEGIN SELECT RAISE(FAIL,'fixture coordinator completion failure'); END;`);}finally{fault.close();}
    mode='text-execution';
    await cli(['requests','execute',id,'--revision','3','--approval-digest',approved.plan.approval.approval_sha256,
      '--operation-id',randomUUID()]);
    await until(async()=>calls.length===count+1 && (await cli(['jobs','get',id])).data.status==='interrupted','lost coordinator completion checkpoint');
    const runtime=(await coordinator(['diagnostics'])).runtime_id;await coordinator(['restart']);
    await until(async()=>(await coordinator(['diagnostics'])).runtime_id!==runtime&&await ready(),'restart with retained completed node evidence');
    const source=(await cli(['jobs','get',id])).data,grant=source.grant_attempts[0].grant_id;
    assert.equal(source.status,'interrupted');assert.equal(source.grant_attempts[0].state,'unknown');
    const command={id,expected_revision:source.revision,grant_id:grant,operation_id:randomUUID()};
    const args=['requests','recover-result',id,'--revision',String(source.revision),'--grant-id',grant,'--operation-id',command.operation_id];
    assert.equal((await cli(['requests','result',id,'--grant-id',grant])).result.result.availability,'retained_final_report');
    const pending=await cli(args,false);assert.equal(pending.error_code,'recovery_unconfirmed');
    assert.deepEqual((await cli(['jobs','get',id])).data,source,'failed recovery checkpoint cannot mutate source');
    const body={schema_version:1,expected_node_id:config.node_id,command},url=node+'/api/commands/requests/recover-result';
    assert.equal((await fetch(url,{method:'POST',headers:{'Content-Type':'application/json',Origin:'https://untrusted.example'},body:JSON.stringify(body)})).status,403);
    assert.equal((await fetch(url,{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify({...body,command:{...command,result:{completed:true}}})})).status,422);
    assert.equal((await fetch(url,{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify({...body,expected_node_id:'other'})})).status,400);
    const removeFault=new DatabaseSync(join(scratch,'eef.db'));try{removeFault.exec('DROP TRIGGER eef_fixture_block_completion');}finally{removeFault.close();}
    const evidence=(await cli(['requests','result',id,'--grant-id',grant])).result.result;
    assert.equal(evidence.report.data.content,'fixture concrete grant result');
    assert.deepEqual((await cli(['jobs','get',id])).data,source,'cached final result does not automatically reconcile');
    assert.equal((await cli(args,false,peerConfigPath)).error_code,'recovery_unconfirmed');
    const staleArgs=[...args];staleArgs[4]=String(source.revision-1);
    assert.equal((await cli(staleArgs,false)).error_code,'recovery_unconfirmed');
    if(recoveryActor==='origin') {
      // Drop the HTTP reply only after the command committed. Reuse inspects
      // the exact saved receipt rather than fetching or applying evidence twice.
      const originalMarker=await readFile(marker,'utf8');
      try {
        const altered=JSON.parse(originalMarker);altered.address=`127.0.0.1:${proxyPort}`;
        await writeFile(marker,JSON.stringify(altered));proxyMode='drop';
        const lost=await cli(args,false);assert.equal(lost.outcome_unknown,true);assert.equal(lost.error_code,'recovery_unconfirmed');
      }finally{await writeFile(marker,originalMarker);}
    }
    const recovered=recoveryActor==='origin'?(await cli(args)).result:await coordinator(args);
    assert.equal(recovered.reconciled,true);assert.equal(recovered.execution_requested,false);
    assert.equal(recovered.automatically_retried,false);assert.equal(recovered.remote_stop_confirmed,false);
    assert.equal(recovered.result_sha256,evidence.report_sha256);assert.equal(recovered.applied_revision,source.revision+1);
    assert.equal(recovered.reused,recoveryActor==='origin');
    const saved=(await cli(['jobs','get',id])).data;
    assert.equal(saved.status,'completed');assert.equal(saved.revision,source.revision+1);
    assert.equal(saved.grant_attempts[0].acknowledged,source.grant_attempts[0].acknowledged);
    assert.equal(saved.grant_attempts[0].success,true);assert.equal(saved.can_resume,false);
    assert.equal(saved.recovery.reconciled,true);assert(!JSON.stringify(saved).includes('fixture concrete grant result'));
    assert.equal((await cli(['jobs','output',id])).data.outputs[0].content,'fixture concrete grant result');
    const privatePlan=(await coordinator(['requests','inspect',id])).plan;
    assert.equal(privatePlan.execution.attempts[0].report,null,'do not invent original completion');
    assert.deepEqual(privatePlan.execution.attempts[0].recovery.result,evidence);
    await cli(['jobs','resume',id],false);
    const beforeRestart=(await coordinator(['diagnostics'])).runtime_id;await coordinator(['restart']);
    await until(async()=>(await coordinator(['diagnostics'])).runtime_id!==beforeRestart&&await ready(),'retained reconciliation reopened');
    await cli(['restart','--wait-seconds','15']);await until(ready,'recovered executor session replaced');
    const same=recoveryActor==='origin'?(await cli(args)).result:await coordinator(args);
    assert.equal(same.reused,true);assert.deepEqual((await cli(['jobs','get',id])).data,saved);
    const changed=[...args];changed[8]=randomUUID();assert.equal((await cli(changed,false)).error_code,'recovery_unconfirmed');
    if(recoveryActor==='owner') {
      assert.equal((await cli(args,false)).error_code,'recovery_unconfirmed','origin cannot reuse coordinator-owner receipt');
      await cli(['connection','pause']);
      await until(async()=>(await json(eef+'/api/diagnostics')).registered_node_count===1,'executor disconnected for receipt reuse');
      assert.equal((await coordinator(args)).reused,true,'exact saved owner receipt needs no live executor');
      await cli(['connection','resume']);await until(ready,'restore executor after read-only reuse');
    }
    assert.equal(calls.length,count+1,'evidence recovery must not replay backend work');
  }
  await writeFile(join(scratch, 'results.json'), JSON.stringify({passed: true, api_only: true, node_to_coordinator_review: true,
    original_text_preserved: true, authenticated_origin: true, clarification_returned: true, rejected_authority_fields: true,
    stateless_no_jobs_created: true, durable_inferred_review: true, lost_draft_found_without_replay: true,
    preview_and_compilation_without_execution: true, restart_invalidates_inference: true, lost_reply_not_replayed: true,
    explicit_model_plan_without_inference:true,owner_role_binding:true,legacy_approval_denied_without_inference:true,
    pinned_model_grant_execution:true,withdrawn_role_denies_without_rebinding:true,durable_model_output:true,
    cross_node_interpreter_vocabulary:true,origin_without_peer_feature:true,directional_disclosure_and_revocation:true,
    offline_preview_local_only:true,historical_snapshot_preserved:true,
    disconnect_withdraws_vocabulary:true,inflight_context_revocation_rechecked:true,cross_node_proposal_revision:true,
    explicit_answer_reinterpretation:true,source_revision_race_rejected:true,source_digest_retained:true,
    invalid_resolution_no_save:true,resolution_restart_invalidation:true,resolution_receipt_without_inference:true,
    lost_resolution_reply_inspected:true,
    source_bound_method_preparation:true,durable_method_questions:true,method_answer_history_preserved:true,
    method_capacity_and_restart_guards:true,stale_method_source_rejected:true,method_receipt_without_inference:true,
    lost_method_reply_inspected:true,prepared_plan_restart_persistence:true,preparation_without_execution:true,
    cross_node_method_preparation:true,method_disclosure_revocation_rechecked:true,
    explicit_concrete_requests:true,explicit_source_inputs_immutable:true,model_free_explicit_compilation:true,
    explicit_text_grant_execution:true,explicit_receipt_restart_recovery:true,partial_explicit_admission_inspectable:true,
    legacy_workload_routes_fenced:true,normal_input_through_running_node:true,generate_text_requires_review:true,
    retained_executor_results:true,explicit_origin_owner_result_reads:true,result_restart_recovery:true,result_reads_no_replay_or_reconciliation:true,
    explicit_result_reconciliation:true,lost_coordinator_checkpoint_recovered_after_restart:true,recovery_no_replay_or_fake_ack:true,
    actual_local_disconnect_retains_unknown:true,unknown_result_does_not_reconcile:true,recovery_write_failure_preserves_source:true,
    recovery_origin_and_owner_scope:true,recovery_lost_reply_receipt_reuse:true,recovery_without_live_executor:true,
    real_inference: false, physical_two_pc: false}, null, 2));
  console.log('Request review checks passed: ' + scratch);
} finally {
  for (const response of gates) response.destroy();
  for (const response of recoveryGates) response.destroy();
  for (const child of children) if (child.exitCode === null) child.kill();
  await Promise.allSettled(children.map(c => c.result));
  for (const server of [fake, proxy]) {server.closeAllConnections(); await new Promise(r => server.close(r));}
}
