// Service startup reads setup evidence before handoff, identity writes or listeners.
// Every process and file belongs to this private fixture. No models or real nodes.
import assert from 'node:assert/strict';
import {mkdtemp,readFile,writeFile,copyFile,link,mkdir,rm,rmdir,access,realpath,readdir} from 'node:fs/promises';
import {join,resolve,toNamespacedPath} from 'node:path';
import {tmpdir} from 'node:os';
import {spawn} from 'node:child_process';
import {createServer} from 'node:net';
import {createHash,randomBytes} from 'node:crypto';
import {readLoopbackCoordinatorTemplate,coordinatorTemplatePorts} from './local-test-network.mjs';

assert.equal(process.platform,'win32');
const repository=resolve(import.meta.dirname,'..');
const bin=resolve(process.env.EEF_TEST_BINARY_DIR||join(repository,'target/debug'));
const scratch=await mkdtemp(join(process.env.EEF_TEST_SCRATCH_PARENT||tmpdir(),'eef-setup-startup-'));
const children=[],cases=[];
const env={...process.env,RUST_LOG:'info',APPDATA:join(scratch,'appdata'),LOCALAPPDATA:join(scratch,'localappdata'),EEF_DISCOVERY_DIR:join(scratch,'discovery'),EEF_NODE_PSK:randomBytes(32).toString('hex'),PATH:join(repository,'.tooling/llvm-mingw-20260616-ucrt-x86_64/bin')+';'+process.env.PATH};
const hash=bytes=>createHash('sha256').update(bytes).digest('hex');
function frame(record){const body=Buffer.from(JSON.stringify(record)),length=Buffer.alloc(4);length.writeUInt32LE(body.length);return Buffer.concat([length,body,createHash('sha256').update(body).digest()]);}
function launch(exe,args){const c=spawn(exe,args,{cwd:scratch,env,windowsHide:true,stdio:['ignore','pipe','pipe']});let out='',err='';
 c.stdout.on('data',b=>out=(out+b).slice(-1048576));c.stderr.on('data',b=>err=(err+b).slice(-1048576));c.logs=()=>({out,err});
 c.result=new Promise((r,j)=>{c.once('error',j);c.once('close',(code,signal)=>r({code,signal,out,err}));});c.result.catch(()=>{});children.push(c);return c;}
