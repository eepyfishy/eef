// Shipping CLI/local API path with no eligible model; no inference or downloads.
import assert from 'node:assert/strict';
import {randomUUID} from 'node:crypto';
export async function checkInterpreterCommandUnavailable({local,node,nodeId}) {
 const saved=await local(['requests','save-input','--text','Retain this model-free interpreter fixture']);
 assert.equal(saved.success,true);const id=saved.result.id,operation=randomUUID();
 const rejected=await local(['requests','interpret-network',id,'--operation-id',operation],false);
 assert.equal(rejected.report_type,'node_interpreter_command');assert.equal(rejected.success,false);
 assert.equal(rejected.command.id,id);assert.equal(rejected.command.operation_id,operation);
 assert.equal(rejected.error_code,'coordinator_rejected');assert.equal(rejected.coordinator_acknowledged,false);
 for(const flag of ['execution_authorized','dispatched','automatically_retried'])assert.equal(rejected[flag],false,flag);
 const plain=await local(['requests','interpret-network',id],false,true);
 assert(plain.out.includes('Interpreter result unavailable'));assert(plain.out.includes('eefn jobs get '+id));
 const request={schema_version:1,expected_node_id:nodeId,expected_runtime_id:saved.runtime_id,
  command:{id,stage:'initial',expected_revision:null,operation_id:randomUUID(),preferences:{local_only:false,provider:null,backend:null,model_id:null}}};
 const post=(body,headers={})=>fetch(node+'/api/commands/requests/interpreter',{method:'POST',headers:{'Content-Type':'application/json',...headers},body:JSON.stringify(body),signal:AbortSignal.timeout(15000)});
 assert.equal((await post(request,{Origin:'https://untrusted.example'})).status,403);
 const stale=await post({...request,expected_runtime_id:randomUUID()});assert.equal(stale.status,200);
 const staleBody=await stale.json();assert.equal(staleBody.error_code,'runtime_changed');assert.equal(staleBody.outcome_unknown,false);
 assert.equal((await post({...request,expected_node_id:'other-origin'})).status,400);
 assert([400,422].includes((await post({...request,command:{...request.command,prompt:'forged'}})).status));
 assert.equal((await post({...request,padding:'x'.repeat(8192)})).status,413);
 const inspected=await local(['jobs','get',id]);assert.equal(inspected.data.id,id);
 assert.equal(inspected.data.task_count,0);assert(inspected.data.input_interpretation==null);
 console.log('Network interpreter command unavailable-path validation passed');
}
