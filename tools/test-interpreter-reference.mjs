// Public test-only capture tooling; copy metadata to fresh isolated scratch.
// Never alter an evaluation, start a node, download a model or infer/execute.
import assert from 'node:assert/strict';
import {mkdtemp,readFile,writeFile,stat,realpath} from 'node:fs/promises';
import {resolve,join,sep} from 'node:path';
import {spawnSync} from 'node:child_process';
import {createHash} from 'node:crypto';
assert.equal(process.argv.length,4,'Supply matching binaries and a completed quality fixture');
const root=resolve(import.meta.dirname,'..'),bin=await realpath(process.argv[2]);
const source=await realpath(process.argv[3]),validation=await realpath(join(root,'.validation'));
assert(source.startsWith(validation+sep));
const original=await readFile(join(source,'results.json'));
const report=JSON.parse(original),scratch=await mkdtemp(join(validation,'reference-capture-'));
const sha=bytes=>createHash('sha256').update(bytes).digest('hex');
const hasSchema=Object.hasOwn(report.prompt_reference,'schema_sha256');
const output=join(scratch,hasSchema?'PRIVATE-generation-reference.json':'PRIVATE-prompt-reference.json');
async function run(value,ok,directory=scratch){
 await writeFile(join(directory,'results.json'),JSON.stringify(value));
 const child=spawnSync(process.execPath,[join(root,'tools/capture-interpreter-reference.mjs'),bin,directory],
  {cwd:root,windowsHide:true,timeout:15000,maxBuffer:131072});
 assert.equal(child.error,undefined);assert.equal(child.status===0,ok);
}
// Unknown suites and known-but-substituted case hashes fail before output.
if(report.backend==='ollama'){
 for(const changes of [{real_cpu_interpretation:true},{shared_service_owned:true},{model_file_hash_verified:true},
   {provider_reported_model_digest:'not-a-digest'},{sha256:'a'.repeat(64)},{runtime_sha256:'a'.repeat(64)},{gpu_layers:0},{threads:1}])
  await run({...report,...changes},false);
}else{
 await run({...report,backend:'unsupported'},false);
 await run({...report,shared_ollama_provider:true},false);
 const oldCpu=structuredClone(report);delete oldCpu.backend;
 const oldCpuScratch=await mkdtemp(join(validation,'reference-old-cpu-'));await run(oldCpu,true,oldCpuScratch);
}
await run({...report,case_set:'unsupported-v1'},false);
await run({...report,case_set:report.case_set==='generalization-v1'?'original-v1':'generalization-v1'},false);
await run({...report,rubric_sha256:'0'.repeat(64)},false);
if(hasSchema)for(const schema_sha256 of ['0'.repeat(64),'',null,4])
 await run({...report,prompt_reference:{...report.prompt_reference,schema_sha256}},false);
assert.equal(await stat(output).then(()=>true,()=>false),false);
await run(report,true);
const reference=await readFile(output);
const captured=JSON.parse(reference);
assert.equal(sha(JSON.stringify(hasSchema?captured.messages:captured)),report.prompt_reference.messages_sha256);
if(hasSchema)assert.equal(sha(JSON.stringify(captured.schema)),report.prompt_reference.schema_sha256);
await run(report,false);assert.equal(sha(await readFile(output)),sha(reference),'Exclusive capture cannot overwrite evidence');
await run({...report,case_set:'unsupported-v1'},false);
assert.equal(sha(await readFile(output)),sha(reference));
assert.equal(sha(await readFile(join(source,'results.json'))),sha(original),'Original evaluation remains unchanged');
if(hasSchema){
 const legacy=structuredClone(report);delete legacy.prompt_reference.schema_sha256;
 const legacyScratch=await mkdtemp(join(validation,'reference-legacy-'));
 await run(legacy,true,legacyScratch);
 const legacyMessages=JSON.parse(await readFile(join(legacyScratch,'PRIVATE-prompt-reference.json'),'utf8'));
 assert.equal(sha(JSON.stringify(legacyMessages)),report.prompt_reference.messages_sha256);
 assert.equal(await stat(join(legacyScratch,'PRIVATE-generation-reference.json')).then(()=>true,()=>false),false);
}
await writeFile(join(scratch,'verification.json'),JSON.stringify({passed:true,case_set:report.case_set||'original-v1',
 source_evaluation_unchanged:true,matching_capture:true,unknown_suite_refused:true,alternate_suite_substitution_refused:true,case_hash_substitution_refused:true,
 existing_output_preserved:true,generation_schema_checked:hasSchema,schema_substitution_refused:hasSchema,
 legacy_reference_shape_compatible:hasSchema,
 inference:false,model_downloads:false,workload_execution:false},null,2));
console.log('Interpreter reference case-set checks passed: '+scratch);
