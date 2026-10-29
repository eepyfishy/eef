// Existing trusted interpreter; fake device API only. No device package needed.
import assert from 'node:assert/strict';
import {mkdtemp,mkdir,readFile,writeFile,stat} from 'node:fs/promises';
import {resolve,join} from 'node:path';
import {spawn} from 'node:child_process';
import {createHash} from 'node:crypto';
const root=resolve(import.meta.dirname,'..'),python=process.env.EEF_TEST_PYTHON;
assert(python,'Supply an existing trusted interpreter; no download is attempted');await stat(python);
await mkdir(join(root,'.validation'),{recursive:true});const scratch=await mkdtemp(join(root,'.validation/microphone-energy-'));
const paths=[python,join(root,'python/streams/microphone_energy.py'),join(root,'python/tests/test_microphone_energy.py'),import.meta.filename];
const inputs=await Promise.all(paths.map(async path=>({path,sha256:createHash('sha256').update(await readFile(path)).digest('hex')})));
await writeFile(join(scratch,'inputs.json'),JSON.stringify(inputs,null,2),{flag:'wx'});
const child=spawn(python,['-I','-B','-X','utf8','python/tests/test_microphone_energy.py'],{cwd:root,windowsHide:true,stdio:['ignore','pipe','pipe']});let stdout='',stderr='';
child.stdout.on('data',b=>stdout+=b);child.stderr.on('data',b=>stderr+=b);
const timer=setTimeout(()=>child.kill(),10000);
let code;try{code=await new Promise((r,j)=>{child.once('error',j);child.once('close',r);});}finally{clearTimeout(timer);}
await writeFile(join(scratch,'process-result.json'),JSON.stringify({code,stdout,stderr},null,2),{flag:'wx'});
assert.equal(code,0,'Fake-device tests failed; retain '+scratch);assert.match(stderr,/Ran 11 tests/);assert.match(stderr,/\nOK\s*$/);
assert.equal(await stat(join(root,'python/streams/__pycache__')).then(()=>true,()=>false),false);
await writeFile(join(scratch,'results.json'),JSON.stringify({passed:true,tests:11,real_interpreter:true,device_api_fake:true,single_open_and_close_tested:true,gap_uncertainty_and_hysteresis_tested:true,parameters_and_block_sizes_bounded:true,raw_media_emitted:false,physical_capture:false,workload_admission_integrated:false,ambient_capture_enabled:false,continuous_workloads_accepted:false,stable_release_accepted:false},null,2),{flag:'wx'});
console.log('Microphone energy fake-device validation passed: '+scratch);
