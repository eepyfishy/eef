// Formatting-only diagnostic against an isolated node-owned llama.cpp process.
// No coordinator, generation, model download/copy or user action execution.
import assert from 'node:assert/strict';
import {mkdtemp,readFile,writeFile,stat,realpath} from 'node:fs/promises';
import {createReadStream} from 'node:fs';
import {resolve,join,sep} from 'node:path';
import {createHash,randomBytes} from 'node:crypto';
import {spawn,spawnSync} from 'node:child_process';
import {createServer} from 'node:net';
import {caseSet} from './interpreter-generalization-cases.mjs';
assert.equal(process.argv.length,4,'Supply the runtime bundle and a completed quality fixture');
assert(process.env.EEF_TEST_MODEL_FILE,'Select an existing verified artifact; no downloads supported');
const root=resolve(import.meta.dirname,'..'),bundle=await realpath(process.argv[2]);
const bin=await realpath(process.env.EEF_TEST_BINARY_DIR||join(root,'target/debug'));
const source=await realpath(process.argv[3]),validation=await realpath(join(root,'.validation'));
assert(source.startsWith(validation+sep));
const sourceBytes=await readFile(join(source,'results.json')),sourceReport=JSON.parse(sourceBytes);
assert(sourceReport.schema_version===1&&sourceReport.rubric_version===1&&sourceReport.harness_completed===true
 &&sourceReport.real_cpu_interpretation===true&&sourceReport.api_only===true&&sourceReport.coordinator_started===false
 &&sourceReport.model_downloads===false&&sourceReport.model_copies===false);
const sha=bytes=>createHash('sha256').update(bytes).digest('hex');
async function fileSha(path){const hash=createHash('sha256');for await(const b of createReadStream(path))hash.update(b);return hash.digest('hex');}
const {cases}=caseSet(sourceReport.case_set);
assert.equal(sha(JSON.stringify(cases)),sourceReport.rubric_sha256);
const model=await realpath(process.env.EEF_TEST_MODEL_FILE),runtime=join(bundle,'tools/llama-server.exe');
assert.equal((await stat(model)).size,sourceReport.bytes);assert.equal(await fileSha(model),sourceReport.sha256);
assert.equal(await fileSha(runtime),sourceReport.runtime_sha256,'Runtime differs from evaluated artifact');
const helper=join(bin,'examples/interpretation_fixture.exe');
const env={...process.env,PATH:join(root,'.tooling/llvm-mingw-20260616-ucrt-x86_64/bin')+';'+process.env.PATH};
const referenceChild=spawnSync(helper,[],{cwd:bundle,windowsHide:true,timeout:10000,maxBuffer:131072,
 env:{...env,EEF_INTERPRETATION_FIXTURE:'1'},input:JSON.stringify({mode:'generation_reference',input:cases[0].input,
 allowed_capabilities:sourceReport.allowed_capabilities})});
assert.equal(referenceChild.error,undefined);assert.equal(referenceChild.status,0,'Shared reference helper failed');
const {success,reference}=JSON.parse(referenceChild.stdout);assert.equal(success,true);
assert.equal(sha(reference.messages[0].content),sourceReport.prompt_reference.sha256);
assert.equal(sha(JSON.stringify(reference.messages)),sourceReport.prompt_reference.messages_sha256);
assert.equal(reference.messages.at(-1).content,cases[0].input);
const scratch=await mkdtemp(join(validation,'interpreter-template-'));
async function freePort(){const socket=createServer();await new Promise(r=>socket.listen(0,'127.0.0.1',r));
 const port=socket.address().port;await new Promise(r=>socket.close(r));return port;}
