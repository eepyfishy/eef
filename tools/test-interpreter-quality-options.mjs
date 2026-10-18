// Reject unsafe/malformed evaluation arguments before artifact/config/process work.
import assert from 'node:assert/strict';
import {spawnSync} from 'node:child_process';
import {resolve,join} from 'node:path';
const root=resolve(import.meta.dirname,'..'),harness=join(root,'tools/test-interpreter-quality.mjs');
const invalid=[null,{},['--config','outside-config.json'],['--restart','1'],
 ['--interpreter-temperature'],['--interpreter-temperature','--config'],
 ['--interpreter-temperature','NaN'],['--interpreter-temperature','0.1234'],
 ['--interpreter-temperature','1'.repeat(33)],
 ['--interpreter-temperature','1','--interpreter-temperature','0'],
 ['--interpreter-seed',42],Array(16).fill('0')];
for(const options of invalid){
 const result=spawnSync(process.execPath,[harness],{cwd:root,windowsHide:true,timeout:10000,maxBuffer:65536,
  env:{...process.env,EEF_TEST_QUALITY_SUITE:'original-v1',EEF_TEST_INTERPRETER_SAMPLING_ARGS:JSON.stringify(options)}});
 assert.equal(result.error,undefined);assert.notEqual(result.status,0);
 assert.match(result.stderr.toString(),/bounded interpreter sampling CLI arguments|Unknown\/duplicate sampling flag|explicit bounded numeric sampling values/);
}
console.log('Interpreter evaluation argument rejection passed; no model/config/process fixture started');
for(const [args,extra,pattern] of [
 [['--ollama','fixture'],{EEF_TEST_ALLOW_SHARED_OLLAMA:'0'},/Explicitly opt in/],
 [['--ollama'],{EEF_TEST_ALLOW_SHARED_OLLAMA:'1'},/Supply --ollama/],
 [['--ollama','fixture','http://example.invalid:11434'],{EEF_TEST_ALLOW_SHARED_OLLAMA:'1'},/IP-literal loopback/],
 [['--ollama','fixture','http://localhost:11434'],{EEF_TEST_ALLOW_SHARED_OLLAMA:'1'},/IP-literal loopback/],
 [['--ollama','fixture','http://0.0.0.0:11434'],{EEF_TEST_ALLOW_SHARED_OLLAMA:'1'},/IP-literal loopback/],
 [['--ollama','fixture','http://user:secret@127.0.0.1:11434'],{EEF_TEST_ALLOW_SHARED_OLLAMA:'1'},/IP-literal loopback/],
 [['--ollama','fixture','http://127.0.0.1:11434/path'],{EEF_TEST_ALLOW_SHARED_OLLAMA:'1'},/IP-literal loopback/],
 [['--ollama','fixture'],{EEF_TEST_ALLOW_SHARED_OLLAMA:'1',EEF_TEST_MODEL_FILE:'must-not-read.gguf'},/Supply --ollama/],
 [['--ollama','fixture'],{EEF_TEST_ALLOW_SHARED_OLLAMA:'1',EEF_TEST_MODEL_ID:'must-not-select'},/Supply --ollama/],
 [['--ollama','fixture'],{EEF_TEST_INFERENCE_TIMEOUT_SECONDS:'121'},/inference test budget/],
]){
 const result=spawnSync(process.execPath,[harness,...args],{cwd:root,windowsHide:true,timeout:10000,maxBuffer:65536,
  env:{...process.env,EEF_TEST_MODEL_FILE:'',EEF_TEST_MODEL_ID:'',EEF_TEST_THINKING_MODE:'inherit',EEF_TEST_INTERPRETER_SAMPLING_ARGS:'[]',...extra}});
 assert.equal(result.error,undefined);assert.notEqual(result.status,0);assert.match(result.stderr.toString(),pattern);
}
console.log('Shared-provider evaluation admission arguments passed; no provider/fixture/process work started.');
