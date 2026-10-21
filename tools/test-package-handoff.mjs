// Native same-version handoff/rollback. Selectors are synthetic fixture input;
// this does NOT exercise HTTPS repair download or certify packaged dependencies.
import assert from 'node:assert/strict';
import {mkdtemp,mkdir,readFile,writeFile,link,unlink} from 'node:fs/promises';
import {join,resolve,dirname,sep} from 'node:path';
import {spawn} from 'node:child_process';
import {randomUUID} from 'node:crypto';
import {createServer} from 'node:net';
assert.equal(process.platform,'win32');
const root=resolve(import.meta.dirname,'..'),bin=resolve(process.env.EEF_TEST_BINARY_DIR||join(root,'target/debug'));
const scratch=await mkdtemp(join(root,'.validation/package-handoff-')),children=[],links=[];
const version=(await readFile(join(root,'Cargo.toml'),'utf8')).match(/^version\s*=\s*"([^"]+)"/m)[1];
const env={...process.env,EEF_NODE_PSK:randomUUID(),APPDATA:join(scratch,'appdata'),EEF_DISCOVERY_DIR:join(scratch,'discovery'),PATH:join(root,'.tooling/llvm-mingw-20260616-ucrt-x86_64/bin')+';'+process.env.PATH};
for(const key of Object.keys(env))if(['EEF_PYTHON','EEF_NODE_OLLAMA'].includes(key.toUpperCase()))delete env[key];
function owned(path){assert(resolve(path).startsWith(resolve(scratch)+sep));return path;}
async function alias(source,target){await mkdir(dirname(owned(target)),{recursive:true});await link(source,target);links.push(target);}
function launch(exe,args){const child=spawn(exe,args,{cwd:root,env,windowsHide:true,stdio:['ignore','pipe','pipe']});
 let out='',err='';child.stdout.on('data',b=>out=(out+b).slice(-1048576));child.stderr.on('data',b=>err=(err+b).slice(-1048576));
 child.logs=()=>({out,err});child.result=new Promise((r,j)=>{child.on('error',j);child.on('close',code=>r({code,out,err}));});children.push(child);return child;}
async function port(){const s=createServer();await new Promise(r=>s.listen(0,'127.0.0.1',r));const p=s.address().port;await new Promise(r=>s.close(r));return p;}
async function json(url,body){const r=await fetch(url,{method:body?'POST':'GET',headers:{'Content-Type':'application/json'},body:body?JSON.stringify(body):undefined,signal:AbortSignal.timeout(8000)});assert(r.ok);return r.json();}
async function until(fn,label){const end=Date.now()+25000;while(Date.now()<end){try{if(await fn())return;}catch{}await new Promise(r=>setTimeout(r,200));}throw Error('Timeout: '+label);}
function psLiteral(s){return s.replaceAll("'","''");}
async function processes(stop=false){const literal=psLiteral(resolve(scratch));
 const select=`Get-CimInstance Win32_Process | Where-Object { $_.Name -in @('eef.exe','eefn.exe') -and $_.ExecutablePath -and $_.ExecutablePath.StartsWith('${literal}\\',[StringComparison]::OrdinalIgnoreCase) }`;
 const script=stop?`${select} | ForEach-Object { Stop-Process -Id $_.ProcessId -Force -ErrorAction Stop }`:`$items=@(${select} | Select-Object ProcessId,ExecutablePath); ConvertTo-Json -InputObject $items -Compress`;
 const result=await launch('powershell.exe',['-NoProfile','-NonInteractive','-Command',script]).result;
 assert.equal(result.code,0,result.err);return stop?[]:JSON.parse(result.out);}