const [port,apiPort,unused]=await Promise.all([freePort(),freePort(),freePort()]);
const base=`http://127.0.0.1:${port}`,node=`http://127.0.0.1:${apiPort}`,configPath=join(scratch,'node.json');
const config=JSON.parse(await readFile(join(root,'config/node.example.json'),'utf8'));
Object.assign(config,{node_id:'interpreter-template-fixture',name:'Formatting diagnostic',connection_enabled:false,
 auto_local:false,local_pairing:false,psk:randomBytes(32).toString('hex'),endpoints:[`127.0.0.1:${unused}`],
 update:{policy:'off'},dashboard:{enabled:true,ui_enabled:false,host:'127.0.0.1',port:apiPort},
 models:{provider:'llamacpp',ollama:{base_url:`http://127.0.0.1:${unused}`,selected:[]},
  llamacpp:{binary:runtime,slots:[{model_id:sourceReport.candidate,model_path:model,port,gpu_layers:0,context:4096,cpu_threads:2}]}}});
await writeFile(configPath,JSON.stringify(config));
let backend,out='',err='',spawnError;const requests=[];
async function boundedJson(response){
 let size=0;const chunks=[];
 for await(const chunk of response.body){size+=chunk.length;assert(size<=2*1024*1024,'Bounded diagnostic response');chunks.push(chunk);}
 assert(response.ok,'Backend diagnostic request failed');return JSON.parse(Buffer.concat(chunks));
}
async function request(path,body){
 assert(['/health','/props','/slots','/apply-template'].includes(path),'Only formatting/read-only routes permitted');
 assert((path==='/apply-template')===(body!==undefined),'Unexpected diagnostic method');
 requests.push({path,method:body===undefined?'GET':'POST'});
 const response=await fetch(base+path,{method:body===undefined?'GET':'POST',redirect:'error',
  headers:{'Content-Type':'application/json'},body:body===undefined?undefined:JSON.stringify(body),signal:AbortSignal.timeout(5000)});
 return boundedJson(response);
}
const report={schema_version:1,passed:false,candidate:sourceReport.candidate,sha256:sourceReport.sha256,
 bytes:sourceReport.bytes,runtime_sha256:sourceReport.runtime_sha256,helper_sha256:await fileSha(helper),
 reference_system_sha256:sourceReport.prompt_reference.sha256,reference_messages_sha256:sourceReport.prompt_reference.messages_sha256,
 node_binary_sha256:await fileSha(join(bin,'eefn.exe')),
 physical_two_pc:false,model_downloads:false,model_copies:false,node_started:false,coordinator_started:false,
 inference:false,workload_execution:false,remote_attestation:false,payload_reference_not_wire_capture:true,
 context_tokens:4096,gpu_layers:0,cpu_threads:2,variants:[]};
