// Capture a matching test-only prompt reference, never infer or execute work.
import assert from 'node:assert/strict';
import {readFile,writeFile,realpath} from 'node:fs/promises';
import {createReadStream} from 'node:fs';
import {createHash} from 'node:crypto';
import {spawnSync} from 'node:child_process';
import {resolve,join,sep} from 'node:path';
import {caseSet} from './interpreter-generalization-cases.mjs';

assert(process.argv.length===4,'Supply the binary directory and existing quality fixture directory.');
const root=resolve(import.meta.dirname,'..'),bin=await realpath(resolve(process.argv[2]));
const fixture=await realpath(resolve(process.argv[3])),validation=await realpath(join(root,'.validation'));
assert(fixture.startsWith(validation+sep),'Reference output must stay in an existing isolated validation fixture.');
const report=JSON.parse(await readFile(join(fixture,'results.json'),'utf8'));
assert(report.schema_version===1&&report.rubric_version===1&&report.harness_completed===true&&report.api_only===true
 &&report.coordinator_started===false&&report.real_cpu_interpretation===true&&report.model_downloads===false&&report.model_copies===false);
const sha=bytes=>createHash('sha256').update(bytes).digest('hex');
const {cases}=caseSet(report.case_set);
assert.equal(sha(JSON.stringify(cases)),report.rubric_sha256,'Case set differs from evaluation.');
async function fileSha(path){const hash=createHash('sha256');for await(const bytes of createReadStream(path))hash.update(bytes);return hash.digest('hex');}
assert.equal(await fileSha(join(bin,'eefn.exe')),report.node_binary_sha256,'Node binary differs from evaluation.');
const helper=join(bin,'examples','interpretation_fixture.exe');
assert.equal(await fileSha(helper),report.prompt_reference.fixture_sha256,'Reference helper differs from evaluation.');
const child=spawnSync(helper,[],{cwd:root,windowsHide:true,timeout:10000,maxBuffer:131072,
 env:{...process.env,EEF_INTERPRETATION_FIXTURE:'1',PATH:join(root,'.tooling/llvm-mingw-20260616-ucrt-x86_64/bin')+';'+process.env.PATH},
 input:JSON.stringify({mode:'prompt',input:cases[0].input,allowed_capabilities:report.allowed_capabilities})});
assert.equal(child.status,0,'Test-only reference helper failed.');
const value=JSON.parse(child.stdout);assert.equal(value.success,true);
assert.equal(value.messages[0].role,'system');assert.equal(value.messages.at(-1).content,cases[0].input);
assert.equal(sha(value.messages[0].content),report.prompt_reference.sha256);
assert.equal(sha(JSON.stringify(value.messages)),report.prompt_reference.messages_sha256,'Full reference messages differ.');
await writeFile(join(fixture,'PRIVATE-prompt-reference.json'),JSON.stringify(value.messages,null,2),{flag:'wx'});
console.log('Matching private prompt reference captured; no inference, model download or workload.');
