// API-only node -> EEF review, using fake inference and isolated local data.
// Stateless review, then durable review-only records; no executor/grant/download.
import assert from 'node:assert/strict';
import {mkdtemp, readFile, writeFile} from 'node:fs/promises';
import {resolve, join} from 'node:path';
import {spawn} from 'node:child_process';
import {createServer} from 'node:http';
import {randomUUID, randomBytes} from 'node:crypto';
const root = resolve(import.meta.dirname, '..');
const scratch = await mkdtemp(join(root, '.validation/request-review-'));
const bin = resolve(process.env.EEF_TEST_BINARY_DIR || join(root, 'target/debug'));
const configPath = join(scratch, 'node.json'), eefConfig = join(scratch, 'eef.yaml');
const marker = configPath + '.api.json', children = [], calls = [], gates = new Set();
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
async function cli(args, success = true) {
  const child = launch('eefn', ['--config', configPath, ...args, '--json']);
  const timer = setTimeout(() => child.kill(), 45000);
  try {const result = await child.result; assert.equal(result.code, success ? 0 : 1, result.out + result.err); return JSON.parse(result.out);}
  finally {clearTimeout(timer);}
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
let mode = 'valid', node = '', proxyMode = 'drop', forwarded = 0;
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
    const value = proposal();
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
  const yaml = (await readFile(join(root, 'config/default_identity.yaml'), 'utf8'))
    .replace('port: 51334', `port: ${eefPort}`).replace('port: 51335', `port: ${gatewayPort}`).replace('policy: prompt', 'policy: off');
  await writeFile(eefConfig, yaml);
  launch('eef', ['--config', eefConfig, '--database', join(scratch, 'eef.db'), '--no-brain', '--no-ui'], {EEF_NODE_PSK: secret});
  launch('eefn', ['--config', configPath, '--no-ui']);
  const ready = async () => {
    node = 'http://' + JSON.parse(await readFile(marker, 'utf8')).address;
    return (await json(node + '/api/status')).connection.state === 'connected'
      && (await json(eef + '/api/diagnostics')).registered_node_count === 1;
  };
  await until(ready, 'registered node');
  for (const base of [node, eef]) assert.equal((await fetch(base + '/')).status, 404);
  const before = await readFile(configPath, 'utf8'), text = '  Is this node running?\n';
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
  assert.equal(disconnected.error_code, 'coordinator_unavailable'); assert.equal(disconnected.outcome_unknown, false);
  assert.equal(disconnected.coordinator_acknowledged, false); assert.equal(disconnected.result, undefined);
  assert.equal((await preview('Local preview still works')).success, true);
  await cli(['connection', 'resume']); await until(ready, 'resumed connection');
  const jobs = await cli(['jobs', 'list']); assert.deepEqual(jobs.data.jobs, []); assert.equal(jobs.data.total, 0);
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
  assert.equal(calls.length,beforeCompile,'compilation must not invoke inference');
  const modelReused=await json(planUrl,planRequest);assert.equal(modelReused.reused,true);assert.deepEqual(modelReused.plan,modelPlan.plan);
  await cli(['jobs','stop',modelDraft.result.id]);await cli(['jobs','remove',modelDraft.result.id]);
  for(const id of [durable.result.id,found.jobs[0].id]) {await cli(['jobs','stop',id]);await cli(['jobs','remove',id]);}
  assert.equal((await cli(['jobs','list'])).data.total,0);
  assert.deepEqual(JSON.parse(await readFile(configPath, 'utf8')), JSON.parse(before), 'review leaves config unchanged');
  await writeFile(join(scratch, 'results.json'), JSON.stringify({passed: true, api_only: true, node_to_coordinator_review: true,
    original_text_preserved: true, authenticated_origin: true, clarification_returned: true, rejected_authority_fields: true,
    stateless_no_jobs_created: true, durable_inferred_review: true, lost_draft_found_without_replay: true,
    no_execution: true, restart_invalidates_inference: true, lost_reply_not_replayed: true,
    explicit_model_plan_without_inference:true,owner_role_binding:true,real_inference: false, physical_two_pc: false}, null, 2));
  console.log('Request review checks passed: ' + scratch);
} finally {
  for (const response of gates) response.destroy();
  for (const child of children) if (child.exitCode === null) child.kill();
  await Promise.allSettled(children.map(c => c.result));
  for (const server of [fake, proxy]) {server.closeAllConnections(); await new Promise(r => server.close(r));}
}
