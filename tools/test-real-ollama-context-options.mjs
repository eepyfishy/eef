// Admission guards must fail before provider/config/process work.
import assert from 'node:assert/strict';
import {spawnSync} from 'node:child_process';
import {resolve,join} from 'node:path';
const root=resolve(import.meta.dirname,'..'),script=join(root,'tools/test-real-ollama-context.mjs');
for(const [allow,args,pattern] of [
 ['0',['fixture'],/Explicitly opt in/],
 ['1',[],/Supply one installed model/],
 ['1',[' bad '],/Supply one installed model/],
 ['1',['fixture','http://example.invalid:11434'],/IP-literal loopback/],
 ['1',['fixture','http://localhost:11434'],/IP-literal loopback/],
 ['1',['fixture','http://0.0.0.0:11434'],/IP-literal loopback/],
 ['1',['fixture','https://127.0.0.1:11434'],/IP-literal loopback/],
 ['1',['fixture','http://user:secret@127.0.0.1:11434'],/IP-literal loopback/],
 ['1',['fixture','http://127.0.0.1:11434/path'],/IP-literal loopback/],
 ['1',['fixture','http://127.0.0.1:11434/?query=1'],/IP-literal loopback/],
 ['1',['fixture','http://127.0.0.1:11434','--unexpected'],/Only optional --prepare/],
]){
 const child=spawnSync(process.execPath,[script,...args],{cwd:root,windowsHide:true,timeout:10000,maxBuffer:65536,
  env:{...process.env,EEF_TEST_ALLOW_SHARED_OLLAMA:allow}});
 assert.equal(child.error,undefined);assert.notEqual(child.status,0);assert.match(child.stderr.toString(),pattern);
}
console.log('Shared-Ollama test admission guards passed; no provider request, configuration or node process started.');
