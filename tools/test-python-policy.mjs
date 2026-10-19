// Public native node/coordinator commands; synthetic plugin, no device/model work.
import assert from 'node:assert/strict';
import {mkdtemp,readFile,writeFile,stat} from 'node:fs/promises';
import {join,resolve} from 'node:path';
import {spawn} from 'node:child_process';
import {createServer as tcpServer} from 'node:net';
import {createServer} from 'node:http';
import {randomBytes,randomUUID} from 'node:crypto';
const root=resolve(import.meta.dirname,'..'),bin=resolve(process.env.EEF_TEST_BINARY_DIR||join(root,'target/debug'));
const python=process.env.EEF_TEST_PYTHON;
assert.equal(process.platform,'win32','Native job-commit acceptance is Windows-only');
assert(python,'Supply an existing trusted interpreter; no Python/model download is attempted');await stat(python);
const scratch=await mkdtemp(join(root,'.validation/python-policy-')),children=[];
const nodeConfig=join(scratch,'node.json'),eefConfig=join(scratch,'eef.yaml'),id='python-policy-fixture';
const marker=join(scratch,'plugin-imports'),plugin=join(scratch,'plugin.py');
const env={...process.env,EEF_NODE_PSK:'',APPDATA:join(scratch,'appdata'),EEF_DISCOVERY_DIR:join(scratch,'discovery'),PATH:join(root,'.tooling/llvm-mingw-20260616-ucrt-x86_64/bin')+';'+process.env.PATH};
let node,mode='drop',writes=0;
const proxy=createServer(async(req,res)=>{
 try{let body='';for await(const b of req)body+=b;const incoming=body&&JSON.parse(body),mutation=incoming?.command?.operation==='configure';
  if(mode==='unsupported'&&req.url==='/api/commands/python'){res.writeHead(404);return res.end('{}');}
  if(mutation){writes++;if(mode==='old'){res.writeHead(404);return res.end('{}');}}
  const response=await fetch(node+req.url,{method:req.method,headers:{'Content-Type':'application/json'},body:body||undefined,signal:AbortSignal.timeout(10000)});
  const value=await response.json();
  if(mutation){if(mode==='drop')return res.destroy();if(mode==='corrupt')value.saved.max_job_memory_mib=256;if(mode==='prose')value.note='PRIVATE-PYTHON-POLICY-MUST-NOT-ECHO';}
  res.writeHead(response.status,{'Content-Type':'application/json'});res.end(JSON.stringify(value));
 }catch{res.destroy();}
});
async function port(){const s=tcpServer();await new Promise(r=>s.listen(0,'127.0.0.1',r));const p=s.address().port;await new Promise(r=>s.close(r));return p;}
function launch(name,args,extra={}){
 const c=spawn(join(bin,name+'.exe'),args,{cwd:root,windowsHide:true,env:{...env,...extra},stdio:['ignore','pipe','pipe']});let out='',err='';
 c.stdout.on('data',b=>out=(out+b).slice(-1048576));c.stderr.on('data',b=>err=(err+b).slice(-1048576));c.logs=()=>({out,err});
 c.result=new Promise((r,j)=>{c.on('error',j);c.on('close',code=>r({code,out,err}));});children.push(c);return c;
}
async function cli(name,args,ok=true,plain=false){const c=launch(name,['--config',name==='eefn'?nodeConfig:eefConfig,...(plain?[]:['--json']),...args]),timer=setTimeout(()=>c.kill(),30000);try{const r=await c.result;assert.equal(r.code,ok?0:1,'CLI failed; inspect private '+scratch);return plain?r:JSON.parse(r.out);}finally{clearTimeout(timer);}}
const local=(args,ok=true,plain=false)=>cli('eefn',args,ok,plain);
const remote=(args,ok=true,plain=false)=>cli('eef',['node','python','--node',id,...args],ok,plain);
async function http(url,body,headers={}){return fetch(url,{method:body?'POST':'GET',headers:{'Content-Type':'application/json',...headers},body:body?JSON.stringify(body):undefined,signal:AbortSignal.timeout(10000)});}
async function json(url,body){const r=await http(url,body);assert(r.ok,'HTTP failed; inspect private '+scratch);return r.json();}
async function until(fn,label){const end=Date.now()+30000;while(Date.now()<end){try{if(await fn())return;}catch{}await new Promise(r=>setTimeout(r,100));}throw Error('Timeout: '+label);}
const show=async()=>(await local(['python','show'])).result;
try{
 await new Promise(r=>proxy.listen(0,'127.0.0.1',r));const [api,gateway,web]=await Promise.all([port(),port(),port()]);node=`http://127.0.0.1:${api}`;
 await writeFile(plugin,`from pathlib import Path\nCAPABILITY='fixture.budget'\nACTIONS=['run']\nwith Path(__file__).with_name('plugin-imports').open('ab') as f: f.write(b'I')\ndef invoke(action,params): return {'synthetic': True}\n`);
 const config=JSON.parse(await readFile(join(root,'config/node.example.json'),'utf8'));config.node_id=id;config.dashboard.port=api;config.update.policy='off';config.models.provider='llamacpp';config.models.ollama.base_url='http://127.0.0.1:1';config.custom={preserve:true};config.python_plugins=[plugin];
 await writeFile(nodeConfig,JSON.stringify(config));
 const yaml=(await readFile(join(root,'config/default_identity.yaml'),'utf8')).replace('port: 51334',`port: ${web}`).replace('port: 51335',`port: ${gateway}`).replace('policy: prompt','policy: off');await writeFile(eefConfig,yaml);
 launch('eef',['--config',eefConfig,'--database',join(scratch,'eef.db'),'--no-brain','--no-ui'],{EEF_NODE_PSK:randomBytes(32).toString('hex')});
 launch('eefn',['--config',nodeConfig,'--no-ui','--python',python]);
 await until(async()=>(await json(node+'/api/status')).connection.state==='connected','connected');
 const initial=await show();assert.equal(initial.saved.max_job_memory_mib,undefined);assert.deepEqual(initial.saved,initial.applied);assert.equal(initial.memory_ceiling_supported,true);
 const imports=await readFile(marker,'utf8');assert.match(imports,/^I+$/,'synthetic plugin inspected during initial node startup');
 const saved=(await local(['python','configure','--memory-mib','128','--timeout-seconds','8'])).result;
 assert.equal(saved.saved.max_job_memory_mib,128);assert.equal(saved.applied.max_job_memory_mib,undefined);assert.equal(saved.restart_required,true);assert.equal(saved.restart_requested,false);
 const noOpBefore=await readFile(nodeConfig,'utf8');assert.equal((await local(['python','configure','--memory-mib','128'])).result.changed,false);assert.equal(await readFile(nodeConfig,'utf8'),noOpBefore);
 const localPlain=await local(['python','show'],true,true);assert(localPlain.out.includes('Node '+id+' Python bridge'));assert(localPlain.out.includes('128 MiB'));
 assert.equal((await remote(['show'])).result.saved.max_job_memory_mib,128);
 const deniedBefore=await readFile(nodeConfig,'utf8');assert.equal((await remote(['configure','--timeout-seconds','9'],false)).success,false);assert.equal(await readFile(nodeConfig,'utf8'),deniedBefore);
 await local(['permissions','allow','remote-management']);
 const approved=await remote(['configure','--max-stderr-bytes','2048']);assert.equal(approved.result.saved.max_stderr_bytes,2048);assert.equal(approved.result.saved.max_job_memory_mib,128);
 assert((await remote(['show'],true,true)).out.includes('128 MiB'));
 await local(['permissions','deny','remote-management']);const revoked=await readFile(nodeConfig,'utf8');await remote(['configure','--default-memory'],false);assert.equal(await readFile(nodeConfig,'utf8'),revoked);
 const current=await show();const request={schema_version:1,expected_node_id:id,expected_runtime_id:current.runtime_id,command:{operation:'configure',limits:{...current.saved,timeout_seconds:11},expected_digest:current.saved_digest}};
 const url=node+'/api/commands/python';
 assert.equal((await http(url,{...request,command:{...request.command,expected_digest:initial.saved_digest}})).status,400);
 assert.equal((await http(url,{...request,expected_runtime_id:randomUUID()})).status,400);
 assert.equal((await http(url,request,{Origin:'https://untrusted.example'})).status,403);
 assert.equal((await http(url,{...request,padding:'x'.repeat(8192)})).status,413);
 assert.equal((await http(url,{...request,command:{...request.command,restart:true}})).status,422);
 for(const [i,next] of ['drop','corrupt','prose','old'].entries()){
  const apiMarker=nodeConfig+'.api.json',original=await readFile(apiMarker,'utf8');mode=next;const count=writes,seconds=12+i;
  await writeFile(apiMarker,JSON.stringify({...JSON.parse(original),address:'127.0.0.1:'+proxy.address().port}));
  try{const response=await local(['python','configure','--timeout-seconds',String(seconds)],false);assert.equal(response.outcome_unknown,true);assert.equal(response.automatically_retried,false);assert.equal(response.command.limits.timeout_seconds,seconds);assert.equal(writes,count+1);assert(!JSON.stringify(response).includes('PRIVATE-PYTHON-POLICY-MUST-NOT-ECHO'));}
  finally{await writeFile(apiMarker,original);}
  assert.equal((await show()).saved.timeout_seconds,next==='old'?14:seconds);
 }
 {
  const apiMarker=nodeConfig+'.api.json',original=await readFile(apiMarker,'utf8'),before=await readFile(nodeConfig,'utf8'),count=writes;mode='unsupported';
  await writeFile(apiMarker,JSON.stringify({...JSON.parse(original),address:'127.0.0.1:'+proxy.address().port}));
  try{const response=await local(['python','configure','--timeout-seconds','16'],false);assert.equal(response.success,false);assert.equal(response.sending_attempted,false);assert.equal(response.outcome_unknown,false);assert.equal(writes,count);assert.equal(await readFile(nodeConfig,'utf8'),before);}
  finally{await writeFile(apiMarker,original);}
 }
 assert.equal(await readFile(marker,'utf8'),imports,'configuration commands cannot inspect/import/execute Python');
 assert.equal((await show()).runtime_id,initial.runtime_id,'no automatic restart');
 await local(['restart','--wait-seconds','30']);
 await until(async()=>{const r=await show();return r.runtime_id!==initial.runtime_id&&r.applied?.max_job_memory_mib===128&&r.applied?.timeout_seconds===14;},'explicitly restarted limits applied');
 assert.equal(await readFile(marker,'utf8'),imports+'I','only the explicit restart imports the plugin again');
 const defaulted=(await local(['python','configure','--default-memory'])).result;assert.equal(defaulted.saved.max_job_memory_mib,undefined);assert.equal(defaulted.applied.max_job_memory_mib,128);assert.equal(defaulted.saved.timeout_seconds,14);
 const final=JSON.parse(await readFile(nodeConfig,'utf8'));assert.deepEqual(final.permissions,{...config.permissions});assert.deepEqual(final.models,config.models);assert.deepEqual(final.python_plugins,[plugin]);assert.deepEqual(final.custom,{preserve:true});
 await writeFile(join(scratch,'results.json'),JSON.stringify({passed:true,physical_two_pc:false,no_models_or_devices:true,local_and_remote_commands:true,owner_consent_and_revocation:true,saved_applied_limits:true,explicit_restart_only:true,strict_origin_runtime_digest_body:true,lost_corrupt_unsupported_reply_no_retry:true,unsupported_inspection_before_any_write:true,unrelated_settings_preserved:true,native_commit_enforcement:'separate test-python-bridge.mjs'},null,2));
 console.log('Python policy command validation passed: '+scratch);
}catch(error){await writeFile(join(scratch,'PRIVATE-failure.json'),JSON.stringify({message:String(error)},null,2));await writeFile(join(scratch,'PRIVATE-logs.json'),JSON.stringify(children.map(c=>({exe:c.spawnfile,code:c.exitCode,...c.logs()})),null,2));throw error;}
finally{for(const c of children)if(c.exitCode===null)c.kill();await Promise.allSettled(children.map(c=>c.result));proxy.closeAllConnections();await new Promise(r=>proxy.close(r));}
