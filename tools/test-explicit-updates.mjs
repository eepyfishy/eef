// Isolated command/transport regression. No release downloads or personal installs.
import assert from 'node:assert/strict';
import {mkdtemp, readFile, writeFile, mkdir, link, unlink} from 'node:fs/promises';
import {resolve, join} from 'node:path';
import {spawn} from 'node:child_process';
import {createServer} from 'node:http';
import {randomUUID, randomBytes} from 'node:crypto';
const root=resolve(import.meta.dirname,'..'), scratch=await mkdtemp(join(root,'.validation/explicit-updates-'));
const version=(await readFile(join(root,'Cargo.toml'),'utf8')).match(/^version\s*=\s*"([^"]+)"/m)[1];
const bin=resolve(process.env.EEF_TEST_BINARY_DIR||join(root,'target/debug')), children=[];
const nodeInstall=join(scratch,'node-app'),nodeExe=join(nodeInstall,'eefn.exe');let nodeLinked=false;
const env={...process.env,EEF_NODE_PSK:'',APPDATA:join(scratch,'appdata'),EEF_DISCOVERY_DIR:join(scratch,'discovery'),PATH:join(root,'.tooling/llvm-mingw-20260616-ucrt-x86_64/bin')+';'+process.env.PATH};
for(const key of Object.keys(env))if(['HTTP_PROXY','HTTPS_PROXY','ALL_PROXY','NO_PROXY'].includes(key.toUpperCase()))delete env[key];
env.NO_PROXY='127.0.0.1,localhost';
function launch(name,args,extra={}) {
 const child=spawn(name==='eefn'?nodeExe:join(bin,name+'.exe'),args,{cwd:root,env:{...env,...extra},windowsHide:true,stdio:['ignore','pipe','pipe']});
 let out='',err='';child.stdout.on('data',b=>out+=b);child.stderr.on('data',b=>err+=b);
 child.result=new Promise((resolve,reject)=>{child.on('error',reject);child.on('close',code=>resolve({code,out,err}));});children.push(child);return child;
}
async function port(){const s=createServer();await new Promise(r=>s.listen(0,'127.0.0.1',r));const p=s.address().port;await new Promise(r=>s.close(r));return p;}
async function json(url,body){const r=await fetch(url,{method:body?'POST':'GET',headers:{'Content-Type':'application/json'},body:body?JSON.stringify(body):undefined,signal:AbortSignal.timeout(15000)});return r.json();}
async function until(fn){const end=Date.now()+30000;while(Date.now()<end){try{if(await fn())return;}catch{}await new Promise(r=>setTimeout(r,100));}throw Error('node did not register');}
const id='update-fixture', nodeConfig=join(scratch,'node.json'), eefConfig=join(scratch,'eef.yaml'), proxyConfig=join(scratch,'proxy.yaml');
let mode='success',requests=[];
const proxy=createServer(async(req,res)=>{
 let raw='';for await(const chunk of req)raw+=chunk;
 const body=JSON.parse(raw);requests.push(body);
 if(mode==='drop'){res.destroy();return;}
 if(mode==='old'){res.writeHead(400,{'Content-Type':'application/json'});res.end(JSON.stringify({success:false,error:'unknown node.update action'}));return;}
 const data={schema_version:1,report_type:body.action==='repair_explicit'?'explicit_repair':'explicit_update',success:true,node_id:body.params.expected_node_id,request_id:body.params.request_id,applied:{new_version:body.params.version}};
 if(mode==='mismatch')data.request_id=randomUUID();
 res.setHeader('Content-Type','application/json');res.end(JSON.stringify({success:true,data}));
});
async function cli(config,args,success){const c=launch('eef',['--config',config,'--json','node',...args]);const timer=setTimeout(()=>c.kill(),20000);try{const r=await c.result;assert.equal(r.code,success?0:1,r.out+r.err);return JSON.parse(r.out);}finally{clearTimeout(timer);}}
try {
 await mkdir(nodeInstall);await link(join(bin,'eefn.exe'),nodeExe);nodeLinked=true;
 const [eefPort,nodePort,gateway,proxyPort]=await Promise.all([port(),port(),port(),port()]);
 const yaml=(await readFile(join(root,'config/default_identity.yaml'),'utf8')).replaceAll('host: 0.0.0.0','host: 127.0.0.1').replace('port: 51334',`port: ${eefPort}`).replace('port: 51335',`port: ${gateway}`).replace('policy: prompt','policy: off');
 await writeFile(eefConfig,yaml);await writeFile(proxyConfig,yaml.replace(`port: ${eefPort}`,`port: ${proxyPort}`));
 await new Promise(r=>proxy.listen(proxyPort,'127.0.0.1',r));
 const secret=randomBytes(24).toString('hex'), config=JSON.parse(await readFile(join(root,'config/node.example.json'),'utf8'));
 Object.assign(config,{node_id:id,name:'Update fixture',auto_local:false,local_pairing:false,psk:secret,endpoints:[`127.0.0.1:${gateway}`],heartbeat_seconds:0.5,dashboard:{enabled:true,host:'127.0.0.1',port:nodePort},update:{policy:'off',manifest_url:'https://example.invalid/stable.json'},models:{provider:'ollama',ollama:{base_url:'http://127.0.0.1:1',selected:[]}}});
 config.permissions.remote_updates=true;await writeFile(nodeConfig,JSON.stringify(config));
 launch('eef',['--config',eefConfig,'--database',join(scratch,'eef.db'),'--no-brain','--no-ui'],{EEF_NODE_PSK:secret});
 launch('eefn',['--config',nodeConfig,'--no-ui']);
 const node=`http://127.0.0.1:${nodePort}`,eef=`http://127.0.0.1:${eefPort}`;
 await until(async()=>(await json(node+'/api/diagnostics')).connection_state==='connected');
 const status=await cli(eefConfig,['update-status','--node',id],true);
 assert.equal(status.data.report_type,'update_status');assert.equal(status.data.restart_required,false);
 const args=['update','--node',id,'--version','99.0.0-alpha.1','--url','https://example.invalid/never-downloaded','--sha256','a'.repeat(64),'--size-bytes','123','--allow-prerelease'];
 const request={schema_version:1,expected_node_id:id,request_id:randomUUID(),version:'99.0.0-alpha.1',url:'https://example.invalid/never-downloaded',sha256:'a'.repeat(64),size_bytes:123,allow_prerelease:true};
 const invoke=(params,action='apply_explicit')=>json(eef+`/api/node/${id}/invoke`,{capability:'node.update',action,params,timeout:10});
 const wrong=await invoke({...request,expected_node_id:'other-node'});assert.equal(wrong.success,false);assert.match(wrong.error,/different node/);
 const noConsent=await invoke({...request,allow_prerelease:false});assert.equal(noConsent.success,false);assert.match(noConsent.error,/allow_prerelease/);
 const wrongRepair=await invoke({...request,expected_node_id:'other-node'},'repair_explicit');assert.equal(wrongRepair.success,false);assert.match(wrongRepair.error,/different node/);
 const repairId=randomUUID(),closedPort=await port();
 const failedRepair={...request,request_id:repairId,version,url:`https://127.0.0.1:${closedPort}/never`};
 const failedRemote=await invoke(failedRepair,'repair_explicit');assert.equal(failedRemote.success,false);
 const retained=await cli(eefConfig,['update-status','--node',id,'--operation-id',repairId],true);
 assert.equal(retained.data.repair_receipts.available,true);assert.equal(retained.data.repair_receipts.attempts.length,1);
 assert.equal(retained.data.repair_receipts.attempts[0].receipt.request_id,repairId);assert.equal(retained.data.repair_receipts.attempts[0].receipt.outcome,'failed');
 const replayed=await invoke(failedRepair,'repair_explicit');assert.equal(replayed.success,false);assert.match(replayed.error,/already admitted/);
 // Revocation is enforced without restart, despite the applied runtime grant.
 config.permissions.remote_updates=false;const saved=JSON.stringify(config);await writeFile(nodeConfig,saved);
 const revoked=await invoke(request);assert.equal(revoked.success,false);assert.match(revoked.error,/not allowed/);
 const revokedRepair=await invoke({...request,version},'repair_explicit');assert.equal(revokedRepair.success,false);assert.match(revokedRepair.error,/not allowed/);
 assert.equal((await cli(eefConfig,args,false)).outcome_unknown,true);
 assert.equal(await readFile(nodeConfig,'utf8'),saved);
 // Mock only the coordinator response to check exact confirmation / no replay.
 const confirmed=await cli(proxyConfig,args,true);assert.equal(confirmed.outcome_unknown,false);assert.equal(requests.length,1);
 assert.equal(requests[0].action,'apply_explicit');assert.equal(requests[0].params.allow_prerelease,true);
 for(const next of ['drop','old','mismatch']){mode=next;const before=requests.length;const result=await cli(proxyConfig,args,false);assert.equal(result.outcome_unknown,true);assert.equal(requests.length,before+1);}
 const before=requests.length;await cli(proxyConfig,args.slice(0,-1),false);assert.equal(requests.length,before,'no consent must fail before sending');
 const repairArgs=[...args,'--repair'];repairArgs[repairArgs.indexOf('--version')+1]=version;
 const pinnedId=randomUUID();repairArgs.push('--operation-id',pinnedId);
 mode='success';const repairBefore=requests.length,repair=await cli(proxyConfig,repairArgs,true);
 assert.equal(repair.report_type,'remote_explicit_repair');assert.equal(repair.outcome_unknown,false);assert.equal(requests.length,repairBefore+1);assert.equal(requests.at(-1).action,'repair_explicit');assert.equal(requests.at(-1).params.request_id,pinnedId);
 for(const next of ['drop','old','mismatch']){mode=next;const count=requests.length;const result=await cli(proxyConfig,repairArgs,false);assert.equal(result.outcome_unknown,true);assert.equal(requests.length,count+1,'repair must not automatically retry or fall back');}
 await writeFile(join(scratch,'results.json'),JSON.stringify({passed:true,real_download:false,real_install:false,permission_revocation:true,exact_target:true,explicit_consent:true,no_replay:true,old_node_no_fallback:true,config_unchanged:true},null,2));
 console.log('Explicit update command checks passed: '+scratch);
} finally {proxy.closeAllConnections();proxy.close();for(const c of children)if(c.exitCode===null)c.kill();await Promise.allSettled(children.map(c=>c.result));if(nodeLinked)await unlink(nodeExe);}
