// Native managed first-run config/identity and explicit private enrollment.
// No installed state, browser, feature grant, model or physical-node workload.
import assert from 'node:assert/strict';
import {mkdtemp,mkdir,readFile,writeFile,link,unlink,readdir} from 'node:fs/promises';
import {resolve,join,sep} from 'node:path';
import {spawn} from 'node:child_process';
import {createServer} from 'node:net';
import {hostname} from 'node:os';
assert.equal(process.platform,'win32');
const root=resolve(import.meta.dirname,'..'),bin=resolve(process.env.EEF_TEST_BINARY_DIR||join(root,'target/debug'));
const scratch=await mkdtemp(join(root,'.validation/first-run-')),children=[],links=[];
const cwd=join(scratch,"unrelated working directory 'owner'");await mkdir(cwd);
const env={...process.env,EEF_NODE_PSK:'',APPDATA:join(scratch,'appdata'),EEF_DISCOVERY_DIR:join(scratch,'discovery'),
 HTTP_PROXY:'',HTTPS_PROXY:'',ALL_PROXY:'',NO_PROXY:'127.0.0.1,localhost,::1',
 http_proxy:'',https_proxy:'',all_proxy:'',no_proxy:'127.0.0.1,localhost,::1',NODE_USE_ENV_PROXY:'0',
 PATH:join(root,'.tooling/llvm-mingw-20260616-ucrt-x86_64/bin')+';'+process.env.PATH};
const version=(await readFile(join(root,'Cargo.toml'),'utf8')).match(/^version\s*=\s*"([^"]+)"/m)[1];
async function alias(role,name){const install=join(scratch,name),exe=join(install,role+'.exe');await mkdir(install,{recursive:true});await link(join(bin,role+'.exe'),exe);links.push(exe);
 await writeFile(join(install,'bundle.json'),JSON.stringify({name:role,version}));return {exe,install};}
function launch(exe,args){const c=spawn(exe,args,{cwd,env,windowsHide:true,stdio:['ignore','pipe','pipe']});let out='',err='';
 c.stdout.on('data',b=>out=(out+b).slice(-1048576));c.stderr.on('data',b=>err=(err+b).slice(-1048576));c.logs=()=>({out,err});
 c.result=new Promise((r,j)=>{c.on('error',j);c.on('close',code=>r({code,out,err}));});children.push(c);return c;}
async function once(exe,args,timeout=30000){const c=launch(exe,args),timer=setTimeout(()=>c.kill(),timeout);return c.result.finally(()=>clearTimeout(timer));}
async function cli(exe,args,code=0){const r=await once(exe,args,args.includes('--enroll-local')?120000:30000);
 assert.equal(r.code,code,'Command failed; inspect private fixture '+scratch);return JSON.parse(r.out);}
