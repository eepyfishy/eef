// Opt-in production-node CPU proposal evaluation. Reuse an existing exact
// catalog artifact: no download/copy, coordinator, user data, workload or UI.
import assert from 'node:assert/strict';
import {mkdtemp,readFile,writeFile,stat} from 'node:fs/promises';
import {createReadStream} from 'node:fs';
import {createHash,randomBytes} from 'node:crypto';
import {spawn,execFile} from 'node:child_process';
import {promisify} from 'node:util';
import {createServer} from 'node:net';
import {resolve,join} from 'node:path';
import {cpus,totalmem} from 'node:os';
import {cases,grade,rubricVersion} from './interpreter-quality-cases.mjs';

assert(process.argv[2]&&process.env.EEF_TEST_MODEL_FILE,
 'Pass the runtime bundle and set EEF_TEST_MODEL_FILE to an existing catalog artifact. No downloads supported.');
const root=resolve(import.meta.dirname,'..'),bundle=resolve(process.argv[2]);
const bin=resolve(process.env.EEF_TEST_BINARY_DIR||join(root,'target/debug'));
const thinkingMode=process.env.EEF_TEST_THINKING_MODE||'inherit';
assert(['inherit','disabled','enabled'].includes(thinkingMode),'Invalid EEF_TEST_THINKING_MODE');
const model=resolve(process.env.EEF_TEST_MODEL_FILE),runtime=join(bundle,'tools','llama-server.exe');
const catalog=JSON.parse(await readFile(join(root,'config/model-catalog.json'),'utf8'));
const matches=catalog.filter(m=>m.id===(process.env.EEF_TEST_MODEL_ID||catalog[0]?.id));
assert.equal(matches.length,1,'Choose one exact catalog candidate');const entry=matches[0];
async function digest(path){const h=createHash('sha256');for await(const b of createReadStream(path))h.update(b);return h.digest('hex');}
assert.equal((await stat(model)).size,entry.bytes);assert.equal(await digest(model),entry.sha256,'Candidate hash mismatch');
const runtimeHash=await digest(runtime),nodeHash=await digest(join(bin,'eefn.exe'));
const scratch=await mkdtemp(join(root,'.validation/interpreter-quality-')),configPath=join(scratch,'node.json');
const children=[],nodeId='interpreter-quality-node';let node='',runtimeId='';
const env={...process.env,EEF_NODE_PSK:'',APPDATA:join(scratch,'appdata'),EEF_DISCOVERY_DIR:join(scratch,'discovery'),
 PATH:join(root,'.tooling/llvm-mingw-20260616-ucrt-x86_64/bin')+';'+process.env.PATH};
function launch(args){
 const child=spawn(join(bin,'eefn.exe'),['--config',configPath,...args],{cwd:bundle,windowsHide:true,env,stdio:['ignore','pipe','pipe']});
 let out='',err='';child.stdout.on('data',b=>out=(out+b).slice(-1048576));child.stderr.on('data',b=>err=(err+b).slice(-1048576));
 child.result=new Promise((r,j)=>{child.on('error',j);child.on('close',code=>r({code,out,err}));});children.push(child);return child;
}
async function cli(args){const child=launch(['--json',...args]),timer=setTimeout(()=>child.kill(),35000);
 try{const r=await child.result;assert([0,1].includes(r.code),r.out+r.err);const value=JSON.parse(r.out);
  assert.equal(r.code,value.success===true?0:1,r.out+r.err);return value;
 }finally{clearTimeout(timer);}}
