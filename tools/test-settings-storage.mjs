// Real owner APIs with native Windows delete locks; no user files/models/devices.
import assert from 'node:assert/strict';
import {mkdtemp,readFile,writeFile,readdir} from 'node:fs/promises';
import {join,resolve,sep} from 'node:path';
import {spawn} from 'node:child_process';
import {createServer} from 'node:net';
import {randomBytes} from 'node:crypto';
assert.equal(process.platform,'win32','Windows sharing-lock acceptance only');
const root=resolve(import.meta.dirname,'..'),bin=resolve(process.env.EEF_TEST_BINARY_DIR||join(root,'target/debug'));
const scratch=await mkdtemp(join(root,'.validation/settings-storage-')),children=[];
const config=join(scratch,'node.json'),identity=join(scratch,'eef.yaml');
const env={...process.env,EEF_NODE_PSK:'',EEF_NODE_OLLAMA:'',APPDATA:join(scratch,'appdata'),EEF_DISCOVERY_DIR:join(scratch,'discovery'),PATH:join(root,'.tooling/llvm-mingw-20260616-ucrt-x86_64/bin')+';'+process.env.PATH};
function owned(path){assert(resolve(path).startsWith(resolve(scratch)+sep),'Only this newly created fixture directory is mutable');return path;}
function start(exe,args,input=false,extra={}){
 const c=spawn(exe,args,{cwd:root,windowsHide:true,env:{...env,...extra},stdio:[input?'pipe':'ignore','pipe','pipe']});let out='',err='';
 c.stdout.on('data',b=>out=(out+b).slice(-1048576));c.stderr.on('data',b=>err=(err+b).slice(-1048576));c.logs=()=>({out,err});
 c.result=new Promise((r,j)=>{c.on('error',j);c.on('close',code=>r({code,out,err}));});c.result.catch(()=>{});children.push(c);if(input)c.stdin.on('error',()=>{});return c;
}
async function until(fn,label){const end=Date.now()+30000;while(Date.now()<end){try{if(await fn())return;}catch{}await new Promise(r=>setTimeout(r,100));}throw Error('Timeout: '+label);}
async function json(url){const r=await fetch(url,{signal:AbortSignal.timeout(10000)});assert(r.ok,'Read failed; private evidence '+scratch);return r.json();}
async function save(url,value,ok){const r=await fetch(url+'/api/config',{method:'PUT',headers:{'Content-Type':'application/json'},body:JSON.stringify({config:value}),signal:AbortSignal.timeout(10000)});assert.equal(r.ok,ok,'Save confirmation mismatch; private evidence '+scratch);await r.arrayBuffer();}
async function port(){const s=createServer();await new Promise(r=>s.listen(0,'127.0.0.1',r));const p=s.address().port;await new Promise(r=>s.close(r));return p;}
async function cli(name,path,args){const c=start(join(bin,name+'.exe'),['--config',path,'--json',...args]);const timer=setTimeout(()=>c.kill(),70000);try{const r=await c.result;assert.equal(r.code,0,'CLI failed; private evidence '+scratch);return JSON.parse(r.out);}finally{clearTimeout(timer);}}
async function locked(path,operation){
 const c=start(join(bin,'examples/settings-storage-fixture.exe'),[owned(path)],true);
 try{await until(()=>c.logs().out.includes('"locked":true'),'native file lock');await operation();}
 finally{c.stdin.end('\n');const timer=setTimeout(()=>c.kill(),10000);try{const result=await c.result;assert.equal(result.code,0,'Lock helper exit unconfirmed');}finally{clearTimeout(timer);}}
}
async function blocked(url,path,backup,edit,lockBackup){
 const before=await readFile(owned(path)),backupBefore=await readFile(owned(backup));
 const value=(await json(url+'/api/config')).config;edit(value);
 await locked(lockBackup?backup:path,async()=>{await save(url,value,false);assert.deepEqual(await readFile(path),before);if(lockBackup)assert.deepEqual(await readFile(backup),backupBefore);});
 assert.deepEqual(await readFile(path),before);
 assert(!(await readdir(scratch)).some(name=>name.startsWith('.eef-write-')),'Failed publication must clean its own temporary files');
 // The same requested edit succeeds after release; no process/app restart is
 // needed to recover the writer. Application of saved settings stays separate.
 await save(url,value,true);
}
try{
 const [api,web,gateway]=await Promise.all([port(),port(),port()]),node=`http://127.0.0.1:${api}`,eef=`http://127.0.0.1:${web}`;
 const value=JSON.parse(await readFile(join(root,'config/node.example.json'),'utf8'));
 Object.assign(value,{node_id:'settings-storage-fixture',name:'Original node',auto_local:false,local_pairing:false,connection_enabled:false,endpoints:[],psk:'',dashboard:{enabled:true,ui_enabled:false,host:'127.0.0.1',port:api},update:{policy:'off'},models:{provider:'ollama',ollama:{base_url:'http://127.0.0.1:1',selected:[]},llamacpp:{slots:[]}}});
 await writeFile(owned(config),JSON.stringify(value));
 const yaml=(await readFile(join(root,'config/default_identity.yaml'),'utf8')).replace('port: 51334',`port: ${web}`).replace('port: 51335',`port: ${gateway}`).replace('policy: prompt','policy: off');await writeFile(owned(identity),yaml);
 const sentinel=Buffer.from('Existing sibling belongs to its owner, not to the next save');
 for(const path of [config+'.next',identity+'.next'])await writeFile(owned(path),sentinel);
 start(join(bin,'eefn.exe'),['--config',config,'--no-ui']);
 start(join(bin,'eef.exe'),['--config',identity,'--database',join(scratch,'eef.db'),'--no-brain','--no-ui'],false,{EEF_NODE_PSK:randomBytes(24).toString('hex')});
 await until(async()=>(await json(node+'/api/diagnostics')).runtime_id&&(await json(eef+'/api/diagnostics')).initialized,'core apps without models');
 const nodeValue=(await json(node+'/api/config')).config;nodeValue.name='First saved node';await save(node,nodeValue,true);
 const eefValue=(await json(eef+'/api/config')).config;eefValue.identity.name='First saved EEF';await save(eef,eefValue,true);
 await cli('eefn',config,['restart','--wait-seconds','60']);
 assert.equal((await json(node+'/api/status')).pending_restart,false);
 const originalRuntime=(await json(node+'/api/diagnostics')).runtime_id;
 await locked(config,async()=>{
  const before=await readFile(config),next=(await json(node+'/api/config')).config;next.name='Refused node name';
  await save(node,next,false);assert.deepEqual(await readFile(config),before);assert.equal((await json(node+'/api/status')).pending_restart,false);
 });
 await blocked(node,config,config+'.bak',value=>value.name='Node writer recovered',false);
 await blocked(node,config,config+'.bak',value=>value.name='Node backup writer recovered',true);
 await blocked(eef,identity,identity+'.bak',value=>value.identity.name='EEF writer recovered',false);
 await blocked(eef,identity,identity+'.bak',value=>value.identity.name='EEF backup writer recovered',true);
 assert.equal((await json(node+'/api/diagnostics')).runtime_id,originalRuntime,'Saving is not automatic application/restart');
 for(const path of [config+'.next',identity+'.next'])assert.deepEqual(await readFile(path),sentinel);
 assert(!(await readdir(scratch)).some(name=>name.startsWith('.eef-write-')));
 await writeFile(owned(join(scratch,'results.json')),JSON.stringify({passed:true,physical_two_pc:false,synthetic_configuration_only:true,real_disk_exhaustion:false,power_loss_tested:false,
  real_windows_delete_locks:true,node_primary_and_backup_refused_without_overwrite:true,coordinator_primary_and_backup_refused_without_overwrite:true,
  failed_node_save_does_not_set_pending_restart:true,stale_next_siblings_preserved:true,writer_recovers_after_lock_release:true,temporary_files_cleaned:true,no_automatic_restart:true},null,2));
 console.log('Atomic settings native validation passed: '+scratch);
}catch(error){await writeFile(owned(join(scratch,'PRIVATE-failure.json')),JSON.stringify({message:String(error)},null,2));await writeFile(owned(join(scratch,'PRIVATE-logs.json')),JSON.stringify(children.map(c=>({exe:c.spawnfile,code:c.exitCode,...c.logs()})),null,2));throw error;}
finally{for(const c of children)if(c.exitCode===null)c.kill();await Promise.allSettled(children.map(c=>c.result));}
