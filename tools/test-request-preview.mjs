// API-only owner preview acceptance. Fake model, no EEF, jobs, downloads or actions.
import assert from 'node:assert/strict';
import {mkdtemp, readFile, writeFile} from 'node:fs/promises';
import {resolve, join} from 'node:path';
import {spawn} from 'node:child_process';
import {createServer} from 'node:http';
import {randomUUID} from 'node:crypto';
const root = resolve(import.meta.dirname, '..');
const scratch = await mkdtemp(join(root, '.validation/request-preview-'));
const bin = resolve(process.env.EEF_TEST_BINARY_DIR || join(root, 'target/debug'));
const configPath = join(scratch, 'node.json'), marker = configPath + '.api.json';
const children = [], calls = [], gates = new Set();
const env = {...process.env, EEF_NODE_PSK: '', APPDATA: join(scratch, 'appdata'),
  EEF_DISCOVERY_DIR: join(scratch, 'discovery'), PATH: join(root, '.tooling/llvm-mingw-20260616-ucrt-x86_64/bin') + ';' + process.env.PATH};
function launch(args) {
  const child = spawn(join(bin, 'eefn.exe'), ['--config', configPath, ...args], {cwd: root, windowsHide: true, env, stdio: ['ignore', 'pipe', 'pipe']});
  let out = '', err = '';
  child.stdout.on('data', b => out = (out + b).slice(-1048576));
  child.stderr.on('data', b => err = (err + b).slice(-1048576));
  child.result = new Promise((resolve, reject) => {child.on('error', reject); child.on('close', code => resolve({code, out, err}));});
  children.push(child); return child;
}
async function cli(args, success = true) {
  const child = launch([...args, '--json']), timer = setTimeout(() => child.kill(), 30000);
  try { const result = await child.result; assert.equal(result.code, success ? 0 : 1, result.out + result.err); return JSON.parse(result.out); }
  finally { clearTimeout(timer); }
}
const preview = (text, success = true) => cli(['requests', 'preview', '--text', text], success);
async function until(fn, label) { const deadline = Date.now() + 45000; while (Date.now() < deadline) {try {if (await fn()) return;} catch {} await new Promise(r => setTimeout(r, 100));} throw Error('Timeout: ' + label); }
async function json(url, body, method) { const response = await fetch(url, {method: method || (body ? 'POST' : 'GET'), headers: {'Content-Type': 'application/json'}, body: body ? JSON.stringify(body) : undefined, signal: AbortSignal.timeout(10000)}); const value = await response.json(); assert(response.ok, JSON.stringify(value)); return value; }
async function listen(server) {await new Promise(r => server.listen(0, '127.0.0.1', r)); return server.address().port;}
let mode = 'valid', node = '', proxyMode = 'drop', forwarded = 0;
const proposal = () => ({schema_version: 1, intent: 'information', goal: 'Inspect node status', context_hints: ['this node'], constraints: [], suggested_capabilities: ['system.info'], complexity: 'simple', workload: 'one_shot', needs_clarification: false, clarification: null});
const fake = createServer(async (req, res) => {
  res.setHeader('Content-Type', 'application/json');
  if (req.url === '/api/tags') return res.end(JSON.stringify({models: [{name: 'fixture'}, {name: 'second'}]}));
  if (req.url === '/api/show') return res.end(JSON.stringify({capabilities: ['completion']}));
  if (req.url === '/api/chat') {
    let body = ''; for await (const chunk of req) body += chunk;
    calls.push(JSON.parse(body));
    if (mode === 'gate') {res.write(' '); gates.add(res); res.on('close', () => gates.delete(res)); return;}
    const value = proposal(); if (mode === 'invalid') value.permissions = ['everything'];
    return res.end(JSON.stringify({message: {content: JSON.stringify(value)}, done: true,
      ...(mode === 'unknown' ? {} : {done_reason: mode === 'partial' ? 'length' : 'stop'})}));
  }
  res.writeHead(404); res.end('{}');
});
const proxy = createServer(async (req, res) => {
  forwarded++;
  if (proxyMode === 'old') {res.writeHead(404, {'Content-Type': 'application/json'}); res.end('{}'); return;}
  try {
    let body = ''; for await (const chunk of req) body += chunk;
    const result = await fetch(node + req.url, {method: 'POST', headers: {'Content-Type': 'application/json'}, body});
    await result.text(); res.destroy();
  } catch {res.destroy();}
});
try {
  const fakePort = await listen(fake), proxyPort = await listen(proxy);
  const portProbe = createServer(); const apiPort = await listen(portProbe); await new Promise(r => portProbe.close(r));
  const config = JSON.parse(await readFile(join(root, 'config/node.example.json'), 'utf8'));
  const selected = id => ({model_id: id, modality: 'text', selection: {roles: ['request_interpreter']}});
  Object.assign(config, {node_id: 'preview-fixture-node', name: 'Preview fixture', auto_local: false,
    local_pairing: false, connection_enabled: false, endpoints: [], update: {policy: 'off'},
    dashboard: {enabled: true, ui_enabled: false, host: '127.0.0.1', port: apiPort},
    models: {provider: 'ollama', ollama: {base_url: `http://127.0.0.1:${fakePort}`, selected: [selected('fixture')]}, llamacpp: {slots: []}}});
  await writeFile(configPath, JSON.stringify(config));
  assert.equal((await preview('Check status', false)).error_code, 'node_not_running'); assert.equal(calls.length, 0);
  launch(['--no-ui']);
  await until(async () => {const value = JSON.parse(await readFile(marker, 'utf8')); node = 'http://' + value.address; return (await json(node + '/api/status')).connection.state === 'paused';}, 'paused node ready');
  assert.equal((await fetch(node + '/')).status, 404);
  const before = (await json(node + '/api/config')).config, text = '  Is this node running?\n';
  const good = await preview(text);
  assert.equal(good.execution_authorized, false); assert.equal(good.dispatched, false);
  assert.equal(good.result.model.model_id, 'fixture');
  assert.equal(good.result.interpretation.original_text, text);
  assert.equal(good.result.interpretation.review_state, 'review_required');
  assert.equal(good.result.interpretation.execution_authorized, false);
  assert.equal(calls.length, 1); assert.equal(calls[0].messages.at(-1).content, text);
  assert.equal(calls[0].options.num_predict, 512);
  for (const [next, expected] of [['invalid', 'interpretation_invalid'], ['partial', 'interpretation_incomplete'], ['unknown', 'interpretation_incomplete']]) {
    mode = next; const result = await preview('Check status', false);
    assert.equal(result.error_code, expected); assert.equal(result.dispatched, false); assert.equal(result.result, undefined);
  }
  const endpoint = node + '/api/commands/requests/preview';
  const request = {schema_version: 1, expected_node_id: config.node_id, request_id: randomUUID(), text: 'check'};
  const count = calls.length;
  assert.equal((await json(endpoint, {...request, expected_node_id: 'other'})).error_code, 'invalid_preview_request');
  assert.equal((await json(endpoint, {...request, text: 'x'.repeat(32769)})).error_code, 'invalid_preview_input');
  assert.equal((await fetch(endpoint, {method: 'POST', headers: {'Content-Type': 'application/json', Origin: 'https://untrusted.example'}, body: JSON.stringify(request)})).status, 403);
  assert.equal(calls.length, count);
  mode = 'gate'; const waiting = preview('Wait for backend', false);
  await until(() => gates.size === 1, 'in-flight preview');
  assert.equal((await preview('Second preview', false)).error_code, 'interpreter_busy');
  assert.equal(calls.length, count + 1);
  const restarted = await cli(['restart', '--wait-seconds', '15']); assert.equal(restarted.completed, true);
  assert.equal((await waiting).error_code, 'runtime_changed');
  await until(() => gates.size === 0, 'old inference request dropped');
  mode = 'valid'; const recovered = await preview('Check after restart');
  assert.notEqual(recovered.runtime_id, good.runtime_id); assert.equal(recovered.dispatched, false);
  assert.deepEqual((await json(node + '/api/config')).config, before, 'preview does not edit configuration');
  mode = 'gate'; const beforeTimeout = calls.length, timeoutStart = Date.now();
  assert.equal((await preview('Bounded inference timeout', false)).error_code, 'interpretation_timeout');
  assert(Date.now() - timeoutStart < 26000); assert.equal(calls.length, beforeTimeout + 1);
  await until(() => gates.size === 0, 'timed-out backend request dropped');
  mode = 'valid'; assert.equal((await preview('Capacity released after timeout')).success, true);
  const originalMarker = await readFile(marker, 'utf8');
  try {
    const altered = JSON.parse(originalMarker); altered.address = `127.0.0.1:${proxyPort}`; await writeFile(marker, JSON.stringify(altered));
    const beforeLoss = calls.length;
    const lost = await preview('Lose only the preview reply', false);
    assert.equal(lost.error_code, 'preview_unconfirmed'); assert.equal(lost.automatically_retried, false);
    assert.equal(calls.length, beforeLoss + 1); assert.equal(forwarded, 1);
    proxyMode = 'old'; assert.equal((await preview('Old node', false)).error_code, 'unsupported_command');
    assert.equal(calls.length, beforeLoss + 1); assert.equal(forwarded, 2);
  } finally {await writeFile(marker, originalMarker);}
  const ambiguous = structuredClone(before); ambiguous.models.ollama.selected.push(selected('second'));
  await json(node + '/api/config', {config: ambiguous}, 'PUT'); await cli(['restart', '--wait-seconds', '15']);
  const beforeAmbiguous = calls.length;
  assert.equal((await preview('Choose no implicit model', false)).error_code, 'interpreter_ambiguous');
  for (const selected of ambiguous.models.ollama.selected) selected.selection.roles = [];
  await json(node + '/api/config', {config: ambiguous}, 'PUT'); await cli(['restart', '--wait-seconds', '15']);
  assert.equal((await preview('No role selected', false)).error_code, 'interpreter_unavailable');
  assert.equal(calls.length, beforeAmbiguous, 'missing/ambiguous role never falls back');
  await writeFile(join(scratch, 'results.json'), JSON.stringify({passed: true, api_only: true, coordinator_required: false,
    preview_only: true, original_input_preserved: true, role_required: true, ambiguous_role_rejected: true,
    invalid_and_incomplete_rejected: true, no_configuration_changes: true, concurrency_bounded: true,
    restart_invalidates_inflight: true, inference_timeout_bounded: true, no_automatic_retry: true, no_legacy_fallback: true,
    real_inference: false, physical_two_pc: false}, null, 2));
  console.log('Request preview checks passed: ' + scratch);
} finally {
  for (const response of gates) response.destroy();
  for (const child of children) if (child.exitCode === null) child.kill();
  await Promise.allSettled(children.map(c => c.result));
  for (const server of [fake, proxy]) {server.closeAllConnections(); await new Promise(r => server.close(r));}
}
