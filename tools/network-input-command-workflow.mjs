// Two logical nodes on one hosted Windows runner; fake inference, real commands.
import assert from 'node:assert/strict';
import {access, mkdtemp, readFile, writeFile} from 'node:fs/promises';
import {join, resolve} from 'node:path';
import {spawn} from 'node:child_process';
import {createServer} from 'node:http';
import {randomBytes, randomUUID} from 'node:crypto';
import {coordinatorTemplatePorts, readLoopbackCoordinatorTemplate} from './local-test-network.mjs';

export async function networkInputCommandWorkflow() {
  const root = resolve(import.meta.dirname, '..');
  const bin = resolve(process.env.EEF_TEST_BINARY_DIR || join(root, 'target/debug'));
  await Promise.all(['eef.exe', 'eefn.exe'].map(name => access(join(bin, name))));
  const scratch = await mkdtemp(join(root, '.validation/network-input-commands-'));
  const config = join(scratch, 'eef.yaml'), children = [], usedPorts = new Set();
  const inputId = 'input-' + randomUUID(), providerId = 'provider-' + randomUUID();
  const providerName = 'serving fixture';
  const env = {...process.env, EEF_NODE_PSK: '', APPDATA: join(scratch, 'appdata'),
    EEF_DISCOVERY_DIR: join(scratch, 'discovery')};
  const calls = [], unexpectedLocalCalls = [], fakeErrors = [];
  function launch(name, file, args, extra = {}) {
    const child = spawn(join(bin, name + '.exe'), ['--config', file, ...args],
      {cwd: root, windowsHide: true, env: {...env, ...extra}, stdio: ['ignore', 'pipe', 'pipe']});
    let out = '', err = '';
    child.stdout.on('data', bytes => out = (out + bytes).slice(-1048576));
    child.stderr.on('data', bytes => err = (err + bytes).slice(-1048576));
    child.result = new Promise((yes, no) => {
      child.on('error', no);
      child.on('close', code => yes({code, out, err}));
    });
    // Services may fail before the readiness poll; their rejection is observed
    // again during cleanup, without leaving an unhandled Promise rejection.
    child.result.catch(() => {});
    children.push(child);
    return child;
  }
  async function cli(name, file, args, success = true) {
    const child = launch(name, file, ['--json', ...args]);
    const timer = setTimeout(() => child.kill(), 45000);
    try {
      const result = await child.result;
      if (result.code !== (success ? 0 : 1)) {
        let report; try { report = JSON.parse(result.out); } catch {}
        const code = value => typeof value === 'string' && /^[a-z0-9_]{1,100}$/.test(value) ? value : undefined;
        const observation = {app: name, command: args.slice(0, 2), exit_code: result.code,
          error_code: code(report?.error_code), status: code(report?.status),
          interpretation_error: code(report?.interpretation?.error_code), preparation_error: code(report?.preparation?.error_code),
          interpreter_error: code(report?.preparation?.network_interpreter?.error_code),
          interpreter_phase: code(report?.preparation?.network_interpreter?.result?.attempt?.phase),
          model_calls: calls.length, local_model_calls: unexpectedLocalCalls.length};
        await writeFile(join(scratch, 'failure.json'), JSON.stringify(observation, null, 2));
        assert.equal(result.code, success ? 0 : 1, JSON.stringify(observation));
      }
      return JSON.parse(result.out);
    } finally { clearTimeout(timer); }
  }
  const owner = args => cli('eef', config, args);
  async function json(url) {
    const result = await fetch(url, {signal: AbortSignal.timeout(5000)});
    assert(result.ok, 'Fixture status HTTP ' + result.status);
    return result.json();
  }
  async function until(read, label) {
    const end = Date.now() + 45000;
    while (Date.now() < end) {
      try { if (await read()) return; } catch {}
      await new Promise(yes => setTimeout(yes, 100));
    }
    throw Error('Network command fixture timed out: ' + label);
  }
  async function listen(server) {
    await new Promise((yes, no) => { server.once('error', no); server.listen(0, '127.0.0.1', yes); });
    return server.address().port;
  }
  async function port() {
    for (let attempt = 0; attempt < 32; attempt++) {
      const socket = createServer();
      const value = await listen(socket);
      await new Promise(yes => socket.close(yes));
      if (!usedPorts.has(value)) { usedPorts.add(value); return value; }
    }
    throw Error('No distinct loopback port available for command fixture');
  }
  const fake = createServer(async (request, response) => {
    try {
      response.setHeader('Content-Type', 'application/json');
      if (request.url.startsWith('/origin/')) {
        if (request.url.endsWith('/api/chat')) unexpectedLocalCalls.push(request.url);
        return response.end(JSON.stringify({models: []}));
      }
      if (request.url === '/api/tags') return response.end(JSON.stringify({models: [{name: 'fixture'}]}));
      if (request.url === '/api/show') return response.end(JSON.stringify({capabilities: ['completion']}));
      if (request.url !== '/api/chat') { response.writeHead(404); return response.end('{}'); }
      let bytes = '';
      for await (const chunk of request) {
        bytes += chunk;
        if (Buffer.byteLength(bytes) > 65536) { response.writeHead(413); return response.end('{}'); }
      }
      const body = JSON.parse(bytes); calls.push(body);
      const result = body.format && typeof body.format === 'object'
        ? {schema_version: 1, intent: 'information', goal: 'Inspect node status', context_hints: [],
          constraints: [], suggested_capabilities: ['system.info'], complexity: 'simple',
          workload: 'one_shot', needs_clarification: false, clarification: null}
        // Semantic candidates may name a node; internal routing IDs are bound
        // later by the deterministic compiler and must not come from a model.
        : {schema_version: 1, method: {schema_version: 1, template: 'system_info', params: {},
          constraints: {node_name: providerName}}, clarification: null};
      response.end(JSON.stringify({message: {content: JSON.stringify(result)}, done: true, done_reason: 'stop'}));
    } catch (error) {
      fakeErrors.push(String(error));
      response.writeHead(500); response.end('{}');
    }
  });
  try {
    const fakePort = await listen(fake); usedPorts.add(fakePort);
    const web = await port(), gateway = await port(), ownerApi = `http://127.0.0.1:${web}`;
    const yaml = coordinatorTemplatePorts(await readLoopbackCoordinatorTemplate(), web, gateway)
      .replace('policy: prompt', 'policy: off');
    await writeFile(config, yaml);
    launch('eef', config, ['--database', join(scratch, 'jobs.db'), '--no-brain', '--no-ui'],
      {EEF_NODE_PSK: randomBytes(24).toString('hex')});
    await until(async () => (await json(ownerApi + '/api/diagnostics')).initialized, 'owner startup');
    const nodes = [];
    for (const [id, serving] of [[inputId, false], [providerId, true]]) {
      const file = join(scratch, (serving ? 'provider' : 'input') + '.json');
      const apiPort = await port(), api = `http://127.0.0.1:${apiPort}`;
      const value = JSON.parse(await readFile(join(root, 'config/node.example.json'), 'utf8'));
      Object.assign(value, {node_id: id, name: serving ? providerName : 'input fixture', psk: '',
        endpoints: [], auto_local: false, local_pairing: false, heartbeat_seconds: 0.5,
        metadata: {area: ['fixture', serving ? 'execution' : 'input']}, update: {policy: 'off'},
        input_workflow: {prepare_on_input: true, resolve_on_answer: true, network_interpreter: false},
        dashboard: {enabled: true, ui_enabled: false, host: '127.0.0.1', port: apiPort},
        models: {provider: 'ollama', ollama: {base_url: `http://127.0.0.1:${fakePort}${serving ? '' : '/origin'}`,
          selected: serving ? [{model_id: 'fixture', modality: 'text', selection: {roles: ['request_interpreter']}}] : []},
          llamacpp: {slots: []}}});
      value.permissions.interpreter_sharing = serving;
      await writeFile(file, JSON.stringify(value));
      launch('eefn', file, ['--no-ui']);
      await until(async () => (await json(api + '/api/status')).node_id === id, 'standalone node');
      const invitation = await owner(['credentials', 'provision', '--node', id, '--address', `127.0.0.1:${gateway}`]);
      assert.equal(invitation.private, true);
      const decoded = JSON.parse(Buffer.from(invitation.code, 'base64').toString('utf8'));
      assert.equal(decoded.node_id, id);
      const privateFile = join(scratch, (serving ? 'provider' : 'input') + '-PRIVATE.json');
      await writeFile(privateFile, JSON.stringify(invitation));
      nodes.push({id, serving, file, api, privateFile, credential: decoded.psk});
    }
    const [input, provider] = nodes;
    assert.notEqual(input.credential, provider.credential, 'each node needs its own credential');
    const a = (args, success = true) => cli('eefn', input.file, args, success);
    const b = (args, success = true) => cli('eefn', provider.file, args, success);
    await owner(['credentials', 'legacy', '--allow', 'false']);
    await owner(['discovery', 'grant', '--requester', inputId, '--target', providerId]);
    const runtime = (await owner(['diagnostics'])).runtime_id;
    await owner(['restart']);
    await until(async () => (await owner(['diagnostics'])).runtime_id !== runtime, 'applied credentials and disclosure');
    for (const node of nodes) {
      const paired = await cli('eefn', node.file, ['connection', 'pair', '--code-file', node.privateFile]);
      assert.equal(paired.saved_auth_mode, 'node_credential');
      await until(async () => {
        const status = await json(node.api + '/api/status');
        return status.connection.state === 'connected' && status.connection.transport === 'pinned_tls13';
      }, 'pinned node connection');
    }
    await until(async () => (await owner(['diagnostics'])).registered_node_count === 2, 'both logical nodes');
    for (const api of [ownerApi, input.api, provider.api]) assert.equal((await fetch(api + '/')).status, 404);
    const policy = await a(['requests', 'workflow', 'set', '--network-interpreter', 'true']);
    assert.equal(policy.saved_policy.network_interpreter, true);
    await a(['restart', '--wait-seconds', '15']);
    await until(async () => (await json(input.api + '/api/status')).connection.state === 'connected', 'input policy restart');
    assert.equal((await a(['requests', 'workflow', 'show'])).applied_policy.network_interpreter, true);
    assert.deepEqual(JSON.parse(await readFile(input.file, 'utf8')).models.ollama.selected, []);
    await owner(['interpreter-policies', 'approve', '--origin', inputId, '--provider', providerId,
      '--max-calls', '2', '--valid-for-seconds', '120']);
    await until(async () => (await owner(['interpreter-policies', 'resolve', '--origin', inputId])).result?.selected?.connection.node_id === providerId,
      'authorized interpreter route');
    assert.equal(calls.length, 0, 'routing and policy setup cannot run inference');
    const original = '  Inspect the other node status\n', operation = randomUUID();
    const inputArgs = ['requests', 'input', '--text', original, '--operation-id', operation];
    const opened = await a(inputArgs);
    assert.equal(opened.success, true);
    assert.equal(opened.status, 'awaiting_approval');
    assert.equal(opened.execution_authorized, false); assert.equal(opened.dispatched, false);
    for (const stage of ['interpretation', 'preparation']) {
      assert.equal(opened[stage].network_interpreter.result.attempt.provider, providerId);
      assert.equal(opened[stage].network_interpreter.result.model_inference_requested, true);
    }
    assert.equal(opened.interpretation.model_inference_requested, false);
    assert.equal(calls.length, 2); assert.deepEqual(unexpectedLocalCalls, []);
    const id = opened.result.id, view = (await a(['requests', 'plan', id])).result;
    assert.equal(view.bindings.length, 1); assert.equal(view.bindings[0].node_id, providerId);
    const frozen = (await owner(['requests', 'inspect', id])).plan;
    assert.equal(frozen.goal.request_context.origin_node, inputId);
    assert.equal(frozen.request_review.original_text, original);
    assert.equal(frozen.status, 'awaiting_approval');
    assert(!Object.hasOwn(frozen, 'approval')); assert(!Object.hasOwn(frozen, 'execution'));
    const denied = await a(['requests', 'execute', id, '--revision', String(view.revision),
      '--approval-digest', 'a'.repeat(64), '--operation-id', randomUUID()], false);
    assert.equal(denied.success, false);
    assert.deepEqual((await owner(['requests', 'inspect', id])).plan, frozen);
    assert.equal((await a(inputArgs)).success, true); assert.equal(calls.length, 2, 'input receipt reuse must not infer again');
    const approved = await owner(['requests', 'approve', id, '--revision', String(view.revision),
      '--digest', view.plan_sha256, '--valid-for-ms', '60000', '--max-execution-ms', '5000',
      '--max-result-bytes', '16384', '--operation-id', randomUUID()]);
    assert.equal(approved.plan.status, 'approved'); assert.equal(calls.length, 2);
    const execution = ['requests', 'execute', id, '--revision', String(approved.plan.revision),
      '--approval-digest', approved.plan.approval.approval_sha256, '--operation-id', randomUUID()];
    assert.equal((await a(execution)).success, true);
    await until(async () => (await a(['jobs', 'get', id])).data.status === 'completed', 'remote system-info completion');
    const completed = (await owner(['requests', 'inspect', id])).plan;
    assert.equal(completed.execution.attempts.length, 1);
    const attempt = completed.execution.attempts[0];
    assert.equal(attempt.grant.executor_node, providerId);
    assert.equal(attempt.grant.request_context.origin_node, inputId);
    assert.equal(attempt.report.executor_node, providerId); assert.equal(attempt.report.success, true);
    const evidence = await a(['requests', 'result', id, '--grant-id', attempt.grant.grant_id]);
    assert.equal(evidence.result.result.availability, 'retained_final_report');
    assert.equal(evidence.result.result.report.executor_node, providerId);
    assert.equal(evidence.result.result.report.success, true);
    assert.notEqual(evidence.result.result.report.data, null);
    assert.deepEqual((await b(['grants', 'result', '--id', attempt.grant.grant_id])).result, evidence.result.result);
    assert.equal((await b(['requests', 'plan', id], false)).success, false, 'executor cannot read the origin-owned plan');
    assert.equal((await a(execution)).result.reused, true);
    assert.deepEqual((await owner(['requests', 'inspect', id])).plan, completed);
    assert.equal(calls.length, 2); assert.deepEqual(unexpectedLocalCalls, []); assert.deepEqual(fakeErrors, []);
    const result = {passed: true, model_free_input: true, private_tls_nodes: true,
      bounded_network_interpretation_calls: 2, separate_owner_approval: true, remote_executor_report: true,
      result_read_through_origin: true, executor_plan_denied: true, receipt_reuse_without_replay: true,
      dashboard_required: false, real_inference: false, physical_two_pc: false};
    await writeFile(join(scratch, 'results.json'), JSON.stringify(result, null, 2));
    console.log('Public network input-to-execution command validation passed: ' + scratch);
    return result;
  } finally {
    for (const child of children) if (child.exitCode === null) child.kill();
    await Promise.allSettled(children.map(child => child.result));
    fake.closeAllConnections();
    await new Promise(yes => fake.close(yes));
  }
}
