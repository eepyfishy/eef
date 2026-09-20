// Opt-in real HTTPS artifact update in a disposable installation, never personal config.
// bootstrap-node must already contain the explicit-update fix and be older than target.
import assert from 'node:assert/strict';
import {mkdtemp,mkdir,copyFile,readFile,writeFile} from 'node:fs/promises';
import {resolve,join} from 'node:path';
import {spawn} from 'node:child_process';
import {createServer} from 'node:net';
import {randomBytes,createHash} from 'node:crypto';
const [bootstrap,url,sha256,size,version]=process.argv.slice(2);
assert(bootstrap&&url?.startsWith('https://')&&/^[a-f0-9]{64}$/i.test(sha256)&&Number(size)>0&&version,'Usage: node test-real-update.mjs BOOTSTRAP_NODE HTTPS_URL SHA256 BYTES VERSION');
const root=resolve(import.meta.dirname,'..'),scratch=await mkdtemp(join(root,'.validation/real-update-'));
const installation=join(scratch,'node'),nodeConfig=join(installation,'config.json'),eefConfig=join(scratch,'eef.yaml');
const bin=resolve(process.env.EEF_TEST_BINARY_DIR||join(root,'target/release')),children=[];
const env={...process.env,EEF_NODE_PSK:'',APPDATA:join(scratch,'appdata'),EEF_DISCOVERY_DIR:join(scratch,'discovery'),PATH:join(root,'.tooling/llvm-mingw-20260616-ucrt-x86_64/bin')+';'+process.env.PATH};
function launch(exe,args,extra={},cwd=root){const child=spawn(exe,args,{cwd,env:{...env,...extra},windowsHide:true,stdio:['ignore','pipe','pipe']});let out='',err='';child.stdout.on('data',b=>out=(out+b).slice(-1048576));child.stderr.on('data',b=>err=(err+b).slice(-1048576));child.result=new Promise((resolve,reject)=>{child.on('error',reject);child.on('close',code=>resolve({code,out,err}));});children.push(child);return child;}
async function port(){const s=createServer();await new Promise(r=>s.listen(0,'127.0.0.1',r));const p=s.address().port;await new Promise(r=>s.close(r));return p;}
async function json(url,body){const r=await fetch(url,{method:body?'POST':'GET',headers:{'Content-Type':'application/json'},body:body?JSON.stringify(body):undefined,signal:AbortSignal.timeout(15000)});const v=await r.json();assert(r.ok,JSON.stringify(v));return v;}
async function until(fn,label){const end=Date.now()+90000;while(Date.now()<end){try{if(await fn())return;}catch{}await new Promise(r=>setTimeout(r,250));}throw Error('Timeout: '+label);}
async function cli(args,success=true){const c=launch(join(bin,'eef.exe'),['--config',eefConfig,'--json','node',...args]);const timer=setTimeout(()=>c.kill(),180000);try{const r=await c.result;assert.equal(r.code,success?0:1,r.out+r.err);return JSON.parse(r.out);}finally{clearTimeout(timer);}}
const hash=bytes=>createHash('sha256').update(bytes).digest('hex');
try {
 await mkdir(installation);await copyFile(resolve(bootstrap),join(installation,'eefn.exe'));
 const [apiPort,eefPort,gateway]=await Promise.all([port(),port(),port()]);
 const node=`http://127.0.0.1:${apiPort}`,eef=`http://127.0.0.1:${eefPort}`,id='real-update-fixture',secret=randomBytes(24).toString('hex');
 const config=JSON.parse(await readFile(join(root,'config/node.example.json'),'utf8'));
 Object.assign(config,{node_id:id,name:'Real update fixture',psk:secret,auto_local:false,local_pairing:false,endpoints:[`127.0.0.1:${gateway}`],heartbeat_seconds:0.5,dashboard:{enabled:true,host:'127.0.0.1',port:apiPort},management:{allow_remote:true},update:{policy:'auto',check_interval_seconds:300,manifest_url:'https://raw.githubusercontent.com/eepyfishy/eef/main/update/eefn.json'},models:{provider:'ollama',ollama:{base_url:'http://127.0.0.1:1',selected:[]}}});
 config.permissions.remote_updates=true;await writeFile(nodeConfig,JSON.stringify(config));
 const yaml=(await readFile(join(root,'config/default_identity.yaml'),'utf8')).replace('port: 51334',`port: ${eefPort}`).replace('port: 51335',`port: ${gateway}`).replace('policy: prompt','policy: off');await writeFile(eefConfig,yaml);
 launch(join(bin,'eef.exe'),['--config',eefConfig,'--database',join(scratch,'eef.db'),'--no-brain','--no-ui'],{EEF_NODE_PSK:secret});
 launch(join(installation,'eefn.exe'),['--config',nodeConfig,'--no-ui'],{},installation);
 await until(async()=>(await json(node+'/api/diagnostics')).connection_state==='connected','bootstrap connection');
 const before=await json(node+'/api/diagnostics');assert.notEqual(before.version,version);
 const original=await readFile(nodeConfig),configHash=hash(original);
 const args=['update','--node',id,'--version',version,'--url',url,'--sha256',sha256,'--size-bytes',size];
 await cli(args,false); // Missing alpha consent must not start a download.
 assert.equal((await cli(['update-status','--node',id])).data.restart_required,false);
 const applied=await cli([...args,'--allow-prerelease']);assert.equal(applied.success,true);assert.equal(applied.result.applied.new_version,version);
 assert.equal(hash(await readFile(nodeConfig)),configHash);
 const staged=await cli(['update-status','--node',id]);assert.equal(staged.data.current_version,before.version);assert.equal(staged.data.installed_version,version);assert.equal(staged.data.restart_required,true);
 const restart=await cli(['restart','--node',id,'--wait-seconds','60']);assert.equal(restart.completed,true);
 await until(async()=>{const d=await json(node+'/api/diagnostics');return d.version===version&&d.connection_state==='connected';},'new release after handoff');
 const after=await json(node+'/api/diagnostics');assert.equal(after.node_id,before.node_id);assert.notEqual(after.runtime_id,before.runtime_id);assert.deepEqual(after.startup_issues,[]);
 assert.equal(hash(await readFile(nodeConfig)),configHash);
 assert.equal((await cli(['update-status','--node',id])).data.restart_required,false);
 const checked=await json(eef+`/api/node/${id}/invoke`,{capability:'node.update',action:'check',params:{},timeout:15});assert.equal(checked.success,true);assert.equal(checked.data.feed_paused,true);assert.equal(checked.data.update_available,false);
 const result={passed:true,real_https_download:true,artifact_sha256:sha256,artifact_bytes:Number(size),bootstrap_version:before.version,target_version:version,bootstrap_sha256:hash(await readFile(resolve(bootstrap))),identity_preserved:true,config_preserved:true,restart_handoff:true,automatic_feed_paused:true,physical_two_pc:false};
 await writeFile(join(scratch,'results.json'),JSON.stringify(result,null,2));console.log('Real artifact update passed: '+scratch);
} finally {
 // Handoff spawns another process; stop only executables inside this unique fixture.
 const literal=installation.replaceAll("'","''");
 const cleanup=launch('powershell.exe',['-NoProfile','-NonInteractive','-Command',`Get-CimInstance Win32_Process -Filter "Name='eefn.exe'" | Where-Object { $_.ExecutablePath -and $_.ExecutablePath.StartsWith('${literal}\\', [StringComparison]::OrdinalIgnoreCase) } | ForEach-Object { Stop-Process -Id $_.ProcessId -Force }`]);
 await cleanup.result;
 for(const c of children)if(c.exitCode===null)c.kill();await Promise.allSettled(children.map(c=>c.result));
}
