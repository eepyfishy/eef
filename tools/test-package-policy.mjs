// Command-first remote policy control against production alpha apps. Isolated
import {readLoopbackCoordinatorTemplate} from './local-test-network.mjs';
// hardlinked managed node bundle; no model, installer, artifact or external I/O.
import assert from 'node:assert/strict';
import {mkdtemp,mkdir,link,unlink,readFile,writeFile,access} from 'node:fs/promises';
import {join,resolve} from 'node:path';
import {spawn} from 'node:child_process';
import {createServer as tcpServer} from 'node:net';
import {createServer} from 'node:http';
import {randomBytes,randomUUID} from 'node:crypto';
assert.equal(process.platform,'win32');
const root=resolve(import.meta.dirname,'..'),bin=resolve(process.env.EEF_TEST_BINARY_DIR||join(root,'target/debug'));
const scratch=await mkdtemp(join(root,'.validation/package-policy-')),children=[];
const install=join(scratch,'node'),config=join(install,'node.json'),identity=join(scratch,'eef.yaml'),id='package-policy-fixture';
const nodeExe=join(install,'eefn.exe'),policy=join(install,'.package-repair-eefn-policy.json');
const env={...process.env,EEF_NODE_PSK:'',APPDATA:join(scratch,'appdata'),EEF_DISCOVERY_DIR:join(scratch,'discovery'),PATH:join(root,'.tooling/llvm-mingw-20260616-ucrt-x86_64/bin')+';'+process.env.PATH};
let node,eef,proxyMode='drop',writes=0,networkChecks=0,ownerConfig=identity;
const source=tcpServer(socket=>{networkChecks++;socket.destroy();});
const proxy=createServer(async(req,res)=>{
 try {
  let body='';for await(const part of req)body+=part;
  const incoming=body&&JSON.parse(body),mutation=incoming?.command?.operation==='configure';
  if(mutation){writes++;if(proxyMode==='old'){res.writeHead(404);return res.end('{}');}}
  const reply=await fetch(eef+req.url,{method:req.method,headers:{'Content-Type':'application/json'},body:body||undefined,signal:AbortSignal.timeout(15000)});
  const value=await reply.json();
  if(mutation){
   if(proxyMode==='drop')return res.destroy();
   if(proxyMode==='corrupt')value.result.request_id=randomUUID();
   if(proxyMode==='prose')value.result.note='PRIVATE-MUST-NOT-ECHO';
  }
  res.writeHead(reply.status,{'Content-Type':'application/json'});res.end(JSON.stringify(value));
 }catch{res.destroy();}
});
async function port(){const s=tcpServer();await new Promise(r=>s.listen(0,'127.0.0.1',r));const p=s.address().port;await new Promise(r=>s.close(r));return p;}
function launch(exe,args,input,extra={}) {
 const c=spawn(exe,args,{cwd:root,env:{...env,...extra},windowsHide:true,stdio:[input===undefined?'ignore':'pipe','pipe','pipe']});let out='',err='';
 c.stdout.on('data',b=>out=(out+b).slice(-1048576));c.stderr.on('data',b=>err=(err+b).slice(-1048576));c.logs=()=>({out,err});
 c.result=new Promise((r,j)=>{c.on('error',j);c.on('close',code=>r({code,out,err}));});children.push(c);
 if(input!==undefined){c.stdin.on('error',()=>{});c.stdin.end(input);}return c;
}
async function cli(role,args,ok=true,plain=false,input) {
 const c=launch(role==='eefn'?nodeExe:join(bin,'eef.exe'),['--config',role==='eefn'?config:ownerConfig,...(plain?[]:['--json']),...args],input);
 const timer=setTimeout(()=>c.kill(),30000);
 try{const r=await c.result;assert.equal(r.code,ok?0:1,'CLI failed; private evidence '+scratch);return plain?r.out:JSON.parse(r.out);}finally{clearTimeout(timer);}
}
const local=(args,ok=true)=>cli('eefn',args,ok),owner=(args,ok=true)=>cli('eef',args,ok);
const remote=(args,ok=true)=>owner(['node','repair-setup','--node',id,...args],ok);
async function response(url,body,origin){return fetch(url,{method:body?'POST':'GET',headers:{'Content-Type':'application/json',...(origin?{Origin:origin}:{})},body:body?JSON.stringify(body):undefined,signal:AbortSignal.timeout(15000)});}
async function json(url,body){const r=await response(url,body);assert(r.ok,'HTTP failed; private evidence '+scratch);return r.json();}
async function until(fn,label){const end=Date.now()+30000;while(Date.now()<end){try{if(await fn())return;}catch{}await new Promise(r=>setTimeout(r,100));}throw Error('Timeout: '+label);}
async function absent(path){try{await access(path);assert.fail('Unexpected maintenance state');}catch(e){if(e.code!=='ENOENT')throw e;}}
async function saved(){return JSON.parse(await readFile(policy,'utf8'));}
try {
 await mkdir(install);await link(join(bin,'eefn.exe'),nodeExe);
 const versionProcess=launch(nodeExe,['--version']);const versionResult=await versionProcess.result;assert.equal(versionResult.code,0);
 const version=versionResult.out.trim().split(/\s+/).at(-1);assert(version.includes('-'),'Production prerelease gate required; stable release acceptance is separate');
 await new Promise(r=>source.listen(0,'127.0.0.1',r));await new Promise(r=>proxy.listen(0,'127.0.0.1',r));
 const manifest=`https://127.0.0.1:${source.address().port}/public.json`;
 await writeFile(join(install,'bundle.json'),JSON.stringify({name:'eefn',version}));await writeFile(join(install,'files.sha256.json'),'[]');
 await writeFile(join(install,'eefn.repair-source.json'),JSON.stringify({schema_version:1,program:'eefn',version,manifest_url:manifest}));
 const [api,gateway,web]=await Promise.all([port(),port(),port()]);node=`http://127.0.0.1:${api}`;eef=`http://127.0.0.1:${web}`;
 const initial=JSON.parse(await readFile(join(root,'config/node.example.json'),'utf8'));
 Object.assign(initial,{node_id:id,name:'Package policy fixture',auto_local:false,local_pairing:false,connection_enabled:false,endpoints:[],psk:'',
  dashboard:{enabled:true,ui_enabled:false,host:'127.0.0.1',port:api},update:{policy:'off'},
  request_limits:{work:1,control:2},models:{provider:'llamacpp',ollama:{base_url:'http://127.0.0.1:1',selected:[]},llamacpp:{binary:'tools/not-installed.exe',slots:[]}},custom:{preserve:true}});
 await writeFile(config,JSON.stringify(initial));
 const yaml=(await readLoopbackCoordinatorTemplate()).replace('port: 51334',`port: ${web}`).replace('port: 51335',`port: ${gateway}`).replace('policy: prompt','policy: off');await writeFile(identity,yaml);
 const proxyIdentity=join(scratch,'proxy-eef.yaml');await writeFile(proxyIdentity,yaml.replace(`port: ${web}`,`port: ${proxy.address().port}`));
 launch(nodeExe,['--config',config,'--no-ui']);launch(join(bin,'eef.exe'),['--config',identity,'--database',join(scratch,'eef.db'),'--no-brain','--no-ui'],undefined,{EEF_NODE_PSK:randomBytes(32).toString('hex')});
 await until(async()=>(await json(node+'/api/diagnostics')).runtime_id,'node runtime');
 await until(async()=>(await json(eef+'/api/diagnostics')).initialized,'coordinator');
 const invitation=await owner(['credentials','provision','--node',id,'--address',`127.0.0.1:${gateway}`]);
 await cli('eefn',['connection','pair-stdin'],true,false,JSON.stringify(invitation));
 const epoch=(await json(eef+'/api/diagnostics')).runtime_id;await owner(['restart']);
 await until(async()=>(await json(eef+'/api/diagnostics')).runtime_id!==epoch,'coordinator credentials adopted');
 await local(['connection','resume']);
 await until(async()=>{const v=await json(node+'/api/status');return v.connection.state==='connected'&&v.connection.transport==='pinned_tls13';},'pinned node connection');
 const show=await remote(['show']);assert.equal(show.result.policy.mode,'off');assert.equal(show.result.version.revision,0);assert.equal(show.result.remote_policy_allowed,false);await absent(policy);
 await remote(['configure','--mode','off'],false);await absent(policy);
 await local(['permissions','allow','remote-management']);
 await remote(['configure','--mode','off'],false);await absent(policy);
 const beforeConsent=await readFile(config,'utf8');
 await owner(['node','permissions','--node',id,'allow','remote-repair-setup-policy'],false);
 assert.equal(await readFile(config,'utf8'),beforeConsent,'eef cannot self-enable consent');
 const enabled=(await local(['permissions','allow','remote-repair-setup-policy'])).result;
 assert.equal(enabled.restart_required,false);
 const off=await remote(['configure','--mode','off']);assert.equal(off.result.policy.revision,1);
 const prompt=await remote(['configure','--mode','prompt']);assert.equal(prompt.result.policy.revision,2);assert.equal(prompt.result.policy.binding.public_source,manifest);
 assert.equal(prompt.result.prerelease_automation_blocked,true);
 await local(['repair-setup','run'],false);
 await absent(join(install,'.package-repair-eefn-setup.json'));
 assert.equal(networkChecks,0,'Actual alpha maintenance runner refuses before source I/O');
 assert((await cli('eef',['node','repair-setup','--node',id,'show'],true,true)).includes('shared installation maintenance policy'));
 const request={schema_version:1,node_id:id,expected_runtime_id:prompt.result.runtime_id,request_id:randomUUID(),command:{operation:'configure',previous:off.result.version,mode:'off',allow_automatic_repair:false,manifest:null}};
 const stale=await json(eef+'/api/commands/node/repair-setup-policy',request);assert.equal(stale.success,false);assert.equal((await saved()).revision,2);
 request.command.previous=prompt.result.version;request.expected_runtime_id=randomUUID();
 const wrong=await json(eef+'/api/commands/node/repair-setup-policy',request);assert.equal(wrong.success,false);assert.equal((await saved()).revision,2);
 request.expected_runtime_id=prompt.result.runtime_id;
 assert.equal((await response(eef+'/api/commands/node/repair-setup-policy',request,'https://untrusted.example')).status,403);
 assert.equal((await response(eef+'/api/commands/node/repair-setup-policy',{...request,padding:'x'.repeat(16384)})).status,413);
 await remote(['configure','--mode','auto','--allow-automatic-repair'],false);assert.equal((await saved()).revision,2);
 for(const mode of ['drop','corrupt','prose','old']) {
  const previous=await saved(),count=writes;
  proxyMode=mode;ownerConfig=proxyIdentity;
  try{const lost=await remote(['configure','--mode','off'],false);assert.equal(lost.outcome_unknown,true);assert.equal(lost.automatically_retried,false);assert.equal(writes,count+1);assert(!JSON.stringify(lost).includes('PRIVATE-MUST-NOT-ECHO'));}
  finally{ownerConfig=identity;}
  assert.equal((await saved()).revision,previous.revision+(mode==='old'?0:1));
 }
 await local(['permissions','deny','remote-repair-setup-policy']);
 const before=await readFile(policy,'utf8');await remote(['configure','--mode','prompt'],false);assert.equal(await readFile(policy,'utf8'),before);
 assert.equal((await remote(['show'])).result.remote_policy_allowed,false);
 const oldRuntime=(await remote(['show'])).result.runtime_id;
 await local(['restart']);await until(async()=>{const r=await remote(['show']);return r.success&&r.result.runtime_id!==oldRuntime;},'restarted node inspection');
 assert.equal(await readFile(policy,'utf8'),before,'restart does not replay a policy save');
 await absent(join(install,'.package-repair-eefn-setup.json'));await absent(join(install,'.package-repairs.json'));
 const final=JSON.parse(await readFile(config,'utf8'));for(const key of ['permissions','models','custom'])assert.deepEqual(final[key],initial[key]);
 assert.equal(networkChecks,0,'Production alpha supervisor never checks source');
 await writeFile(join(scratch,'results.json'),JSON.stringify({passed:true,physical_two_pc:false,no_models:true,pinned_node_transport:true,
  actual_prerelease_automation_refused:true,separate_local_consent:true,remote_self_grant_refused:true,revision_and_digest_cas:true,
  stale_runtime_refused:true,lost_corrupt_prose_unsupported_no_replay:true,read_only_after_revocation:true,restart_no_replay:true,
  owner_config_preserved:true,source_connections:networkChecks,automatic_repair_success_claimed:false},null,2));
 console.log('Package policy validation passed: '+scratch);
} catch(error) {
 await writeFile(join(scratch,'PRIVATE-failure.json'),JSON.stringify({message:String(error)},null,2));
 await writeFile(join(scratch,'PRIVATE-logs.json'),JSON.stringify(children.map(c=>({exe:c.spawnfile,code:c.exitCode,...c.logs()})),null,2));throw error;
} finally {
 for(const c of children)if(c.exitCode===null)c.kill();await Promise.allSettled(children.map(c=>c.result));
 proxy.closeAllConnections();await Promise.all([new Promise(r=>proxy.close(r)),new Promise(r=>source.close(r))]);
 // Only our hardlink, after owned processes exit. The source binary is retained.
 try{await unlink(nodeExe);}catch(e){if(e.code!=='ENOENT')throw e;}
}
