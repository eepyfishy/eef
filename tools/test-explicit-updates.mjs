// Isolated command/transport regression. No release downloads or personal installs.
import assert from 'node:assert/strict';
import {mkdtemp, readFile, writeFile} from 'node:fs/promises';
import {resolve, join} from 'node:path';
import {spawn} from 'node:child_process';
import {createServer} from 'node:http';
import {randomUUID, randomBytes} from 'node:crypto';
const root=resolve(import.meta.dirname,'..'), scratch=await mkdtemp(join(root,'.validation/explicit-updates-'));
const bin=resolve(process.env.EEF_TEST_BINARY_DIR||join(root,'target/debug')), children=[];
const env={...process.env,EEF_NODE_PSK:'',APPDATA:join(scratch,'appdata'),EEF_DISCOVERY_DIR:join(scratch,'discovery'),PATH:join(root,'.tooling/llvm-mingw-20260616-ucrt-x86_64/bin')+';'+process.env.PATH};
function launch(name,args,extra={}) {
 const child=spawn(join(bin,name+'.exe'),args,{cwd:root,env:{...env,...extra},windowsHide:true,stdio:['ignore','pipe','pipe']});
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
 const data={schema_version:1,report_type:'explicit_update',success:true,node_id:body.params.expected_node_id,request_id:body.params.request_id,applied:{new_version:body.params.version}};
 if(mode==='mismatch')data.request_id=randomUUID();
 res.setHeader('Content-Type','application/json');res.end(JSON.stringify({success:true,data}));
});
async function cli(config,args,success){const c=launch('eef',['--config',config,'--json','node',...args]);const timer=setTimeout(()=>c.kill(),20000);try{const r=await c.result;assert.equal(r.code,success?0:1,r.out+r.err);return JSON.parse(r.out);}finally{clearTimeout(timer);}}
try {
 const [eefPort,nodePort,gateway,proxyPort]=await Promise.all([port(),port(),port(),port()]);
 const yaml=(await readFile(join(root,'config/default_identity.yaml'),'utf8')).replace('port: 51334',`port: ${eefPort}`).replace('port: 51335',`port: ${gateway}`).replace('policy: prompt','policy: off');
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
 const invoke=params=>json(eef+`/api/node/${id}/invoke`,{capability:'node.update',action:'apply_explicit',params,timeout:10});
 const wrong=await invoke({...request,expected_node_id:'other-node'});assert.equal(wrong.success,false);assert.match(wrong.error,/different node/);
 const noConsent=await invoke({...request,allow_prerelease:false});assert.equal(noConsent.success,false);assert.match(noConsent.error,/allow_prerelease/);
 // Revocation is enforced without restart, despite the applied runtime grant.
 config.permissions.remote_updates=false;const saved=JSON.stringify(config);await writeFile(nodeConfig,saved);
 const revoked=await invoke(request);assert.equal(revoked.success,false);assert.match(revoked.error,/not allowed/);
 assert.equal((await cli(eefConfig,args,false)).outcome_unknown,true);
 assert.equal(await readFile(nodeConfig,'utf8'),saved);
 // Mock only the coordinator response to check exact confirmation / no replay.
 const confirmed=await cli(proxyConfig,args,true);assert.equal(confirmed.outcome_unknown,false);assert.equal(requests.length,1);
 assert.equal(requests[0].action,'apply_explicit');assert.equal(requests[0].params.allow_prerelease,true);
 for(const next of ['drop','old','mismatch']){mode=next;const before=requests.length;const result=await cli(proxyConfig,args,false);assert.equal(result.outcome_unknown,true);assert.equal(requests.length,before+1);}
 const before=requests.length;await cli(proxyConfig,args.slice(0,-1),false);assert.equal(requests.length,before,'no consent must fail before sending');
 await writeFile(join(scratch,'results.json'),JSON.stringify({passed:true,real_download:false,real_install:false,permission_revocation:true,exact_target:true,explicit_consent:true,no_replay:true,old_node_no_fallback:true,config_unchanged:true},null,2));
 console.log('Explicit update command checks passed: '+scratch);
} finally {proxy.closeAllConnections();proxy.close();for(const c of children)if(c.exitCode===null)c.kill();await Promise.allSettled(children.map(c=>c.result));}