async function once(exe,args){const c=launch(exe,args),timer=setTimeout(()=>c.kill(),10000);try{return await c.result;}finally{clearTimeout(timer);}}
async function port(){const s=createServer();await new Promise((r,j)=>{s.once('error',j);s.listen(0,'127.0.0.1',r);});const p=s.address().port;await new Promise(r=>s.close(r));return p;}
async function absent(path){await assert.rejects(()=>access(path),{code:'ENOENT'});}
async function diagnostics(url){const response=await fetch(url+'/api/diagnostics',{signal:AbortSignal.timeout(500)});assert(response.ok);return response.json();}
async function ready(child,url){const until=Date.now()+20000;while(Date.now()<until){if(child.exitCode!==null||child.signalCode)throw Error('Service exited: '+JSON.stringify(await child.result));try{if(typeof (await diagnostics(url)).runtime_id==='string')return;}catch{}await new Promise(r=>setTimeout(r,75));}throw Error('Service never became ready: '+JSON.stringify(child.logs()));}
let phase='prepare',role;
try{
 for(role of ['eef','eefn']){
  const installation=join(scratch,role);await mkdir(installation);const exe=join(installation,role+'.exe');await copyFile(join(bin,role+'.exe'),exe);
  const versionResult=await once(exe,['--version']);assert.equal(versionResult.code,0);
  const version=versionResult.out.trim().split(/\s+/).at(-1);
  // Metadata-only doctor needs a managed package identity; no runtime is supplied or launched.
  await writeFile(join(installation,'bundle.json'),JSON.stringify({name:role,version,architecture:'x86_64-windows',python:'3.11.9-embedded',...(role==='eefn'?{llamacpp:'b10621'}:{})}));
  const binaryHash=hash(await readFile(exe)),config=join(installation,role==='eef'?'identity.yaml':'config.json'),db=join(installation,'state.db');
  const api=await port();let gateway;do{gateway=await port();}while(api===gateway);const url=`http://127.0.0.1:${api}`;
  if(role==='eef')await writeFile(config,coordinatorTemplatePorts(await readLoopbackCoordinatorTemplate(),api,gateway).replace('policy: prompt','policy: off'));
  else{const value=JSON.parse(await readFile(join(repository,'config/node.example.json'),'utf8'));Object.assign(value,{node_id:'setup-startup-fixture',name:'Setup startup fixture',auto_local:false,local_pairing:false,endpoints:[],dashboard:{enabled:true,ui_enabled:false,host:'127.0.0.1',port:api},update:{policy:'off'},models:{provider:'llamacpp',llamacpp:{slots:[]}}});await writeFile(config,JSON.stringify(value));}
  const original=await readFile(config),journal=join(installation,'.setup-transaction'),selector=join(installation,'current.txt');
  const args=['--config',config,'--no-ui',...(role==='eef'?['--no-brain','--database',db]:[])];
  const header={kind:'header',header:{version:1,transaction_id:'0123456789abcdef0123456789abcdef',root:toNamespacedPath(await realpath(installation)),role,external:[]}};
  const commit={kind:'committed'};
  const wrongRole=structuredClone(header);wrongRole.header.role=role==='eef'?'eefn':'eef';
  const future=structuredClone(header);future.header.version=2;
  const corrupt=Buffer.concat([frame(header),frame(commit)]);corrupt[corrupt.length-1]^=1;
  const blocked=[['uncommitted',frame(header)],['partial-tail',Buffer.concat([frame(header),Buffer.from([20,0,0,0,123])])],['corrupt-checksum',corrupt],['unknown-version',frame(future)],['wrong-role',frame(wrongRole)],['malformed',Buffer.from('not a setup journal')],['directory',null]];
  for(const [name,bytes] of blocked){phase=name;if(bytes===null)await mkdir(journal);else await writeFile(journal,bytes);
   const before=(await readdir(installation)).sort();
   const result=await once(exe,args);assert.equal(result.code,1,`${role}/${name}: setup must block service startup; ${JSON.stringify(result)}`);assert.match(result.err,/matching installer/);
   assert.deepEqual(await readFile(config),original);await absent(db);assert.deepEqual((await readdir(installation)).sort(),before);await assert.rejects(()=>diagnostics(url));
   if(bytes!==null)assert.deepEqual(await readFile(journal),bytes);
   // Offline help/version/inspection remain usable with the same pending journal.
   if(name==='malformed'){
    for(const command of [['--help'],['--version']])assert.equal((await once(exe,command)).code,0);
    const doctor=await once(exe,['--config',config,'--json','doctor']);assert.equal(doctor.code,0);assert.equal(JSON.parse(doctor.out).probes_requested,false);
    assert.deepEqual(await readFile(journal),bytes);assert.deepEqual(await readFile(config),original);
    assert.deepEqual((await readdir(installation)).sort(),before);
   }
   if(bytes===null)await rmdir(journal);else await rm(journal);
   cases.push({role,case:name,blocked_before_configuration_database_and_api:true});
  }
  phase='before-selected-handoff';await writeFile(journal,frame(header));await writeFile(selector,'99.0.0');const selected=await once(exe,args);assert.equal(selected.code,1);assert.match(selected.err,/Setup did not finish/);assert.deepEqual(await readFile(config),original);
  cases.push({role,case:phase,blocked_before_handoff:true});
  phase='direct-versioned-launch';const versioned=join(installation,'versions/99.0.0',role+'.exe');await mkdir(join(installation,'versions/99.0.0'),{recursive:true});await link(exe,versioned);
  const direct=await once(versioned,args);assert.equal(direct.code,1);assert.match(direct.err,/Setup did not finish/);assert.deepEqual(await readFile(config),original);await absent(db);await assert.rejects(()=>diagnostics(url));await rm(selector);await rm(journal);
  cases.push({role,case:phase,installation_root_journal_checked:true});
  phase='committed-cleanup';
  // A valid no-op committed journal can remain if journal deletion failed.
  // Its partial next append must not turn committed setup back into interrupted setup.
  const committed=Buffer.concat([frame(header),frame(commit),Buffer.from([2,0])]);await writeFile(journal,committed);
  const backup=join(installation,'.eef-setup-owner-fixture.backup');await writeFile(backup,'owner recovery evidence');
  const service=launch(exe,args);await ready(service,url);assert.match(service.logs().err,/backup cleanup is pending/);
  assert.deepEqual(await readFile(journal),committed);assert.equal(await readFile(backup,'utf8'),'owner recovery evidence');service.kill();await service.result;
  cases.push({role,case:phase,service_ready:true,journal_and_backup_untouched:true,binary_sha256:binaryHash});
 }
 await writeFile(join(scratch,'results.json'),JSON.stringify({passed:true,cases,synthetic_package_metadata:true,physical_two_pc:false,installer_recovery_performed:false,already_running_service_guarded:false,model_run:false},null,2));
 console.log('Setup startup validation passed: '+scratch);
}catch(error){await writeFile(join(scratch,'PRIVATE-failure.json'),JSON.stringify({error:String(error),phase,role,processes:children.map(c=>({pid:c.pid,exit_code:c.exitCode,signal:c.signalCode,...c.logs()}))},null,2));console.error('Setup startup validation failed: '+scratch);throw error;}
finally{for(const c of children)if(c.exitCode===null&&!c.signalCode)c.kill();await Promise.allSettled(children.map(c=>c.result));}
