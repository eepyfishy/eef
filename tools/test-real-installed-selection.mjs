// Opt-in integrity/selection test against an existing development cache only.
// Adds a uniquely named test config beside that cache so no weights are copied.
// Never alter pre-existing configs/index or infer/execute. Runtime loading is
// separately opt-in with EEF_TEST_ACTIVATE_BACKEND=1 and EEF_TEST_RUNTIME_BUNDLE.
import assert from 'node:assert/strict';
import {readFile,writeFile,realpath,mkdtemp,stat} from 'node:fs/promises';
import {resolve,join,dirname,sep} from 'node:path';
import {spawn} from 'node:child_process';
import {randomUUID,createHash} from 'node:crypto';
import {createServer as tcpServer} from 'node:net';
import {createServer} from 'node:http';
const root=resolve(import.meta.dirname,'..'),bin=resolve(process.env.EEF_TEST_BINARY_DIR||join(root,'target/debug'));
assert(process.argv[2]&&process.argv[3],'Pass an existing ignored development models directory and catalog ID explicitly');
const validation=await realpath(join(root,'.validation')),directory=await realpath(process.argv[2]),id=process.argv[3];
assert(directory.startsWith(validation+sep)&&directory.endsWith(sep+'models'),'Use only an existing controlled development cache');
const indexPath=join(directory,'installed.json'),indexBefore=await readFile(indexPath);
const index=JSON.parse(indexBefore),catalog=JSON.parse(await readFile(join(root,'config/model-catalog.json'),'utf8'));
assert.equal(index.filter(m=>m.id===id).length,1);assert.equal(catalog.filter(m=>m.id===id).length,1);
const entry=catalog.find(m=>m.id===id),artifact=join(directory,entry.sha256.toLowerCase()+'.gguf');
assert.equal((await stat(artifact)).size,entry.bytes);
const scratch=await mkdtemp(join(validation,'real-installed-selection-'));
const activateBackend=process.env.EEF_TEST_ACTIVATE_BACKEND==='1';
const runtimeBundle=process.env.EEF_TEST_RUNTIME_BUNDLE&&await realpath(process.env.EEF_TEST_RUNTIME_BUNDLE);
assert(!runtimeBundle||activateBackend,'Real startup requires explicit backend activation opt-in');
async function port(){const s=tcpServer();await new Promise(r=>s.listen(0,'127.0.0.1',r));const p=s.address().port;await new Promise(r=>s.close(r));return p;}
const provider=runtimeBundle?createServer((req,res)=>{res.setHeader('Content-Type','application/json');
 if(req.url==='/api/tags')return res.end(JSON.stringify({models:[]}));res.writeHead(404);res.end('{}');}):null;
if(provider)await new Promise(r=>provider.listen(0,'127.0.0.1',r));
const configPath=join(dirname(directory),'PRIVATE-installed-selection-'+randomUUID()+'.json');
const config=JSON.parse(await readFile(join(root,'config/node.example.json'),'utf8'));
Object.assign(config,{node_id:'real-installed-selection-'+randomUUID(),name:'Real installed selection fixture',
 auto_local:false,local_pairing:false,endpoints:[],connection_enabled:false,update:{policy:'off'},
 dashboard:{...config.dashboard,enabled:!!runtimeBundle,ui_enabled:false,port:runtimeBundle?await port():config.dashboard.port}});
if(runtimeBundle){
 const binary=join(runtimeBundle,'tools','llama-server.exe');assert((await stat(binary)).isFile());
 config.models.llamacpp.binary=binary;config.models.ollama.base_url=`http://127.0.0.1:${provider.address().port}`;
}
await writeFile(configPath,JSON.stringify(config),{flag:'wx'});
const env={...process.env,EEF_NODE_PSK:'',APPDATA:join(scratch,'appdata'),EEF_DISCOVERY_DIR:join(scratch,'discovery'),
 PATH:join(root,'.tooling/llvm-mingw-20260616-ucrt-x86_64/bin')+';'+process.env.PATH};
const children=[];
function launch(args){const c=spawn(join(bin,'eefn.exe'),['--config',configPath,...args],
 {cwd:root,windowsHide:true,env,stdio:['ignore','pipe','pipe']});let out='',err='';
 c.stdout.on('data',b=>out=(out+b).slice(-1048576));c.stderr.on('data',b=>err=(err+b).slice(-1048576));
 c.logs=()=>({out,err});c.result=new Promise((r,j)=>{c.on('error',j);c.on('close',code=>r({code,out,err}));});children.push(c);return c;}
async function cli(args){const c=launch(['--json',...args]),timer=setTimeout(()=>c.kill(),85000);
 try{const r=await c.result;assert.equal(r.code,0,r.out+r.err);return JSON.parse(r.out);}finally{clearTimeout(timer);}}