async function json(url){const r=await fetch(url,{signal:AbortSignal.timeout(5000)});assert(r.ok);return r.json();}
async function until(fn,label){const end=Date.now()+60000;while(Date.now()<end){try{if(await fn())return;}catch{}await new Promise(r=>setTimeout(r,100));}throw Error('Timeout: '+label);}
async function exists(path){return readFile(path).then(()=>true,()=>false);}
const privateSecret=bytes=>{const m=bytes.toString().match(/^\s+psk:\s*['"]?([a-f0-9]{64})['"]?\s*$/m);assert(m,'New configuration needs its own saved credential');return m[1];};
async function noDefaultPortCollision(){const probes=[];try{
 for(const port of [51334,51335,51336]){const s=createServer();probes.push(s);await new Promise((r,j)=>{s.once('error',j);s.listen({port,host:'0.0.0.0',exclusive:true},r);});}
 }catch{throw Error('Default first-run ports already occupied; do not stop another app or claim this fixture passed');}
 finally{await Promise.all(probes.map(s=>s.listening?new Promise(r=>s.close(r)):Promise.resolve()));}}
try{
 const offline=await alias('eef','offline');
 await writeFile(join(offline.install,'current.txt'),'99.0.0\n'); // Deliberately missing selected package; never executed.
 const path=join(offline.install,'config/default_identity.yaml');
 const created=await cli(offline.exe,['--json','initialize']);assert.equal(created.report_type,'coordinator_initialization');assert.equal(created.success,true);assert.equal(created.configuration_created,true);
 const bytes=await readFile(path),secret=privateSecret(bytes);assert(!JSON.stringify(created).includes(secret));
 for(const key of ['network_started','enrollment_requested','restart_requested','runtime_verified','models_changed','permissions_changed'])assert.equal(created[key],false,key);
 const preserved=await cli(offline.exe,['--json','initialize']);assert.equal(preserved.state,'existing');assert.equal(preserved.existing_configuration_preserved,true);assert.deepEqual(await readFile(path),bytes);
 assert.equal(await readFile(join(offline.install,'current.txt'),'utf8'),'99.0.0\n');
 await writeFile(path,Buffer.concat([Buffer.from('# Owner formatting must survive\r\n'),bytes]));
 const comments=await readFile(path);assert.equal((await cli(offline.exe,['--json','initialize'])).success,true);assert.deepEqual(await readFile(path),comments);
 const invalid=Buffer.from('PRIVATE_INVALID_SETTINGS: [ never closed');await writeFile(path,invalid);
 const rejected=await cli(offline.exe,['--json','initialize'],1);assert.equal(rejected.success,false);assert(!JSON.stringify(rejected).includes('PRIVATE_INVALID_SETTINGS'));assert.deepEqual(await readFile(path),invalid);
 await writeFile(path,'node:\n  psk: change-me-eef\n');const placeholder=await readFile(path);
 const needsOwner=await cli(offline.exe,['--json','initialize'],1);assert.equal(needsOwner.legacy_secret_requirement_satisfied,false);assert.deepEqual(await readFile(path),placeholder);
 const other=await alias('eef','offline-other');await cli(other.exe,['--json','initialize']);const otherSecret=privateSecret(await readFile(join(other.install,'config/default_identity.yaml')));assert.notEqual(otherSecret,secret);
 assert.equal(await exists(join(cwd,'config/default_identity.yaml')),false);
 await noDefaultPortCollision();
 const coordinator=await alias('eef','live-coordinator'),node=await alias('eefn','live-node');
 const coordinatorConfig=join(coordinator.install,'config/default_identity.yaml'),nodeConfig=join(node.install,'config.json');
 assert.equal(await exists(coordinatorConfig),false);assert.equal(await exists(nodeConfig),false);
 const runningCoordinator=launch(coordinator.exe,['--no-ui','--no-brain']);
 const eef='http://127.0.0.1:51334',eefn='http://127.0.0.1:51336';let eefRuntime;
 await until(async()=>{eefRuntime=(await json(eef+'/api/diagnostics')).runtime_id;return !!eefRuntime;},'fresh coordinator saved config and API');
 const initialized=await readFile(coordinatorConfig);assert.notEqual(privateSecret(initialized),secret);
 const runningNode=launch(node.exe,['--no-ui']);let nodeRuntime;
 await until(async()=>{nodeRuntime=(await json(eefn+'/api/diagnostics')).runtime_id;return !!nodeRuntime&&(await json(eefn+'/api/status')).connection.state==='connected';},'fresh node automatically registers');
 const first=JSON.parse(await readFile(nodeConfig,'utf8'));assert.match(first.node_id,/^device-[a-f0-9-]{36}$/);
 assert.equal(first.name.toLowerCase(),(hostname().trim()||'This PC').toLowerCase());
 const permissions=structuredClone(first.permissions),models=structuredClone(first.models);
 assert.equal((await json(eefn+'/api/status')).models.length,0);assert.equal((await json(eefn+'/api/status')).download.state,'idle');
 const busy=await cli(coordinator.exe,['--json','initialize'],1);assert.equal(busy.state,'coordinator_running');assert.deepEqual(await readFile(coordinatorConfig),initialized);
 // Explicit owner consent: existing backend owns provisioning and both restarts.
 const enrolled=await cli(node.exe,['--json','setup','--enroll-local','--restart-coordinator']);assert.equal(enrolled.success,true);
 await until(async()=>{const n=await json(eefn+'/api/diagnostics'),e=await json(eef+'/api/diagnostics');return !!n.runtime_id&&n.runtime_id!==nodeRuntime&&!!e.runtime_id&&e.runtime_id!==eefRuntime&&(await json(eefn+'/api/status')).connection.state==='connected';},'private enrollment reconnects after both runtime restarts');
 const paired=JSON.parse(await readFile(nodeConfig,'utf8'));assert.equal(paired.auth_mode,'node_credential');
 assert.equal(paired.node_id,first.node_id);assert.equal(paired.name,first.name);assert.deepEqual(paired.permissions,permissions);assert.deepEqual(paired.models,models);
 const snapshot=await readFile(coordinatorConfig);
 const duplicateCoordinator=await once(coordinator.exe,['--no-ui','--no-brain'],10000);assert.equal(duplicateCoordinator.code,0);
 const duplicateNode=await once(node.exe,['--no-ui'],10000);assert.equal(duplicateNode.code,0);
 assert.deepEqual(await readFile(coordinatorConfig),snapshot);assert.equal(JSON.parse(await readFile(nodeConfig,'utf8')).node_id,first.node_id);
 assert.equal(runningCoordinator.exitCode,null);assert.equal(runningNode.exitCode,null);
 assert.equal(await exists(join(cwd,'config/default_identity.yaml')),false);assert.equal(await exists(join(cwd,'config.json')),false);
 assert.deepEqual(await readdir(cwd),[],'No data or settings may be left in the unrelated working directory');
 for(const c of children)for(const key of [secret,otherSecret,privateSecret(initialized),paired.psk]){
  assert(typeof key==='string'&&key.length>=12);assert(!(c.logs().out+c.logs().err).includes(key),'Commands/services must never log generated credentials');
 }
 await writeFile(join(scratch,'results.json'),JSON.stringify({passed:true,managed_default_paths:true,unrelated_cwd_untouched:true,
  offline_initialize:true,missing_selected_package_not_executed:true,idempotent:true,existing_owner_bytes_preserved:true,no_generated_credentials_logged:true,
  invalid_configuration_preserved:true,invalid_legacy_secret_not_rotated:true,distinct_saved_credentials:true,
  normal_coordinator_first_creation:true,normal_node_first_creation:true,automatic_initial_registration:true,detected_hostname:true,stable_node_identity:true,
  explicit_private_enrollment:true,both_runtime_restarts_observed:true,permissions_and_models_preserved:true,duplicate_launch_preserved:true,
  default_ports_required:true,model_downloads:false,model_inference:false,automatic_private_enrollment:false,
  automatic_model_onboarding:false,physical_two_pc:false,msi_installation:false,stable_release_accepted:false},null,2));
 console.log('Managed first-run backend checks passed: '+scratch);
}catch(error){await writeFile(join(scratch,'PRIVATE-failure.json'),JSON.stringify({message:String(error)},null,2));await writeFile(join(scratch,'PRIVATE-logs.json'),JSON.stringify(children.map(c=>({exe:c.spawnfile,code:c.exitCode,...c.logs()})),null,2));throw error;}
finally{for(const c of children)if(c.exitCode===null)c.kill();await Promise.allSettled(children.map(c=>c.result));
 for(const path of links){assert(resolve(path).startsWith(resolve(scratch)+sep));await unlink(path);}}
