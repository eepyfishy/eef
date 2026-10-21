// Metadata-only: no model/runtime, network, process or production changes.
import assert from 'node:assert/strict';
import {reference,decision,grade,evaluateSuite,screenPassed,sha} from './interpreter-decision-screen.mjs';
import {validateEvidence} from './test-interpreter-decision-evidence.mjs';
import {caseSet} from './interpreter-generalization-cases.mjs';
const caps=['system.info','system.ping'],ref=reference(caps);
for(const branch of ref.schema.anyOf){assert.deepEqual(Object.keys(branch.properties),['intent','workload','needs_clarification','suggested_capabilities']);assert.equal(branch.additionalProperties,false);}
assert.deepEqual(ref.schema.anyOf[0].properties.needs_clarification.enum,[false]);assert.deepEqual(ref.schema.anyOf[1].properties.needs_clarification.enum,[true]);
assert(!ref.schema.anyOf[0].properties.intent.enum.includes('unknown'));assert(!ref.schema.anyOf[0].properties.workload.enum.includes('unknown'));
assert(!ref.system.includes("Summarize the user's goal"));
const clear={intent:'information',workload:'one_shot',needs_clarification:false,suggested_capabilities:['system.info']};
assert.deepEqual(decision(JSON.stringify(clear),caps),clear);
for(const bad of [null,[],{}, {...clear,grant:true},{...clear,intent:'command'},{...clear,workload:'unknown'},{...clear,needs_clarification:'false'},
 {...clear,suggested_capabilities:['filesystem.write']},{...clear,suggested_capabilities:['system.info','system.info']},{...clear,suggested_capabilities:'system.info'}]){
 assert.throws(()=>decision(JSON.stringify(bad),caps));
}
for(const bad of ['```json\n{}\n```','{} trailing','x'.repeat(4097)])assert.throws(()=>decision(bad,caps));
assert.throws(()=>decision(JSON.stringify(clear).replace('"intent":"information"','"intent":"action","intent":"information"'),caps));
assert.deepEqual(decision(JSON.stringify(clear).replace('"intent"','"inten\\u0074"'),caps),clear);
for(const bad of [null,['bad name'],['system.info','system.info'],new Array(65).fill('system.info')])assert.throws(()=>reference(bad));
assert.equal(reference([]).schema.anyOf[0].properties.suggested_capabilities.maxItems,0);
for(const id of ['original-v1','generalization-v1']){
 const cases=caseSet(id).cases;assert.equal(cases.length,20);assert.deepEqual(grade(cases[0],clear),[]);
 assert.deepEqual(grade(cases[0],null),['invalid_decision']);assert(grade(cases.find(c=>c.clarification),clear).includes('clarification'));
 assert(grade(cases[0],{...clear,suggested_capabilities:[]}).includes('capability:system.info'));
}
const completion=(value=clear)=>({choices:[{message:{content:JSON.stringify(value)},finish_reason:'stop'}],usage:{completion_tokens:45}});
const sample={id:'unit',cases:caseSet().cases.slice(0,2)};
let calls=0;const failed={cases:[]};
await assert.rejects(evaluateSuite(failed,sample,caps,async()=>{calls++;throw Error('offline');}));
assert.equal(calls,1);assert.equal(failed.cases.length,1);assert.equal(failed.cases[0].error_code,'backend_request_failed');assert.equal(failed.valid_count,undefined);
calls=0;let live=true;const interrupted={cases:[]};
await assert.rejects(evaluateSuite(interrupted,sample,caps,async()=>{calls++;live=false;return completion();},()=>live));
assert.equal(calls,1);assert.equal(interrupted.valid_count,undefined);
calls=0;await assert.rejects(evaluateSuite({cases:[]},sample,caps,async()=>{calls++;return completion();},()=>false));assert.equal(calls,0);
const malformed={cases:[]};calls=0;
await evaluateSuite(malformed,sample,caps,async()=>{calls++;const value=completion();value.choices[0].finish_reason='length';return value;});
assert.equal(calls,2,'Different cases run once; no retries of the same failed output');assert.equal(malformed.valid_count,0);assert.equal(malformed.matched_count,0);
for(const bad of [()=>({choices:[],usage:{completion_tokens:45}}),()=>({...completion(),usage:{completion_tokens:129}}),()=>({...completion(),usage:{}})]){
 const result={cases:[]};await evaluateSuite(result,{id:'unit',cases:sample.cases.slice(0,1)},caps,async()=>bad());assert.equal(result.valid_count,0);
}
const suites=[];
for(const id of ['original-v1','generalization-v1']){
 const result={id,cases:[]};const suite=caseSet(id);
 await evaluateSuite(result,suite,caps,async item=>completion({intent:item.intents[0],workload:item.workloads[0],needs_clarification:item.clarification,suggested_capabilities:item.capabilities||[]}));
 assert.equal(result.matched_count,20);suites.push(result);
}
assert.equal(screenPassed(suites),true);assert.equal(screenPassed([]),false);assert.equal(screenPassed([suites[0],suites[0]]),false);
for(const mutate of [r=>r.cases.pop(),r=>{r.cases[0].id='substituted';},r=>{r.cases[0].decision=null;},r=>{r.cases[0].required_question_lost=true;},
 r=>{r.cases[0].continuous_to_one_shot=true;},r=>{r.cases.slice(0,3).forEach(c=>{c.matched=false;});}]){
 const copy=structuredClone(suites);mutate(copy[0]);assert.equal(screenPassed(copy),false);
}
// Deliberately fabricated metadata, not a model evaluation or persisted result.
const runner='synthetic unit runner, no inference',catalog=[{id:'unit-model',sha256:'a'.repeat(64)}];
const evidence={schema_version:1,candidate:'unit-model',model_sha256:catalog[0].sha256,runtime_sha256:'b'.repeat(64),model_downloads:false,model_copies:false,
 coordinator_started:false,execution_authorized:false,harness_completed:true,default_approved:false,production_changed:false,context_tokens:4096,threads:4,
 gpu_layers:0,output_token_limit:128,inference_timeout_seconds:20,thinking_hint:'disabled',reasoning_effort:'none',capabilities:caps,
 runner_sha256:sha(runner),prompt_sha256:sha(ref.system),schema_sha256:sha(JSON.stringify(ref.schema)),screen_passed:true,suites:structuredClone(suites)};
for(const suite of evidence.suites)suite.case_sha256=sha(JSON.stringify(caseSet(suite.id).cases));
assert.equal(validateEvidence(evidence,ref,runner,catalog).model_quality_approved,false);
for(const mutate of [r=>{r.harness_completed=false;},r=>{r.default_approved=true;},r=>{r.runner_sha256='0'.repeat(64);},r=>{r.model_sha256='0'.repeat(64);},
 r=>{r.schema_sha256='0'.repeat(64);},r=>{r.screen_passed=false;},r=>{r.suites[0].cases[0].input='substituted';},r=>{r.suites[0].matched_count=19;},
 r=>{r.suites[0].cases[0].matched=false;},r=>{r.suites[0].cases[0].finish_reason='length';},r=>{r.suites[0].cases[0].decision=null;},r=>{r.suites[0].cases.pop();}]){
 const copy=structuredClone(evidence);mutate(copy);assert.throws(()=>validateEvidence(copy,ref,runner,catalog));
}
assert.throws(()=>validateEvidence(evidence,{...ref,system:'changed'},runner,catalog));
assert.throws(()=>validateEvidence(evidence,ref,runner+' changed',catalog));
console.log('Decision-screen metadata/lifetime checks passed; no inference or quality approval');