async function until(fn,label,timeout=180000){const end=Date.now()+timeout;while(Date.now()<end){if(await fn())return;await new Promise(r=>setTimeout(r,500));}throw Error('Timeout: '+label);}
function alive(pid){try{process.kill(pid,0);return true;}catch(error){if(error.code==='ESRCH')return false;throw error;}}
try{
 const started=performance.now();
 const selected=await cli(['models','select-installed-gguf','--model',id,'--interpreter',...(activateBackend?['--activate-backend']:[])]);
 assert.equal(selected.running,false);assert.equal(selected.changed,true);
 assert.deepEqual(selected.installed_artifact,{model_id:id,sha256:entry.sha256.toLowerCase(),bytes:entry.bytes,verified_at_selection:true});
 assert.equal(selected.saved_selections.length,1);assert.equal(selected.saved_selections[0].model_id,id);
 assert(selected.saved_selections[0].model_metadata.roles.includes('request_interpreter'));
 assert.equal(selected.registered_models,null);
 const saved=JSON.parse(await readFile(configPath,'utf8'));
 assert.equal(saved.models.provider,activateBackend?'llamacpp':config.models.provider);assert.deepEqual(saved.permissions,config.permissions);
 assert.deepEqual(await readFile(indexPath),indexBefore,'selection must not rewrite the existing installed index');
 assert.equal((await stat(artifact)).size,entry.bytes);
 const elapsed=Math.round(performance.now()-started);
 let startup;
 if(runtimeBundle){
  // Reachable Ollama remains available, but explicit saved choice must start GGUF.
  assert.equal((await fetch(config.models.ollama.base_url+'/api/tags')).status,200);
  await cli(['models','cpu-threads','--model',id,'--threads','2']);
  const owner=launch(['--no-ui']),runtimeStarted=performance.now();let address;
  await until(async()=>{assert.equal(owner.exitCode,null,'isolated node exited before readiness');
   try{address='http://'+JSON.parse(await readFile(configPath+'.api.json','utf8')).address;
    const r=await fetch(address+'/api/diagnostics',{signal:AbortSignal.timeout(2000)}),d=await r.json();
    return r.ok&&d.node_id===config.node_id&&!!d.runtime_id;
   }catch{return false;}},'owner API publication',30000);
  const initialSetup=await cli(['setup']);assert.equal(initialSetup.applied_interpreter.state,'selected');
  let ready;
  await until(async()=>{const report=(await cli(['model-lifecycle','show'])).result;
   const model=report.models.find(m=>m.model_id===id);
   assert.notEqual(model?.state,'error','real model startup failed');
   if(model?.state==='ready'){ready={report,model};return true;}return false;},'real selected CPU model ready');
  const setup=await cli(['setup']);assert.equal(setup.saved_matches_applied,true);
  assert.deepEqual(setup.applied_interpreter,{state:'selected',model:{backend:'llamacpp',model_id:id}});
  assert.equal(setup.connection,'paused');assert.equal(setup.execution_authorized,false);
  assert(ready.model.owned_pid>0);assert.equal((await cli(['models','show'])).provider,'llamacpp');
  const startupMs=Math.round(performance.now()-runtimeStarted),unloadId=randomUUID();
  await cli(['model-lifecycle','--runtime',ready.report.runtime_id,'unload','--model',id,'--generation',String(ready.model.generation),'--operation-id',unloadId]);
  await until(async()=>{const report=(await cli(['model-lifecycle','show','--operation-id',unloadId])).result;
   const model=report.models.find(m=>m.model_id===id);return report.receipt?.phase==='completed'&&model?.state==='unloaded'&&model.owned_pid===null;},'explicit native unload',30000);
  await until(()=>!alive(ready.model.owned_pid),'owned native PID exit',10000);
  startup={cpu_ready:true,startup_ms:startupMs,applied_interpreter_verified:true,reachable_ollama_not_selected:true,
   owned_pid_observed:true,explicit_unload_completed:true,owned_pid_exit_observed:true,job_accounting:ready.model.job_accounting??null};
 }
 await writeFile(join(scratch,'results.json'),JSON.stringify({passed:true,model:id,bytes:entry.bytes,sha256:entry.sha256,
  real_cached_artifact:true,selection_elapsed_ms:elapsed,selection_offline_no_runtime:true,explicit_interpreter_assignment:true,
  explicit_backend_activation:activateBackend,startup:startup??null,preexisting_index_unchanged:true,model_downloads:false,model_copies:false,
  real_model_loading:!!runtimeBundle,model_inference:false,model_quality_approved:false,
  physical_two_pc:false,isolated_config_sha256:createHash('sha256').update(await readFile(configPath)).digest('hex')},null,2));
 console.log('Real installed selection passed in '+elapsed+' ms: '+scratch);
}catch(error){await writeFile(join(scratch,'PRIVATE-failure.json'),JSON.stringify({error:String(error),configPath,
 children:children.map(c=>({pid:c.pid,exit_code:c.exitCode,...c.logs()}))},null,2));throw error;}
finally{
 for(const c of children)if(c.exitCode===null){
  if(process.platform==='win32')await new Promise(r=>{const stop=spawn('taskkill.exe',['/PID',String(c.pid),'/T','/F'],{windowsHide:true,stdio:'ignore'});stop.on('exit',r);stop.on('error',r);});else c.kill();
 }
 await Promise.allSettled(children.map(c=>c.result));
 if(provider){provider.closeAllConnections();await new Promise(r=>provider.close(r));}
 assert.deepEqual(await readFile(indexPath),indexBefore);
}