async function port(){const s=createServer();await new Promise(r=>s.listen(0,'127.0.0.1',r));const p=s.address().port;await new Promise(r=>s.close(r));return p;}
async function json(path){const r=await fetch(node+path,{signal:AbortSignal.timeout(5000)});assert(r.ok);return r.json();}
async function until(fn,label){const end=Date.now()+120000;while(Date.now()<end){try{if(await fn())return;}catch{}await new Promise(r=>setTimeout(r,200));}throw Error('Timeout: '+label);}
const exec=promisify(execFile);
async function referencePrompt(capabilities){
 // Same shared Rust function, emitted by the test-only executable. This is a
 // reproducibility reference, not interception/attestation of a remote model.
 const path=join(bin,'examples','interpretation_fixture.exe');
 const child=spawn(path,[],{cwd:bundle,windowsHide:true,env:{...env,EEF_INTERPRETATION_FIXTURE:'1'},stdio:['pipe','pipe','pipe']});
 let out='';child.stdout.on('data',b=>out=(out+b).slice(-65536));child.stderr.resume();
 child.result=new Promise((r,j)=>{child.on('error',j);child.on('close',code=>r(code));});children.push(child);
 child.stdin.on('error',()=>{});child.stdin.end(JSON.stringify({mode:'prompt',input:cases[0].input,allowed_capabilities:capabilities}));
 const timer=setTimeout(()=>child.kill(),10000);
 try{
  assert.equal(await child.result,0,'shared prompt fixture failed');const value=JSON.parse(out);assert.equal(value.success,true);
  assert.equal(value.messages.at(-1).content,cases[0].input);assert.equal(value.messages[0].role,'system');
  await writeFile(join(scratch,'PRIVATE-prompt-reference.json'),JSON.stringify(value.messages,null,2),{flag:'wx'});
  return {sha256:createHash('sha256').update(value.messages[0].content).digest('hex'),
   messages_sha256:createHash('sha256').update(JSON.stringify(value.messages)).digest('hex'),message_count:value.messages.length,
   fixture_sha256:await digest(path)};
 }finally{clearTimeout(timer);}
}
async function sample(nodeProcess){
 if(process.platform!=='win32')return null;
 assert(Number.isInteger(nodeProcess.pid)&&nodeProcess.pid>0);
 try{
  const {stdout}=await exec('powershell.exe',['-NoProfile','-NonInteractive','-Command',
   `$qualityChildren=@(Get-CimInstance Win32_Process -Filter "ParentProcessId=${nodeProcess.pid}" | Where-Object { $_.Name -eq 'llama-server.exe' }); if ($qualityChildren.Count -eq 1) { $qualityModel=Get-Process -Id $qualityChildren[0].ProcessId -ErrorAction Stop; @{peak_working_set_bytes=$qualityModel.PeakWorkingSet64;process_cpu_seconds=$qualityModel.TotalProcessorTime.TotalSeconds}|ConvertTo-Json -Compress }`],
   {windowsHide:true,timeout:10000,maxBuffer:65536});
  return stdout.trim()?JSON.parse(stdout):null;
 }catch{return null;}
}
const report={schema_version:1,rubric_version:rubricVersion,candidate:entry.id,bytes:entry.bytes,sha256:entry.sha256,
 rubric_sha256:createHash('sha256').update(JSON.stringify(cases)).digest('hex'),
 runtime_sha256:runtimeHash,node_binary_sha256:nodeHash,real_cpu_interpretation:true,model_downloads:false,model_copies:false,
 api_only:true,coordinator_started:false,physical_two_pc:false,context_tokens:4096,gpu_layers:0,output_token_limit:512,thinking_mode:thinkingMode,
 cpu:cpus()[0]?.model,logical_cpus:cpus().length,system_memory_bytes:totalmem(),threads:null,
 startup_ms:null,model_process_sample:null,cases:[],harness_completed:false,default_approved:false,
 scope:'Production node local preview with fixed synthetic inputs. No model-quality approval, execution, bootstrap, memory limit or physical-network acceptance.'};
