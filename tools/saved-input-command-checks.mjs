// Shipping no-ui commands with the existing synthetic Ollama fixture.
import assert from 'node:assert/strict';
import {readFile} from 'node:fs/promises';
import {randomUUID} from 'node:crypto';
export async function verifySavedInputCommands(f) {
 const before=await readFile(f.configPath,'utf8');
 const save=async text=>(await f.nodeCli(['requests','save-input','--text',text])).result;
 const interpret=(id,success=true,operation=randomUUID())=>f.nodeCli(['requests','interpret-saved',id,'--operation-id',operation],success);
 const inspect=async id=>(await f.nodeCli(['jobs','get',id])).data;
 const noExecution=value=>{for(const field of ['execution_authorized','dispatched','automatically_retried'])assert.equal(value[field],false,field);};
 f.setMode('valid');
 const count=f.calls.length,source=await save('PRIVATE_RAW_SAVED_INPUT'),operation=randomUUID();
 assert.equal(f.calls.length,count,'saving input cannot call a model');
 const reviewed=await interpret(source.id,true,operation);noExecution(reviewed);
 assert.equal(reviewed.report_type,'node_saved_input_interpretation');assert.equal(reviewed.success,true);
 assert.equal(reviewed.request_id,source.id);assert.equal(reviewed.historical_snapshot,false);
 assert.equal(reviewed.model_inference_requested,true);assert.equal(f.calls.length,count+1,'one interpretation, no automatic method preparation');
 const checkpoint=reviewed.checkpoint;assert.equal(checkpoint.id,source.id);assert.equal(checkpoint.revision,3);
 assert.equal(checkpoint.record.state,'promoted');assert.equal(checkpoint.record.claim.expected_revision,source.revision);
 assert.equal(checkpoint.record.claim.source_sha256,source.source_sha256);
 assert.equal(checkpoint.record.claim.interpreter.node_id,f.nodeId);assert.equal(checkpoint.record.claim.interpreter.backend,'ollama');
 assert.equal(checkpoint.record.claim.interpreter.model_id,'fixture');
 assert.deepEqual(checkpoint.record.request_context,source.request_context);
 assert.equal(checkpoint.draft.original_text,'PRIVATE_RAW_SAVED_INPUT');
 for(const field of ['inference_authorized','execution_authorized','dispatched'])assert.equal(checkpoint[field],false,field);
 const view=await inspect(source.id);assert.equal(view.status,'review');assert.equal(view.task_count,0);assert.equal(view.planning_state,'not_compiled');
 assert.equal(view.interpretation_state,'promoted');assert.equal(view.can_resume,false);assert.equal(view.grant_attempts,undefined);
 for(const repeated of [operation,randomUUID()]) {
  const retained=await interpret(source.id,true,repeated);noExecution(retained);
  assert.equal(retained.historical_snapshot,true);assert.equal(retained.model_inference_requested,false);
  assert.equal(retained.checkpoint.record.claim.operation_id,operation);assert.equal(f.calls.length,count+1);
 }
 const plain=await f.nodeText(['requests','interpret-saved',source.id],true);
 assert.match(plain,/Inspected retained/);assert(plain.includes(source.id));assert(!plain.includes('PRIVATE_RAW_SAVED_INPUT'));
 const cancelled=await save('Cancelled before interpretation');await f.nodeCli(['jobs','stop',cancelled.id]);
 const cancelledReply=await interpret(cancelled.id,false);noExecution(cancelledReply);
 assert.equal(cancelledReply.error_code,'cancelled');assert.equal(cancelledReply.claim_attempted,false);assert.equal(f.calls.length,count+1);
 const incomplete=await save('Incomplete model response remains saved');
 f.setMode('reasoning-resolution');const failed=await interpret(incomplete.id,false);noExecution(failed);
 assert.equal(failed.error_code,'inference_unconfirmed');assert.equal(failed.outcome_unknown,true);assert.equal(failed.model_inference_requested,true);
 assert(!JSON.stringify(failed).includes('PRIVATE_REASONING_TRACE'));assert.equal(f.calls.length,count+2);
 const interrupted=await inspect(incomplete.id);assert.equal(interrupted.status,'review');assert.equal(interrupted.task_count,0);
 assert.equal(interrupted.interpretation_state,'claimed');assert.equal(interrupted.proposal_count,0);
 const noReplay=await interpret(incomplete.id,false);assert.equal(noReplay.error_code,'existing_attempt');assert.equal(noReplay.model_inference_requested,false);assert.equal(f.calls.length,count+2);
 f.setMode('valid');
 const endpoint=f.nodeUrl+'/api/commands/requests/interpret-saved';
 const body={schema_version:1,expected_node_id:f.nodeId,expected_runtime_id:reviewed.runtime_id,command:{id:source.id,operation_id:randomUUID()}};
 let response=await fetch(endpoint,{method:'POST',headers:{'Content-Type':'application/json',Origin:'https://untrusted.example'},body:JSON.stringify(body)});assert.equal(response.status,403);
 response=await fetch(endpoint,{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify({...body,expected_node_id:'wrong-node'})});assert.equal(response.status,400);
 const question=await save('Saved input with a clarification');f.setMode('clarify');
 const questionPlain=await f.nodeText(['requests','interpret-saved',question.id],true);
 assert.match(questionPlain,/Which room do you mean\?/);assert.equal(f.calls.length,count+3);
 f.setMode('valid');assert.equal(await readFile(f.configPath,'utf8'),before);
 console.log('Saved input interpretation command validation passed');
 return {
  async verifyRetained(expectUnknown=false) {
   const prior=f.calls.length;
   const retained=await interpret(source.id);assert.equal(retained.historical_snapshot,true);assert.equal(retained.model_inference_requested,false);
   assert.equal(retained.checkpoint.record.claim.operation_id,operation);assert.equal(retained.checkpoint.id,source.id);
   const pending=await interpret(incomplete.id,false);assert.equal(pending.error_code,'existing_attempt');assert.equal(pending.model_inference_requested,false);
   if(expectUnknown)assert.equal(pending.source.record.state,'unknown');
   assert.equal(f.calls.length,prior,'history remains readable after restart or model-role withdrawal without inference');
  },
 };
}
