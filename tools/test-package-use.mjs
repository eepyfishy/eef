// Real versioned services, separate profiles and a tiny synthetic setup payload.
// No models, production installation, firewall changes or external nodes.
import assert from 'node:assert/strict';
import {mkdtemp,mkdir,readFile,writeFile,copyFile,access,realpath} from 'node:fs/promises';
import {join,resolve} from 'node:path';
import {tmpdir} from 'node:os';
import {spawn} from 'node:child_process';
import {createServer} from 'node:net';
import {randomBytes} from 'node:crypto';
import {readLoopbackCoordinatorTemplate,coordinatorTemplatePorts} from './local-test-network.mjs';
assert.equal(process.platform,'win32');
const repository=resolve(import.meta.dirname,'..'),bin=resolve(process.env.EEF_TEST_BINARY_DIR||join(repository,'target/debug'));
const scratch=await mkdtemp(join(process.env.EEF_TEST_SCRATCH_PARENT||tmpdir(),'eef-package-use-'));
const downloads=join(scratch,'downloads');await mkdir(downloads);
const children=[],cases=[];
const env={...process.env,RUST_LOG:'info',APPDATA:join(scratch,'appdata'),LOCALAPPDATA:join(scratch,'localappdata'),EEF_DISCOVERY_DIR:join(scratch,'discovery'),EEF_NODE_PSK:randomBytes(32).toString('hex'),PATH:join(repository,'.tooling/llvm-mingw-20260616-ucrt-x86_64/bin')+';'+process.env.PATH};
function launch(exe,args,extra={}){
 const child=spawn(exe,args,{cwd:scratch,env:{...env,...extra},windowsHide:true,stdio:['ignore','pipe','pipe']});
 let out='',err='';child.stdout.on('data',b=>out=(out+b).slice(-1048576));child.stderr.on('data',b=>err=(err+b).slice(-1048576));
 child.logs=()=>({out,err});child.result=new Promise((r,j)=>{child.once('error',j);child.once('close',code=>r({code,out,err}));});children.push(child);return child;
}
async function once(exe,args,extra){const child=launch(exe,args,extra),timer=setTimeout(()=>child.kill(),45000);try{return await child.result;}finally{clearTimeout(timer);}}
async function port(){const server=createServer();await new Promise((r,j)=>{server.once('error',j);server.listen(0,'127.0.0.1',r);});const value=server.address().port;await new Promise(r=>server.close(r));return value;}
async function ready(child,url){const deadline=Date.now()+25000;while(Date.now()<deadline){
 if(child.exitCode!==null||child.signalCode)throw Error('Service exited: '+JSON.stringify(await child.result));
 try{const response=await fetch(url+'/api/diagnostics',{signal:AbortSignal.timeout(500)});if(response.ok&&typeof (await response.json()).runtime_id==='string')return;}catch{}
 await new Promise(r=>setTimeout(r,75));
}throw Error('Service not ready: '+JSON.stringify(child.logs()));}
async function absent(path){await assert.rejects(()=>access(path),{code:'ENOENT'});}
const script=join(scratch,'make-fixture.ps1');
await writeFile(script,`param([string]$Payload,[string]$Stub,[string]$Builder,[string]$Zip,[string]$Output)
$ErrorActionPreference='Stop'
Add-Type -AssemblyName System.IO.Compression.FileSystem
$tokens=$null;$errors=$null
$ast=[Management.Automation.Language.Parser]::ParseFile($Builder,[ref]$tokens,[ref]$errors)
if($errors.Count){throw 'Bundle helper syntax error'}
$function=$ast.Find({param($node) $node -is [Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -ceq 'New-SelfExtractingInstaller'},$false)
if(-not $function){throw 'Missing installer framing helper'}
. ([scriptblock]::Create($function.Extent.Text))
[IO.Compression.ZipFile]::CreateFromDirectory($Payload,$Zip)
New-SelfExtractingInstaller -Stub $Stub -Payload $Zip -Output $Output
`);
let role,phase='prepare';
try{
 for(role of ['eef','eefn']){
  const source=join(bin,role+'.exe'),versionResult=await once(source,['--version']);assert.equal(versionResult.code,0);
  const version=versionResult.out.trim().split(/\s+/).at(-1),installation=join(scratch,role),versioned=join(installation,'versions',version);
  await mkdir(versioned,{recursive:true});await copyFile(source,join(versioned,role+'.exe'));
  const metadata=JSON.stringify({name:role,version,architecture:'x86_64-windows',python:'3.11.9-embedded',...(role==='eefn'?{llamacpp:'b10621'}:{})});
  await writeFile(join(versioned,'bundle.json'),metadata);await writeFile(join(installation,'bundle.json'),metadata);
  const old=Buffer.from('root bootstrap is not the running image');await writeFile(join(installation,role+'.exe'),old);
  const payload=join(scratch,role+'-payload'),installer=join(downloads,role+'-fixture-installer.exe');await mkdir(payload);
  await writeFile(join(payload,'bundle.json'),metadata);await writeFile(join(payload,role+'.exe'),'synthetic replacement, never launched');
  const defaultConfig=role==='eef'?'config/default_identity.yaml':'config.json';
  if(role==='eef')await mkdir(join(payload,'config'));
  await writeFile(join(payload,defaultConfig),'{}');
  const built=await once('powershell.exe',['-NoProfile','-NonInteractive','-File',script,payload,join(bin,'eef-installer-stub.exe'),join(repository,'tools/bundle-windows.ps1'),join(scratch,role+'-payload.zip'),installer]);assert.equal(built.code,0,built.out+built.err);
  const active=[],profiles=[];
  for(let i=0;i<2;i++){
   const config=join(installation,'profile-'+i+(role==='eef'?'.yaml':'.json')),api=await port();let gateway;do{gateway=await port();}while(gateway===api);
   if(role==='eef')await writeFile(config,coordinatorTemplatePorts(await readLoopbackCoordinatorTemplate(),api,gateway).replace('policy: prompt','policy: off'));
   else{const value=JSON.parse(await readFile(join(repository,'config/node.example.json'),'utf8'));Object.assign(value,{node_id:'package-use-'+i,name:'Package use fixture',auto_local:false,local_pairing:false,connection_enabled:false,endpoints:[],dashboard:{enabled:true,ui_enabled:false,host:'127.0.0.1',port:api},update:{policy:'off'},models:{provider:'llamacpp',llamacpp:{slots:[]}}});await writeFile(config,JSON.stringify(value));}
   const args=['--config',config,'--no-ui',...(role==='eef'?['--no-brain','--database',join(installation,'profile-'+i+'.db')]:[])];
   phase=role+'/start-profile-'+i;const child=launch(join(versioned,role+'.exe'),args);await ready(child,'http://127.0.0.1:'+api);active.push(child);profiles.push({config,bytes:await readFile(config),url:'http://127.0.0.1:'+api});
  }
  assert.equal(await realpath(join(installation,'.package-use.lock')),join(await realpath(installation),'.package-use.lock'));
  for(let users=2;users>=1;users--){
   phase=role+'/setup-refusal-'+users;
   const errorLog=join(scratch,role+'-refusal-'+users+'.txt');
   const result=await once(installer,['--quiet','--install-dir',installation,'--no-startup','--no-launch'],{EEF_INSTALLER_ERROR_LOG:errorLog});
   assert.equal(result.code,1,result.out+result.err);assert.match(await readFile(errorLog,'utf8'),/exclusive installation access/);
   assert.deepEqual(await readFile(join(installation,role+'.exe')),old);assert.equal(await readFile(join(installation,'bundle.json'),'utf8'),metadata);
   await absent(join(installation,'.setup-transaction'));await absent(join(installation,defaultConfig));
   for(const profile of profiles)assert.deepEqual(await readFile(profile.config),profile.bytes);
   await ready(active.at(-1),profiles.at(-1).url);
   cases.push({role,active_versioned_profiles:users,setup_refused_before_payload_or_configuration_changes:true});
   const child=active.shift();child.kill();await child.result;
  }
  phase=role+'/setup-after-last-exit';
  const final=await once(installer,['--quiet','--install-dir',installation,'--no-startup','--no-launch'],{EEF_INSTALLER_ERROR_LOG:join(scratch,role+'-final-error.txt')});
  assert.equal(final.code,0,final.out+final.err);assert.equal(await readFile(join(installation,role+'.exe'),'utf8'),'synthetic replacement, never launched');await absent(join(installation,'.setup-transaction'));
  cases.push({role,active_versioned_profiles:0,setup_succeeded_after_last_exit:true});
 }
 await writeFile(join(scratch,'results.json'),JSON.stringify({passed:true,cases,synthetic_setup_payload:true,real_versioned_services:true,models_run:false,firewall_changed:false,physical_two_pc:false,production_installer_acceptance:false},null,2));
 console.log('Package lifetime validation passed: '+scratch);
}catch(error){await writeFile(join(scratch,'PRIVATE-failure.json'),JSON.stringify({error:String(error),role,phase,processes:children.map(c=>({pid:c.pid,exit_code:c.exitCode,signal:c.signalCode,...c.logs()}))},null,2));console.error('Package lifetime validation failed: '+scratch);throw error;}
finally{for(const child of children)if(child.exitCode===null&&!child.signalCode)child.kill();await Promise.allSettled(children.map(c=>c.result));}