let owner;
try{
 const config=JSON.parse(await readFile(join(root,'config/node.example.json'),'utf8'));
 const [apiPort,modelPort,unused]=await Promise.all([port(),port(),port()]);
 Object.assign(config,{node_id:nodeId,name:'Interpreter quality fixture',auto_local:false,local_pairing:false,
  psk:randomBytes(32).toString('hex'),endpoints:[`127.0.0.1:${unused}`],connection_enabled:false,
  update:{policy:'off'},dashboard:{enabled:true,ui_enabled:false,host:'127.0.0.1',port:apiPort},
  models:{provider:'llamacpp',ollama:{base_url:`http://127.0.0.1:${unused}`,selected:[]},
   llamacpp:{binary:runtime,slots:[{model_id:entry.id,model_path:model,port:modelPort,gpu_layers:0,context:4096}]}}});
 await writeFile(configPath,JSON.stringify(config));
 // Use the real owner command rather than manufacturing applied role metadata.
 const assigned=await cli(['models','interpreter','--backend','llamacpp','--model',entry.id]);
 assert.equal(assigned.success,true);assert.equal(assigned.running,false);
 assert.equal(assigned.saved_selections[0].model_metadata.roles[0],'request_interpreter');
 const beforePolicy=(await cli(['model-lifecycle-policy','show'])).result;
 const savedPolicy=(await cli(['model-lifecycle-policy','set','--expected-digest',beforePolicy.saved_digest,'--thinking-mode',thinkingMode])).result;
 assert.equal(savedPolicy.saved_policy.thinking_mode||'inherit',thinkingMode);
 assert.equal(savedPolicy.applied_policy,null);assert.equal(savedPolicy.restart_requested,false);
 node=`http://127.0.0.1:${apiPort}`;const started=performance.now();owner=launch(['--no-ui']);
 await until(async()=>{const s=await json('/api/status');return s.connection.state==='paused'&&s.models.some(m=>m.model_id===entry.id&&m.model_metadata.lifecycle==='ready');},'isolated CPU model ready');
 report.startup_ms=Math.round(performance.now()-started);runtimeId=(await json('/api/diagnostics')).runtime_id;
 const appliedPolicy=(await cli(['model-lifecycle-policy','show'])).result;
 assert.equal(appliedPolicy.applied_policy.thinking_mode||'inherit',thinkingMode);
 report.thinking_hint_saved_applied_checked=true;
 report.allowed_capabilities=(await json('/api/status')).capabilities;report.node_version=(await json('/api/status')).version;
 report.prompt_reference=await referencePrompt(report.allowed_capabilities);
 assert.equal((await fetch(node+'/')).status,404);const saved=await readFile(configPath,'utf8');
 for(const item of cases){
  const begin=performance.now(),value=await cli(['requests','preview','--text',item.input]);
  assert.equal(value.report_type,'request_preview');assert.equal(value.schema_version,1);assert.equal(value.node_id,nodeId);
  assert.equal(value.runtime_id,runtimeId);assert.equal(value.execution_authorized,false);assert.equal(value.dispatched,false);
  assert.equal(value.automatically_retried,false);assert.equal(value.interpretation_context,'local_node');
  if(value.success){
   assert.equal(value.result.model.backend,'llamacpp');assert.equal(value.result.model.model_id,entry.id);
   assert.equal(value.result.interpretation.original_text,item.input);assert.equal(value.result.interpretation.execution_authorized,false);
   assert.equal(value.result.interpretation.model_output_is_untrusted,true);
  }else{assert.equal(typeof value.error_code,'string');}
  const score=grade(item,value);report.cases.push({id:item.id,input:item.input,
   expected:{intents:item.intents,workloads:item.workloads,clarification:item.clarification,capabilities:item.capabilities||[]},
   ...score,elapsed_ms:Math.round(performance.now()-begin),reply:value});
  console.log(`${item.id}: ${score.matched?'match':'MISMATCH'} (${score.reasons.join(', ')||'accepted'})`);
 }
 const final=await json('/api/status'),diagnostics=await json('/api/diagnostics');
 assert.equal(final.connection.state,'paused');assert.equal(diagnostics.successful_connections,0);
 assert.equal(final.last_workload,null);assert.equal(final.download.state,'idle');assert.equal(await readFile(configPath,'utf8'),saved);
 report.model_process_sample=await sample(owner);report.harness_completed=true;
}catch(error){report.error=error.message;process.exitCode=1;}
finally{
 // Stop only the test's owned process trees, including its owned model runtime.
 for(const child of children)if(child.exitCode===null){
  if(child.pid===undefined)continue; // Spawn failed: there is no owned process to stop.
  assert(Number.isInteger(child.pid)&&child.pid>0);
  if(process.platform==='win32')await exec('taskkill.exe',['/PID',String(child.pid),'/T','/F'],{windowsHide:true,timeout:10000}).catch(()=>child.kill());
  else child.kill();
 }
 await Promise.allSettled(children.map(c=>c.result));
 report.accepted_count=report.cases.filter(c=>c.reply.success===true).length;
 report.matched_count=report.cases.filter(c=>c.matched).length;
 await writeFile(join(scratch,'results.json'),JSON.stringify(report,null,2));
 console.log(`Production interpreter evaluation: ${scratch}`);
 console.log(JSON.stringify({harness_completed:report.harness_completed,accepted:report.accepted_count,matched:report.matched_count,total:cases.length,default_approved:false,error:report.error}));
}
