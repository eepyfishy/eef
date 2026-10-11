// Test scoring separately from the Rust schema validator. A semantic match is
// meaningful only after the production node accepted the proposal contract.
import assert from 'node:assert/strict';
import {cases,grade,rubricVersion} from './interpreter-quality-cases.mjs';
assert.equal(rubricVersion,1);assert.equal(cases.length,20);
assert.equal(new Set(cases.map(c=>c.id)).size,cases.length);
for(const item of cases){
 assert(item.input.trim()&&Buffer.byteLength(item.input)<=32768);
 const proposal={intent:item.intents[0],workload:item.workloads[0],needs_clarification:item.clarification,
  suggested_capabilities:item.capabilities||[]};
 const accepted={success:true,result:{interpretation:{proposal}}};
 assert.equal(grade(item,accepted).matched,true);
 assert.equal(grade(item,{...accepted,success:false}).matched,false,'failed model replies are never semantic matches');
 assert.equal(grade(item,{...accepted,success:'true'}).matched,false);
 assert.equal(grade(item,{success:true,result:{}}).matched,false);
 for(const field of ['intent','workload','needs_clarification']){
  const changed={...proposal,[field]:field==='needs_clarification'?!item.clarification:'not-an-enum'};
  assert.equal(grade(item,{success:true,result:{interpretation:{proposal:changed}}}).matched,false);
 }
 if(item.capabilities?.length)assert.equal(grade(item,{success:true,result:{interpretation:{proposal:{...proposal,suggested_capabilities:[]}}}}).matched,false);
}
const ambiguous=cases.find(c=>c.id==='ambiguous-delete');
for(const intent of ['action','unknown'])assert.equal(grade(ambiguous,{success:true,result:{interpretation:{proposal:{intent,workload:'unknown',needs_clarification:true}}}}).matched,true);
console.log('Frozen interpreter semantic rubric checks passed; not a model-quality approval');
