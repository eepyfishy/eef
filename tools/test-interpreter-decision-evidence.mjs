// Read-only regrading of one retained synthetic screen; no inference/approval.
import assert from 'node:assert/strict';
import {readFile,realpath,stat} from 'node:fs/promises';
import {resolve,join,sep} from 'node:path';
import {pathToFileURL} from 'node:url';
import {reference,decision,grade,sha,screenPassed} from './interpreter-decision-screen.mjs';
import {caseSet} from './interpreter-generalization-cases.mjs';
export function validateEvidence(report,recorded,runner,catalogEntries){
assert.equal(report.schema_version,1);
const catalog=catalogEntries.filter(e=>e.id===report.candidate);
assert.equal(catalog.length,1);assert.equal(report.model_sha256,catalog[0].sha256);assert(/^[a-f0-9]{64}$/.test(report.runtime_sha256));
for(const flag of ['model_downloads','model_copies','coordinator_started','execution_authorized'])assert.equal(report[flag],false);
assert.equal(report.harness_completed,true);assert.equal(report.default_approved,false);assert.equal(report.production_changed,false);
assert.equal(report.context_tokens,4096);assert.equal(report.threads,4);assert.equal(report.gpu_layers,0);
assert.equal(report.output_token_limit,128);assert.equal(report.inference_timeout_seconds,20);assert.equal(report.thinking_hint,'disabled');assert.equal(report.reasoning_effort,'none');
assert.equal(report.runner_sha256,sha(runner));
const ref=reference(report.capabilities);assert.deepEqual(recorded,ref);
assert.equal(report.prompt_sha256,sha(ref.system));assert.equal(report.schema_sha256,sha(JSON.stringify(ref.schema)));
assert.equal(report.suites.length,2);
for(const id of ['original-v1','generalization-v1']){
 const suite=caseSet(id),matches=report.suites.filter(s=>s.id===id);assert.equal(matches.length,1);const observed=matches[0];
 assert.equal(observed.case_sha256,sha(JSON.stringify(suite.cases)));assert.equal(observed.cases.length,20);
 for(let i=0;i<20;i++){
  const item=suite.cases[i],row=observed.cases[i];assert.equal(row.id,item.id);assert.equal(row.input,item.input);
  let parsed=null;try{assert.equal(row.finish_reason,'stop');assert(Number.isSafeInteger(row.completion_tokens)&&row.completion_tokens<=128&&row.completion_tokens>=0);parsed=decision(row.output,report.capabilities);}catch{}
  assert.deepEqual(parsed,row.decision);assert.deepEqual(grade(item,parsed),row.reasons);assert.equal(row.matched,row.reasons.length===0);
  assert.equal(row.required_question_lost,item.clarification&&parsed?.needs_clarification===false);
  assert.equal(row.continuous_to_one_shot,!item.workloads.includes('one_shot')&&parsed?.workload==='one_shot');
 }
 assert.equal(observed.valid_count,observed.cases.filter(c=>c.decision).length);assert.equal(observed.matched_count,observed.cases.filter(c=>c.matched).length);
 assert.equal(observed.required_questions_lost,observed.cases.filter(c=>c.required_question_lost).length);assert.equal(observed.continuous_regressions,observed.cases.filter(c=>c.continuous_to_one_shot).length);
}
assert.equal(report.screen_passed,screenPassed(report.suites));
return {retained_evidence_verified:true,model_quality_approved:false,screen_passed:report.screen_passed,matches:report.suites.map(s=>s.matched_count),
 scope:'Reference/archived-source/case hashes and independent regrading, not inference replay or provider attestation'};
}
async function main(){
 assert.equal(process.argv.length,3,'Supply one existing decision-screen directory');
 const validation=await realpath(resolve(import.meta.dirname,'../.validation'));
 const folder=await realpath(resolve(process.argv[2]));assert(folder.startsWith(validation+sep));
 async function bounded(file){const path=join(folder,file);assert((await stat(path)).size<=262144);return await readFile(path,'utf8');}
 const report=JSON.parse(await bounded('results.json')),recorded=JSON.parse(await bounded('PRIVATE-reference.json'));
 const runner=await bounded('PRIVATE-evaluated-runner.mjs'),catalog=JSON.parse(await readFile(resolve(import.meta.dirname,'../config/model-catalog.json'),'utf8'));
 console.log(JSON.stringify(validateEvidence(report,recorded,runner,catalog)));
}
if(process.argv[1]&&pathToFileURL(resolve(process.argv[1])).href===import.meta.url){
 try{await main();}catch{console.error('Decision-screen evidence not confirmed; inspect private fixture locally. No inference or approval.');process.exitCode=1;}
}
