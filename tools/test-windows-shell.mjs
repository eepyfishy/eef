// Real PowerShell command I/O/exit propagation, including selected-package handoff.
// Isolated services only: no browser, model, owner config or physical node.
import assert from 'node:assert/strict';
import {mkdtemp,mkdir,readFile,writeFile,link,unlink,open} from 'node:fs/promises';
import {join,resolve,dirname,sep} from 'node:path';
import {spawn} from 'node:child_process';
import {randomUUID,randomBytes} from 'node:crypto';
import {createServer} from 'node:net';
assert.equal(process.platform,'win32');
const root=resolve(import.meta.dirname,'..'),bin=resolve(process.env.EEF_TEST_BINARY_DIR||join(root,'target/debug'));
const scratch=await mkdtemp(join(root,'.validation/windows-shell-')),children=[],links=[],cases=[],quietCases=[];
const version=(await readFile(join(root,'Cargo.toml'),'utf8')).match(/^version\s*=\s*"([^"]+)"/m)[1];
const env={...process.env,EEF_NODE_PSK:randomBytes(32).toString('hex'),APPDATA:join(scratch,'appdata'),
 EEF_DISCOVERY_DIR:join(scratch,'discovery'),PATH:join(root,'.tooling/llvm-mingw-20260616-ucrt-x86_64/bin')+';'+process.env.PATH};
const literal=s=>"'"+s.replaceAll("'","''")+"'";
function owned(path){assert(resolve(path).startsWith(resolve(scratch)+sep));return path;}
async function alias(source,target){await mkdir(dirname(owned(target)),{recursive:true});await link(source,target);links.push(target);}
function launch(exe,args){const c=spawn(exe,args,{cwd:scratch,env,windowsHide:true,stdio:['ignore','pipe','pipe']});let out='',err='';
 c.stdout.on('data',b=>out=(out+b).slice(-1048576));c.stderr.on('data',b=>err=(err+b).slice(-1048576));c.logs=()=>({out,err});
 c.result=new Promise((r,j)=>{c.on('error',j);c.on('close',code=>r({code,out,err}));});children.push(c);return c;}
async function port(){const s=createServer();await new Promise(r=>s.listen(0,'127.0.0.1',r));const p=s.address().port;await new Promise(r=>s.close(r));return p;}
async function text(path){const b=await readFile(path);return b[0]===255&&b[1]===254?b.subarray(2).toString('utf16le'):b.toString('utf8').replace(/^\uFEFF/,'');}
async function json(url){const r=await fetch(url,{signal:AbortSignal.timeout(8000)});assert(r.ok);return r.json();}
async function until(fn,label){const end=Date.now()+30000;while(Date.now()<end){try{if(await fn())return;}catch{}await new Promise(r=>setTimeout(r,100));}throw Error('Timeout: '+label);}
async function subsystem(exe){const file=await open(exe,'r');try{const dos=Buffer.alloc(64);await file.read(dos,0,64,0);assert.equal(dos.toString('ascii',0,2),'MZ');const pe=Buffer.alloc(96);await file.read(pe,0,96,dos.readUInt32LE(60));assert.equal(pe.toString('ascii',0,4),'PE\0\0');return pe.readUInt16LE(92);}finally{await file.close();}}
async function shell(exe,args,label,input){
 const output=join(scratch,label+'.stdout'),error=join(scratch,label+'.stderr');
 const script=`$global:LASTEXITCODE=$null; ${input===undefined?'':literal(input)+' | '}& ${literal(exe)} ${args.map(literal).join(' ')} 1> ${literal(output)} 2> ${literal(error)}; $result=$LASTEXITCODE; [ordered]@{exit_code=$result;completed=$true} | ConvertTo-Json -Compress`;
 const c=launch('powershell.exe',['-NoProfile','-NonInteractive','-Command',script]),timer=setTimeout(()=>c.kill(),30000);
 const result=await c.result.finally(()=>clearTimeout(timer));assert.equal(result.code,0,'PowerShell observation failed; private '+scratch);
 const report=JSON.parse(result.out);assert.equal(report.completed,true);assert.equal(typeof report.exit_code,'number','Shell must wait and observe an exit code');
 const out=await text(output),err=await text(error);cases.push({label,exit_code:report.exit_code});return {code:report.exit_code,out,err};
}
async function processes(stop=false){const prefix=literal(resolve(scratch)+sep);
 const select=`Get-CimInstance Win32_Process | Where-Object { $_.Name -in @('eef.exe','eefn.exe') -and $_.ExecutablePath -and $_.ExecutablePath.StartsWith(${prefix},[StringComparison]::OrdinalIgnoreCase) }`;
 const script=stop?`${select} | ForEach-Object { Stop-Process -Id $_.ProcessId -Force -ErrorAction Stop }`:`$items=@(${select} | Select-Object ProcessId,ExecutablePath); ConvertTo-Json -InputObject $items -Compress`;
 const r=await launch('powershell.exe',['-NoProfile','-NonInteractive','-Command',script]).result;assert.equal(r.code,0);return stop?[]:JSON.parse(r.out);}
