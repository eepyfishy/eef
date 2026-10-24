// Scoped model-setup restarts. Synthetic provider only; no real weights/inference.
import assert from 'node:assert/strict';
import {mkdtemp,mkdir,readFile,writeFile,link,unlink} from 'node:fs/promises';
import {join,resolve,sep} from 'node:path';
import {spawn} from 'node:child_process';
import {createServer as tcpServer} from 'node:net';
import {createServer} from 'node:http';
import {randomBytes} from 'node:crypto';
const root=resolve(import.meta.dirname,'..'),bin=resolve(process.env.EEF_TEST_BINARY_DIR||join(root,'target/debug'));
const scratch=await mkdtemp(join(root,'.validation/model-activation-')),children=[],states=new Map();
const ownedLinks=[],ownedMarkers=[];
const owned=path=>{assert(resolve(path).startsWith(resolve(scratch)+sep),'Fixture mutation must stay inside its new scratch directory');return path;};
const identity=join(scratch,'eef.yaml');
const env={...process.env,EEF_NODE_PSK:'',APPDATA:join(scratch,'appdata'),EEF_DISCOVERY_DIR:join(scratch,'discovery'),PATH:join(root,'.tooling/llvm-mingw-20260616-ucrt-x86_64/bin')+';'+process.env.PATH};
let eef;
const provider=createServer((req,res)=>{
 req.resume();const [kind,...parts]=req.url.slice(1).split('/'),path='/'+parts.join('/'),s=states.get(kind);
 if(!s){res.writeHead(404);return res.end();}
 if(path==='/api/tags'){
  const send=()=>{if(!res.destroyed)res.end(JSON.stringify({models:[{name:'fixture:latest'}]}));};
  if(s.gate && s.shows>=2){s.held.push(send);return;}return send();
 }
 if(path==='/api/show'){s.shows++;return res.end(JSON.stringify({capabilities:['completion']}));}
 if(path==='/api/ps')return res.end(JSON.stringify({models:[]}));
 if(path==='/api/pull'){s.pulls++;res.writeHead(500);return res.end('Unexpected pull');}
 if(path==='/api/generate'||path==='/api/chat'){s.generationRequests++;return res.end(JSON.stringify({done:true,response:''}));}
 res.writeHead(404);res.end();
});
function start(name,args,input,extra={},binaryOverride){
 const c=spawn(binaryOverride||join(bin,name+'.exe'),args,{cwd:root,windowsHide:true,env:{...env,...extra},stdio:[input===undefined?'ignore':'pipe','pipe','pipe']});let out='',err='';
 c.stdout.on('data',b=>out=(out+b).slice(-1048576));c.stderr.on('data',b=>err=(err+b).slice(-1048576));c.logs=()=>({out,err});
 c.result=new Promise((r,j)=>{c.on('error',j);c.on('close',code=>r({code,out,err}));});children.push(c);
 if(input!==undefined){c.stdin.on('error',()=>{});c.stdin.end(input);}return c;
}
async function cli(name,path,args,expect=[0],input){const c=start(name,['--config',path,'--json',...args],input),timer=setTimeout(()=>c.kill(),30000);try{const r=await c.result;assert(expect.includes(r.code),'CLI failed; private evidence '+scratch);return JSON.parse(r.out);}finally{clearTimeout(timer);}}
async function json(url,body,method){const r=await fetch(url,{method:method||(body?'POST':'GET'),headers:{'Content-Type':'application/json'},body:body?JSON.stringify(body):undefined,signal:AbortSignal.timeout(10000)});assert(r.ok,'HTTP failed; private evidence '+scratch);return r.json();}
async function until(fn,label){const end=Date.now()+30000;while(Date.now()<end){try{if(await fn())return;}catch{}await new Promise(r=>setTimeout(r,100));}throw Error('Timeout: '+label);}
async function port(){const s=tcpServer();await new Promise(r=>s.listen(0,'127.0.0.1',r));const p=s.address().port;await new Promise(r=>s.close(r));return p;}
const owner=(args,expect)=>cli('eef',identity,args,expect);
async function fixture(kind,{gate=false,override=false,isolatedInstall=false}={}){
 const dir=join(scratch,kind);await mkdir(dir);const config=join(dir,'node.json'),api=await port(),url=`http://127.0.0.1:${api}`,id='activation-'+kind;
 const state={gate,shows:0,pulls:0,generationRequests:0,held:[]};states.set(kind,state);
 const base=`http://127.0.0.1:${provider.address().port}/${kind}`;
 const value=JSON.parse(await readFile(join(root,'config/node.example.json'),'utf8'));
 Object.assign(value,{node_id:id,name:'Original '+kind,connection_enabled:false,auto_local:false,local_pairing:false,endpoints:[],psk:'',
  future:{secret:'PRIVATE_FROZEN_CONFIGURATION_MUST_NOT_LEAK'},dashboard:{enabled:true,ui_enabled:false,host:'127.0.0.1',port:api},update:{policy:'off'},
  models:{provider:'auto',ollama:{base_url:base,selected:[]},llamacpp:{binary:'tools/fixture-no-model-runtime.exe',slots:[]}},
  model_catalog:[{id:'fixture',modality:'text',ollama:'fixture:latest',url:'https://example.invalid/fixture.gguf',sha256:'a'.repeat(64),bytes:4,license:'test-only',source_page:'https://example.invalid/license'}]});
 await writeFile(config,JSON.stringify(value));
 let binary,installRoot;
 if(isolatedInstall){
  const version=(await readFile(join(root,'Cargo.toml'),'utf8')).match(/^version\s*=\s*"([^"]+)"/m)[1];
  installRoot=join(dir,'private-install');const versionDir=join(installRoot,'versions',version);await mkdir(versionDir,{recursive:true});
  binary=owned(join(versionDir,'eefn.exe'));await link(join(bin,'eefn.exe'),binary);ownedLinks.push(binary);
 }
 const launch=()=>start('eefn',['--config',config,'--no-ui',...(override?['--ollama-url',base]:[])],undefined,{},binary);
 let process=launch();const local=(args,expect,input)=>cli('eefn',config,args,expect,input),saved=async()=>JSON.parse(await readFile(config,'utf8'));
 await until(async()=>(await json(url+'/api/diagnostics')).runtime_id,'core startup '+kind);
 await local(['model-bootstrap-policy','show']);const original=(await json(url+'/api/diagnostics')).runtime_id;
 return {kind,dir,config,url,id,state,value,local,saved,original,launch,installRoot,get process(){return process;},set process(p){process=p;},show:async()=>(await local(['model-bootstrap-policy','show'])).result,
  enable:()=>local(['model-bootstrap-policy','configure','--mode','auto','--model','fixture','--enable-automation','--restart-after-setup'])};
}
async function assertBound(f){
 const r=await f.show(),runtime=(await json(f.url+'/api/diagnostics')).runtime_id;
 assert.equal(r.setup.attempt.phase,'runtime_configured');assert.equal(r.setup.attempt.activation.replacement_runtime_id,runtime);assert.notEqual(runtime,f.original);
 assert.equal(r.setup.attempt.runtime_id,f.original);assert.equal(r.setup.readiness_verified,false);assert.equal(r.setup.restart_required,false);
 assert.equal((await f.saved()).models.ollama.selected[0].model_id,'fixture:latest');
 const lifecycle=(await f.local(['model-lifecycle','show'])).result;
 assert.equal(lifecycle.runtime_id,runtime);assert.equal(lifecycle.ollama_preparation_supported,true,'replacement engine must bind a preparable Ollama selection; owned-GGUF inventory is separate');
 const text=JSON.stringify(r);assert(!text.includes('PRIVATE_FROZEN_CONFIGURATION_MUST_NOT_LEAK'));assert(!text.includes('example.invalid'));
 return r;
}
try{
 await new Promise(r=>provider.listen(0,'127.0.0.1',r));const [web,gateway]=await Promise.all([port(),port()]);eef=`http://127.0.0.1:${web}`;
 const yaml=(await readFile(join(root,'config/default_identity.yaml'),'utf8')).replace('port: 51334',`port: ${web}`).replace('port: 51335',`port: ${gateway}`).replace('policy: prompt','policy: off');await writeFile(identity,yaml);
 start('eef',['--config',identity,'--database',join(scratch,'eef.db'),'--no-brain','--no-ui'],undefined,{EEF_NODE_PSK:randomBytes(24).toString('hex')});
 await until(async()=>(await json(eef+'/api/diagnostics')).initialized,'coordinator startup');
 const success=await fixture('success',{isolatedInstall:true});
 const invitation=await owner(['credentials','provision','--node',success.id,'--address',`127.0.0.1:${gateway}`]);await success.local(['connection','pair-stdin'],undefined,JSON.stringify(invitation));
 const epoch=(await json(eef+'/api/diagnostics')).runtime_id;await owner(['restart']);await until(async()=>(await json(eef+'/api/diagnostics')).runtime_id!==epoch,'apply fixture credentials');
 await success.local(['connection','resume']);await until(async()=>(await json(eef+'/api/diagnostics')).registered_node_count===1,'pinned registration');
 success.original=(await json(success.url+'/api/diagnostics')).runtime_id;await success.local(['permissions','allow','remote-management']);
 await success.local(['permissions','allow','remote-repair-setup-policy']);
 assert.equal((await json(success.url+'/api/status')).pending_restart,false,'live maintenance consent is not a pending application setting');
 // Isolated install only: a normal restart would consult this pending app
 // selection. Its target deliberately does not exist, so no new executable can
 // be launched by the negative control. Hard-linking avoids binary copies.
 const versionMarker=owned(join(success.installRoot,'current.txt')),pendingVersion='0.4.99-activation-fixture';
 await writeFile(versionMarker,pendingVersion);ownedMarkers.push(versionMarker);
 const response=await owner(['node','model-bootstrap-policy','--node',success.id,'configure','--mode','auto','--model','fixture','--enable-automation','--restart-after-setup'],[0,1]);
 if(!response.success)assert.equal(response.outcome_unknown,true,'a lost restart-adjacent reply must remain uncertain, not replayed');
 await until(async()=>(await success.show()).setup.attempt?.phase==='runtime_configured','automatic pinned remote setup restart');
 const first=await assertBound(success);assert.equal((await json(success.url+'/api/status')).name,'Original success');
 assert.equal((await success.saved()).management.allow_remote_repair_setup_policy,true);
 assert.equal(success.process.exitCode,null);assert.equal(await readFile(versionMarker,'utf8'),pendingVersion);
 await until(async()=>(await owner(['node','model-bootstrap-policy','--node',success.id,'show'])).result.runtime_id===(await json(success.url+'/api/diagnostics')).runtime_id,'replacement pinned session');
 assert.equal((await success.local(['models','show'])).report_type,'model_selections');
 await until(async()=>(await success.local(['models','show'])).registered_models?.some(m=>m.model_id==='fixture:latest'&&m.backend==='ollama'&&m.model_metadata?.roles?.includes('request_interpreter')),'replacement engine advertises the exact interpreter through pinned registration');
 // Negative control: ordinary explicit restart really does consult the marker
 // in this private installation, proving activation did not simply miss it.
 const ordinary=start('eefn',['--config',success.config,'--json','restart']);
 await until(()=>success.process.exitCode!==null,'explicit restart consults pending private app selection');
 if(ordinary.exitCode===null)ordinary.kill();await ordinary.result;
 await unlink(owned(versionMarker));ownedMarkers.splice(ownedMarkers.indexOf(versionMarker),1);

 const blocked=await fixture('blocked');let next=await blocked.saved();next.name='Unrelated pending name';await json(blocked.url+'/api/config',{config:next},'PUT');await blocked.enable();
 await until(async()=>(await blocked.show()).setup.attempt?.phase==='apply_blocked','unrelated pending settings refusal');
 assert.equal((await blocked.show()).setup.attempt.reason,'pending_settings');assert.equal((await json(blocked.url+'/api/diagnostics')).runtime_id,blocked.original);
 assert.equal((await blocked.local(['network','show'])).applied_name,'Original blocked');assert.equal((await blocked.saved()).name,'Unrelated pending name');
 await blocked.local(['restart']);assert.equal((await blocked.local(['network','show'])).applied_name,'Unrelated pending name');

 const late=await fixture('late',{gate:true});await late.enable();
 await until(async()=>late.state.held.length>0&&(await json(late.url+'/api/status')).model_activation?.state==='draining','captured restart with gated provider');
 next=await late.saved();next.name='Saved during admitted restart';next.future.pending=true;next.model_bootstrap.automation=false;await json(late.url+'/api/config',{config:next},'PUT');const lateBytes=await readFile(late.config,'utf8');
 late.state.gate=false;for(const finish of late.state.held.splice(0))finish();
 await until(async()=>(await late.show()).setup.attempt?.phase==='runtime_configured','scoped startup after late settings change');
 await assertBound(late);assert.equal((await late.local(['network','show'])).applied_name,'Original late');assert.equal((await json(late.url+'/api/status')).pending_restart,true);
 assert.equal(await readFile(late.config,'utf8'),lateBytes,'frozen configuration must never overwrite later saved data');

 const liveConsent=await fixture('live-consent',{gate:true});await liveConsent.enable();
 await until(async()=>liveConsent.state.held.length>0&&(await json(liveConsent.url+'/api/status')).model_activation?.state==='draining','scoped startup before live consent edit');
 // The old executor binding has retired at this gate. Use the existing local
 // owner settings backend; never bypass exact-runtime permission commands.
 const liveSettings=await liveConsent.saved();liveSettings.management.allow_remote_repair_setup_policy=true;
 await json(liveConsent.url+'/api/config',{config:liveSettings},'PUT');
 const liveBytes=await readFile(liveConsent.config,'utf8');
 liveConsent.state.gate=false;for(const finish of liveConsent.state.held.splice(0))finish();
 await until(async()=>(await liveConsent.show()).setup.attempt?.phase==='runtime_configured','scoped startup after live consent edit');
 await assertBound(liveConsent);
 assert.equal((await json(liveConsent.url+'/api/status')).pending_restart,false,'late live-only consent does not falsely require another restart');
 assert.equal((await liveConsent.saved()).management.allow_remote_repair_setup_policy,true);
 assert.equal(await readFile(liveConsent.config,'utf8'),liveBytes,'scoped adoption preserves the later local consent save');

 const overrides=await fixture('override',{override:true});await overrides.enable();await until(async()=>(await overrides.show()).setup.attempt?.phase==='apply_blocked','model CLI override refusal');
 assert.equal((await overrides.show()).setup.attempt.reason,'activation_unavailable');assert.equal((await json(overrides.url+'/api/diagnostics')).runtime_id,overrides.original);

 const prompt=await fixture('prompt');
 await prompt.local(['model-bootstrap-policy','configure','--mode','prompt','--model','fixture','--enable-automation','--restart-after-setup']);
 const promptSaved=(await prompt.show()).saved;assert.equal((await prompt.show()).setup.state,'awaiting_approval');
 await prompt.local(['model-bootstrap-policy','approve']);
 await until(async()=>(await prompt.show()).setup.attempt?.phase==='runtime_configured','approved prompt restart with separate consent');
 await assertBound(prompt);assert.deepEqual((await prompt.show()).saved,promptSaved);
 assert.equal((await prompt.show()).saved.policy.mode,'prompt');assert.equal((await prompt.show()).setup.attempt.owner_approved,true);

 const crash=await fixture('crash',{gate:true});await crash.enable();await until(async()=>crash.state.held.length>0&&(await json(crash.url+'/api/status')).model_activation?.state==='draining','accepted activation before fixture exit');
 // Terminate only the fixture handle, not any installed app or general process name.
 crash.process.kill();await crash.process.result;crash.state.gate=false;for(const finish of crash.state.held.splice(0))finish();crash.process=crash.launch();
 await until(async()=>(await json(crash.url+'/api/diagnostics')).runtime_id!==crash.original&&(await crash.show()).setup.attempt?.phase==='unknown','interrupted activation retained unknown');
 const interrupted=await crash.show();assert.equal(interrupted.setup.attempt.activation.replacement_runtime_id,null);assert.equal(interrupted.setup.attempt.reason,'runtime_interrupted');
 assert.equal((await json(crash.url+'/api/status')).model_activation,null,'ordinary process launch is not replay of an admitted snapshot');
 for(const state of states.values())assert.equal(state.pulls,0,'installed metadata must not trigger another pull');
 await writeFile(join(scratch,'results.json'),JSON.stringify({passed:true,physical_two_pc:false,synthetic_provider_only:true,real_model_inference:false,real_weights_downloaded:false,
  pinned_remote_automatic_restart:true,replacement_engine_binding:true,pending_settings_refused:true,late_settings_saved_not_applied:true,late_opt_out_preserved:true,
  explicit_restart_still_applies_pending:true,pending_app_version_not_applied_by_activation:true,explicit_restart_consults_pending_version:true,model_overrides_refused:true,interrupted_activation_unknown_no_replay:true,private_snapshot_not_reported:true,readiness_not_claimed:true,
  prompt_approval_uses_separate_restart_consent:true,prompt_policy_unchanged_after_activation:true,
  live_maintenance_consent_before_activation_not_pending:true,late_live_maintenance_consent_preserved_without_pending_restart:true},null,2));
 console.log('Model activation native validation passed: '+scratch);
}catch(error){await writeFile(join(scratch,'PRIVATE-failure.json'),JSON.stringify({message:String(error)},null,2));await writeFile(join(scratch,'PRIVATE-logs.json'),JSON.stringify(children.map(c=>({exe:c.spawnfile,code:c.exitCode,...c.logs()})),null,2));throw error;}
finally{for(const c of children)if(c.exitCode===null)c.kill();await Promise.allSettled(children.map(c=>c.result));for(const s of states.values())for(const finish of s.held)finish();provider.closeAllConnections();await new Promise(r=>provider.close(r));for(const path of [...ownedMarkers,...ownedLinks])await unlink(owned(path));}