try{
 // Reuse the node's native model-process ownership, including Windows Job cleanup.
 backend=spawn(join(bin,'eefn.exe'),['--config',configPath,'--no-ui'],{cwd:bundle,
  env:{...env,EEF_NODE_PSK:'',APPDATA:join(scratch,'appdata'),EEF_DISCOVERY_DIR:join(scratch,'discovery')},
  windowsHide:true,stdio:['ignore','pipe','pipe']});
 report.node_started=Number.isInteger(backend.pid)&&backend.pid>0;
 backend.stdout.on('data',b=>out=(out+b).slice(-16384));backend.stderr.on('data',b=>err=(err+b).slice(-16384));
 backend.result=new Promise(r=>{backend.on('error',error=>{spawnError=String(error);r({spawn_error:spawnError});});backend.on('close',code=>r({code}));});
 const deadline=Date.now()+120000;let ready=false;
 while(Date.now()<deadline){assert.equal(spawnError,undefined,'Owned node spawn failed');assert.equal(backend.exitCode,null,'Owned node stopped during readiness');
  try{if((await request('/health')).status==='ok'){ready=true;break;}}catch{}
  await new Promise(r=>setTimeout(r,200));}
 assert(ready,'Formatting runtime readiness deadline expired; no restart attempted');
 const status=await boundedJson(await fetch(node+'/api/status',{redirect:'error',signal:AbortSignal.timeout(5000)}));
 const diagnostics=await boundedJson(await fetch(node+'/api/diagnostics',{redirect:'error',signal:AbortSignal.timeout(5000)}));
 assert.equal(status.connection.state,'paused');assert.equal(diagnostics.successful_connections,0);
 assert.equal(status.last_workload,null);assert.equal(status.download.state,'idle');
 const configBefore=await readFile(configPath);
 report.node_connection_paused=true;
 const props=await request('/props');assert.equal(await realpath(props.model_path),model,'Unexpected backend artifact');
 report.build_info=props.build_info;report.chat_template_caps=props.chat_template_caps;report.modalities=props.modalities;
 await writeFile(join(scratch,'PRIVATE-template-properties.json'),JSON.stringify(props,null,2),{flag:'wx'});
 for(const mode of ['inherit','disabled','enabled']){
  const payload={messages:reference.messages,temperature:0,max_tokens:512,stream:false,
   response_format:{type:'json_object',schema:reference.schema}};
  if(mode!=='inherit'){payload.chat_template_kwargs={enable_thinking:mode==='enabled'};
   if(mode==='disabled')payload.reasoning_effort='none';}
  const formatted=await request('/apply-template',payload);assert.equal(typeof formatted.prompt,'string');
  assert(Buffer.byteLength(formatted.prompt)<=131072);
  const systemIndex=formatted.prompt.indexOf(reference.messages[0].content);
  const userIndex=formatted.prompt.indexOf(cases[0].input);assert(systemIndex>=0&&userIndex>systemIndex,'Reference text not preserved in order');
  await writeFile(join(scratch,`PRIVATE-${mode}-template.json`),JSON.stringify({constructed_request:payload,prompt:formatted.prompt},null,2),{flag:'wx'});
  report.variants.push({mode,constructed_request_sha256:sha(JSON.stringify(payload)),rendered_prompt_sha256:sha(formatted.prompt),
   rendered_bytes:Buffer.byteLength(formatted.prompt),system_text_preserved:true,user_text_preserved:true,reference_order_preserved:true});
 }
 const slots=await request('/slots');assert(Array.isArray(slots));assert(slots.every(s=>s.is_processing===false));
 const final=await boundedJson(await fetch(node+'/api/status',{redirect:'error',signal:AbortSignal.timeout(5000)}));
 assert.equal(final.connection.state,'paused');assert.equal(final.last_workload,null);assert.equal(final.download.state,'idle');
 assert.equal(sha(await readFile(configPath)),sha(configBefore),'Formatting cannot mutate saved node config');
 report.disabled_changes_rendering=report.variants[0].rendered_prompt_sha256!==report.variants[1].rendered_prompt_sha256;
 report.enabled_changes_rendering=report.variants[1].rendered_prompt_sha256!==report.variants[2].rendered_prompt_sha256;
 assert.equal(sha(await readFile(join(source,'results.json'))),sha(sourceBytes),'Source evaluation must remain unchanged');
 report.passed=true;
}catch(error){report.error=String(error);await writeFile(join(scratch,'PRIVATE-runtime-log.json'),JSON.stringify({out,err},null,2));throw error;
}finally{
 // Stop only the owned node. Its native ownership closes the selected backend.
 if(backend?.pid&&backend.exitCode===null)backend.kill();
 if(backend){let timeout;
  const result=await Promise.race([backend.result,new Promise(r=>{timeout=setTimeout(()=>r({timeout:true}),10000);})]);clearTimeout(timeout);
  report.node_stop_confirmed=!result.timeout;
  try{await fetch(base+'/health',{redirect:'error',signal:AbortSignal.timeout(2000)});report.backend_unreachable_after_stop=false;}
  catch{report.backend_unreachable_after_stop=true;}
  if(!report.node_stop_confirmed||!report.backend_unreachable_after_stop){report.passed=false;report.cleanup_error='Inspect owned processes; cleanup not confirmed';}
 }
 report.requests=requests;await writeFile(join(scratch,'results.json'),JSON.stringify(report,null,2));
 console.log('Formatting-only interpreter diagnostic: '+scratch);
 if(report.cleanup_error)throw Error(report.cleanup_error);
}