async function hasPath(path){return (await processes()).some(p=>p.ExecutablePath.toLowerCase()===resolve(path).toLowerCase());}
try {
 for(const role of ['eefn','eef']){
  const install=join(scratch,role),slot=version+'.repair-'+randomUUID(),bundle=join(install,'versions',slot),rootExe=join(install,role+'.exe'),selectedExe=join(bundle,role+'.exe');
  await alias(join(bin,role+'.exe'),rootExe);await alias(join(bin,role+'.exe'),selectedExe);
  const api=await port(),config=join(install,role==='eefn'?'node.json':'eef.yaml'),url=`http://127.0.0.1:${api}`;
  const feed=join(install,'stable-fixture.json').replaceAll('\\','/');
  await writeFile(feed,JSON.stringify({version:'99.0.0',url:'https://example.invalid/must-not-download',sha256:'0'.repeat(64),size_bytes:1}));
  await writeFile(join(install,'current.txt'),version);
  if(role==='eefn'){
   const value=JSON.parse(await readFile(join(root,'config/node.example.json'),'utf8'));
   Object.assign(value,{node_id:'repair-handoff-node',name:'Repair handoff fixture',auto_local:false,local_pairing:false,connection_enabled:false,endpoints:[],psk:'',dashboard:{enabled:true,ui_enabled:false,host:'127.0.0.1',port:api},update:{policy:'off',manifest_url:feed},models:{provider:'ollama',ollama:{base_url:'http://127.0.0.1:1',selected:[]}}});
   await writeFile(config,JSON.stringify(value));
  }else{
   const gateway=await port();const yaml=(await readFile(join(root,'config/default_identity.yaml'),'utf8')).replaceAll('host: 0.0.0.0','host: 127.0.0.1').replace('port: 51334',`port: ${api}`).replace('port: 51335',`port: ${gateway}`).replace('policy: prompt','policy: off').replace(/manifest_url: [^\r\n]+/,'manifest_url: '+JSON.stringify(feed));await writeFile(config,yaml);
  }
  const args=['--config',config,'--no-ui',...(role==='eef'?['--database',join(install,'eef.db'),'--no-brain']:[])];
  launch(rootExe,args);await until(async()=>{await json(url+'/api/diagnostics');return hasPath(rootExe);},role+' initial runtime');
  const saved=await readFile(config),selector=join(install,'versions',version+'.bundle.json');
  await writeFile(selector,JSON.stringify({schema_version:1,version,slot,previous_slot:'$root'}));
  const pending=await launch(rootExe,['--config',join(install,'absent-config.json'),'--json','repair','--status']).result;
  assert.equal(pending.code,0,pending.out+pending.err);assert.equal(JSON.parse(pending.out).handoff_required_for_invoked_executable,true);
  assert.equal(JSON.parse(pending.out).application_restart_required,null,'offline bootstrap is not a live-service health check');
  const supersede=await fetch(url+'/api/update/apply',{method:'POST',headers:{'Content-Type':'application/json'},body:'{}',signal:AbortSignal.timeout(10000)});
  assert.equal(supersede.ok,false);assert.match(await supersede.text(),/repair is pending/,'ordinary update must not supersede an uninspected repair');
  assert.equal(JSON.parse(await readFile(selector,'utf8')).slot,slot);
  if(role==='eef')assert.equal((await json(url+'/api/update/status')).restart_required,true,'service must notice an offline selection');
  await json(url+'/api/restart',{});
  await until(async()=>{await json(url+'/api/diagnostics');return await hasPath(selectedExe)&&!(await hasPath(rootExe));},role+' same-version handoff');
  assert.deepEqual(await readFile(config),saved,'handoff must preserve owner config');
  // A broken pending target must not prevent the local rollback command.
  await writeFile(selector,JSON.stringify({schema_version:1,version,slot:version+'.repair-'+randomUUID(),previous_slot:'$root'}));
  const rollback=await launch(rootExe,['--config',config,'--json','repair','--rollback']).result;
  assert.equal(rollback.code,0,rollback.out+rollback.err);assert.equal(JSON.parse(rollback.out).report_type,'package_rollback');
  assert.equal(JSON.parse(await readFile(selector,'utf8')).slot,'$root');
  await json(url+'/api/restart',{});
  await until(async()=>{await json(url+'/api/diagnostics');return await hasPath(rootExe)&&!(await hasPath(selectedExe));},role+' rollback handoff');
  assert.deepEqual(await readFile(config),saved,'rollback must preserve owner config');
  const settled=await launch(rootExe,['--config',join(install,'absent-config.json'),'--json','repair','--status']).result;
  assert.equal(settled.code,0,settled.out+settled.err);assert.equal(JSON.parse(settled.out).handoff_required_for_invoked_executable,false);
  const noConsent=await launch(rootExe,['--json','repair','--url','https://example.invalid/never','--sha256','a'.repeat(64),'--size-bytes','1']).result;
  assert.equal(noConsent.code,1,'prerelease repair needs explicit consent');
  assert.equal(JSON.parse(await readFile(selector,'utf8')).slot,'$root');
  await processes(true);
 }
 await writeFile(join(scratch,'results.json'),JSON.stringify({passed:true,roles:['eef','eefn'],same_version_restart_handoff:true,rollback_bypasses_incomplete_target:true,ordinary_update_cannot_supersede_pending_repair:true,config_preserved:true,prerelease_requires_explicit_consent:true,synthetic_selection:true,https_repair_download:false,physical_two_pc:false},null,2));
 console.log('Package handoff native checks passed: '+scratch);
}catch(error){await writeFile(join(scratch,'observations.json'),JSON.stringify({passed:false,error:String(error),processes:(await processes()).map(p=>({path:p.ExecutablePath.slice(scratch.length),pid:p.ProcessId}))},null,2));await writeFile(join(scratch,'PRIVATE-failure.json'),JSON.stringify({message:String(error)},null,2));await writeFile(join(scratch,'PRIVATE-logs.json'),JSON.stringify(children.map(c=>({exe:c.spawnfile,code:c.exitCode,...c.logs()})),null,2));throw error;}
finally{await processes(true);for(const c of children)if(c.exitCode===null)c.kill();await Promise.allSettled(children.map(c=>c.result));assert.equal((await processes()).length,0);for(const path of links)await unlink(owned(path));}