async function quietService(role,source,oldApi,oldGateway){
 const install=join(scratch,role+'-quiet'),exe=join(install,role+'.exe');await alias(join(bin,role+'.exe'),exe);
 const api=await port(),config=join(install,role==='eef'?'quiet.yaml':'quiet.json');
 if(role==='eef')await writeFile(config,source.toString().replace('port: '+oldApi,'port: '+api).replace('port: '+oldGateway,'port: '+await port()));
 else{const value=JSON.parse(source.toString());value.dashboard.port=api;await writeFile(config,JSON.stringify(value));}
 const args=['--config','"'+config+'"','--no-ui',...(role==='eef'?['--no-brain','--database','"'+join(install,'quiet.db')+'"']:[])];
 const pidFile=join(install,'process.json'),probeFile=join(install,'console.json');
 const start=`$p=Start-Process -FilePath ${literal(exe)} -ArgumentList @(${args.map(literal).join(',')}) -WorkingDirectory ${literal(install)} -WindowStyle Hidden -PassThru; [ordered]@{id=$p.Id} | ConvertTo-Json -Compress | Out-File -LiteralPath ${literal(pidFile)} -Encoding utf8`;
 const started=await launch('powershell.exe',['-NoProfile','-NonInteractive','-Command',start]).result;assert.equal(started.code,0);
 const pid=JSON.parse(await text(pidFile)).id;assert(Number.isInteger(pid)&&pid>0);
 await until(async()=>!!(await json(`http://127.0.0.1:${api}/api/diagnostics`)).runtime_id,role+' quiet service');
 const code='using System; using System.Runtime.InteropServices; public static class ConsoleProbe { [DllImport("kernel32.dll",SetLastError=true)] public static extern bool FreeConsole(); [DllImport("kernel32.dll",SetLastError=true)] public static extern bool AttachConsole(uint processId); }';
 const probe=`Add-Type -TypeDefinition ${literal(code)}; [ConsoleProbe]::FreeConsole() | Out-Null; $attached=[ConsoleProbe]::AttachConsole(${pid}); $errorCode=[Runtime.InteropServices.Marshal]::GetLastWin32Error(); if($attached){[ConsoleProbe]::FreeConsole() | Out-Null}; [ordered]@{attached=$attached;error_code=$errorCode} | ConvertTo-Json -Compress | Out-File -LiteralPath ${literal(probeFile)} -Encoding utf8`;
 const observed=await launch('powershell.exe',['-NoProfile','-NonInteractive','-Command',probe]).result;assert.equal(observed.code,0);
 const result=JSON.parse(await text(probeFile));assert.equal(result.attached,false,'Background service must not keep an allocated console');
 assert.equal(result.error_code,6,'The verified live service has no console; a missing/exited process is not a pass');
 assert((await processes()).some(p=>p.ProcessId===pid),'The quiet service must still be running');
 quietCases.push({role,live_service_without_console:true});await processes(true);
}
try {
 for(const role of ['eef','eefn']){
  const install=join(scratch,role),exe=join(install,role+'.exe');await alias(join(bin,role+'.exe'),exe);
  assert.equal(await subsystem(exe),3,'The public command entrypoint must use the Windows console subsystem');
  const api=await port(),config=join(install,role==="eef"?"owner config 'shell'.yaml":"node config 'shell'.json"),base=`http://127.0.0.1:${api}`;
  let gateway;
  if(role==='eef'){
   gateway=await port();const yaml=(await readFile(join(root,'config/default_identity.yaml'),'utf8')).replace('port: 51334',`port: ${api}`).replace('port: 51335',`port: ${gateway}`).replaceAll('host: 0.0.0.0','host: 127.0.0.1').replace('policy: prompt','policy: off');await writeFile(config,yaml);
  }else{
   const value=JSON.parse(await readFile(join(root,'config/node.example.json'),'utf8'));
   Object.assign(value,{node_id:'shell-fixture',name:'Shell fixture',connection_enabled:false,auto_local:false,local_pairing:false,endpoints:[],psk:'',
    dashboard:{enabled:true,ui_enabled:false,host:'127.0.0.1',port:api},update:{policy:'off'},input_workflow:{prepare_on_input:false,resolve_on_answer:false},models:{provider:'ollama',ollama:{base_url:'http://127.0.0.1:1',selected:[]},llamacpp:{slots:[]}}});await writeFile(config,JSON.stringify(value));
  }
  launch(exe,['--config',config,'--no-ui',...(role==='eef'?['--no-brain','--database',join(install,'eef.db')]:[])]);
  let observation;await until(async()=>{observation=await json(base+'/api/diagnostics');return observation.runtime_id;},role+' service');
  const before=await readFile(config),operation=randomUUID();
  for(const phase of ['root','selected']){
   if(phase==='selected'){
    const slot=version+'.repair-'+randomUUID();await alias(join(bin,role+'.exe'),join(install,'versions',slot,role+'.exe'));
    await writeFile(join(install,'current.txt'),version);await writeFile(join(install,'versions',version+'.bundle.json'),JSON.stringify({schema_version:1,version,slot,previous_slot:'$root'}));
   }
   const prefix=role+'-'+phase;
   const v=await shell(exe,['--version'],prefix+'-version');assert.equal(v.code,0);assert(v.out.includes(role+' '+version));
   const help=await shell(exe,['--help'],prefix+'-help');assert.equal(help.code,0);assert(help.out.includes('Usage:'));
   const invalid=await shell(exe,['--not-an-eef-option'],prefix+'-invalid');assert.equal(invalid.code,2);assert(invalid.err.includes('unexpected argument'));
   const inspect=await shell(exe,['--config',config,'--json',...(role==='eef'?['diagnostics']:['setup'])],prefix+'-json');assert.equal(inspect.code,0);
   const report=JSON.parse(inspect.out);assert.equal(report.success,true);assert.equal(report.runtime_id,observation.runtime_id);
   if(role==='eef'){
    const missing=await shell(exe,['--config',config,'--json','node','verify-model','--node','absent-node','inspect','--operation-id',operation],prefix+'-receipt');
    assert.equal(missing.code,1);const report=JSON.parse(missing.out);assert.equal(report.state,'not_found');assert.equal(report.operation_id,operation);assert.equal(report.historical,true);assert.equal(report.execution_requested,false);
   }else{
    const rejected=await shell(exe,['--config',config,'--json','connection','pair-stdin'],prefix+'-stdin','not a pairing code');
    assert.equal(rejected.code,1);const report=JSON.parse(rejected.out);assert.equal(report.success,false);
    // Empty/lost stdin also fails, but with a different error. Require evidence
    // that the selected command actually received our nonempty invalid input.
    assert.equal(report.error,'invalid pairing code');
    const update=await shell(exe,['--config',config,'--check-update'],prefix+'-update-flag');
    assert.equal(update.code,1);assert(update.err.includes('no update.manifest_url configured'));
    const input=await shell(exe,['--config',config,'--json','--ask','shell fixture input'],prefix+'-input-flag');
    assert.equal(input.code,1);const workflow=JSON.parse(input.out);assert.equal(workflow.success,false);
    assert.equal(workflow.report_type,'node_input_workflow');assert.equal(workflow.execution_authorized,false);assert.equal(workflow.dispatched,false);
   }
   assert.deepEqual(await readFile(config),before,'Read/help/error/invalid invitation must not change configuration');
  }
  await processes(true);
  await quietService(role,before,api,gateway);
 }
 assert.equal(cases.length,24);
 assert.equal(quietCases.length,2);
 await writeFile(join(scratch,'results.json'),JSON.stringify({passed:true,windows_only:true,roles:['eef','eefn'],console_entrypoints:true,
  real_powershell_wait_and_exit:true,stdout_help_and_json:true,stderr_and_nonzero_exit:true,
  selected_command_exit_propagated:true,selected_stdin_preserved:true,selected_version_and_help:true,
  operation_receipt_visible_on_failure:true,configuration_unchanged:true,case_count:cases.length,cases,
  quiet_background_services:true,quiet_service_observations:quietCases,
  physical_two_pc:false,model_inference:false,desktop_visual_acceptance:false},null,2));
 console.log('Windows shell command checks passed: '+scratch);
}catch(error){await writeFile(join(scratch,'PRIVATE-failure.json'),JSON.stringify({message:String(error)},null,2));await writeFile(join(scratch,'PRIVATE-logs.json'),JSON.stringify(children.map(c=>({exe:c.spawnfile,code:c.exitCode,...c.logs()})),null,2));throw error;}
finally{await processes(true);for(const c of children)if(c.exitCode===null)c.kill();await Promise.allSettled(children.map(c=>c.result));assert.equal((await processes()).length,0);for(const path of links)await unlink(owned(path));}
