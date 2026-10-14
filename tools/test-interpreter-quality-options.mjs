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
