// Test-only public-command adapter. No raw RPC, journal edits, retries, models
// or production policy exceptions. Fixtures must explicitly pair TLS first.
import assert from 'node:assert/strict';
import {randomUUID} from 'node:crypto';
export function reviewedFixture(eef, node, id) {
  async function exchange(url, body) {
    const response = await fetch(url, {method:body?'POST':'GET',
      headers:{'Content-Type':'application/json'}, body:body?JSON.stringify(body):undefined,
      signal:AbortSignal.timeout(10000)});
    const value = await response.json();
    assert(response.ok, JSON.stringify(value));
    assert.notEqual(value.success, false, JSON.stringify(value));
    return value;
  }
  const get = job => exchange(eef+'/api/jobs/'+job);
  async function save(input, text='Explicit isolated fixture') {
    const value = await exchange(node+'/api/commands/requests/submit', {
      schema_version:1, expected_node_id:id, command:{operation_id:randomUUID(),
        request:{schema_version:1,original_text:text,title:'Isolated reviewed fixture',
          input:{schema_version:1,...input}}}});
    assert.equal(value.execution_authorized,false);assert.equal(value.dispatched,false);
    const plan=value.result.plan;
    assert.equal(plan.status,'awaiting_approval');assert.equal(plan.request_context.origin_node,id);
    assert(plan.tasks.every(t=>t.status==='pending'));
    return plan;
  }
  async function approve(plan, limits={max_execution_ms:5000,max_result_bytes:65536}) {
    const runtime=(await exchange(eef+'/api/diagnostics')).runtime_id;
    const approval=await exchange(eef+'/api/commands/requests/planning', {
      schema_version:1,expected_runtime_id:runtime,command:{operation:'approve',
        id:plan.id,expected_revision:plan.revision,operation_id:randomUUID(),
        plan_sha256:plan.plan_sha256,valid_for_ms:60000,limits}});
    assert.equal(approval.plan.status,'approved');
    return {id:plan.id,expected_revision:approval.plan.revision,operation_id:randomUUID(),
      approval_sha256:approval.plan.approval.approval_sha256};
  }
  async function start(command) {
    const value=await exchange(node+'/api/commands/requests/execute', {
      schema_version:1,expected_node_id:id,command});
    assert.equal(value.automatically_retried,false);
    return value;
  }
  async function startOwner(command) {
    return exchange(eef+'/api/commands/requests/execute', {
      schema_version:1,expected_runtime_id:(await exchange(eef+'/api/diagnostics')).runtime_id,command});
  }
  async function wait(job) {
    const deadline=Date.now()+45000;
    while(Date.now()<deadline) {
      const value=await get(job);
      if(['completed','failed','interrupted','cancelled','abandoned'].includes(value.status))return value;
      await new Promise(r=>setTimeout(r,100));
    }
    throw Error('Reviewed fixture did not settle: '+job);
  }
  async function result(job) {
    const metadata=await wait(job);
    assert.equal(metadata.grant_attempts.length,1,'one exact issued grant required');
    const value=await exchange(node+'/api/commands/requests/result', {
      schema_version:1,expected_node_id:id,request_id:randomUUID(),
      command:{id:job,grant_id:metadata.grant_attempts[0].grant_id}});
    assert.equal(value.execution_requested,false);assert.equal(value.reconciled,false);
    const retained=value.result.result;
    assert.equal(retained.availability,'retained_final_report');
    assert(['completed','unknown'].includes(retained.report.state));
    return retained.report;
  }
  return {save,approve,start,startOwner,get,wait,result};
}
