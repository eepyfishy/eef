// Opt-in real CPU inference through reviewed public commands and pinned TLS.
// Reuse EEF_TEST_MODEL_FILE, verified against the chosen catalog entry, without
// copying it. Downloads require EEF_TEST_ALLOW_MODEL_DOWNLOAD=1 explicitly.
// Isolated config/processes only; never mutate a personal model or installed app.
import assert from 'node:assert/strict';
import {mkdtemp,readFile,writeFile,stat} from 'node:fs/promises';
import {resolve,join} from 'node:path';
import {spawn} from 'node:child_process';
import {createServer} from 'node:net';
import {randomBytes,createHash} from 'node:crypto';
import {createReadStream} from 'node:fs';
import {reviewedFixture} from './reviewed-fixture.mjs';
assert(process.argv[2],'Pass the runtime bundle directory explicitly; no old-version default');
const root=resolve(import.meta.dirname,'..'),bundle=resolve(process.argv[2]);
const bin=resolve(process.env.EEF_TEST_BINARY_DIR||bundle),reused=process.env.EEF_TEST_MODEL_FILE&&resolve(process.env.EEF_TEST_MODEL_FILE);
assert(reused||process.env.EEF_TEST_ALLOW_MODEL_DOWNLOAD==='1','Choose an existing model or explicitly authorize this isolated download');
const scratch=await mkdtemp(join(root,'.validation','real-model-')),children=[];
const sleep=ms=>new Promise(r=>setTimeout(r,ms));
async function port(){const s=createServer();await new Promise(r=>s.listen(0,'127.0.0.1',r));const p=s.address().port;await new Promise(r=>s.close(r));return p;}
async function json(url,body,method){const r=await fetch(url,{method:method||(body?'POST':'GET'),headers:{'Content-Type':'application/json'},body:body?JSON.stringify(body):undefined});const v=await r.json();assert(r.ok,JSON.stringify(v));return v;}
async function until(fn,label,timeout=60000){const deadline=Date.now()+timeout;while(Date.now()<deadline){try{const v=await fn();if(v)return v;}catch{}await sleep(500);}throw Error('Timed out: '+label);}
function launch(name,args,extra={}){const child=spawn(join(bin,name+'.exe'),args,{cwd:bundle,windowsHide:true,env:{...process.env,PATH:join(root,'.tooling/llvm-mingw-20260616-ucrt-x86_64/bin')+';'+process.env.PATH,APPDATA:join(scratch,'appdata'),EEF_DISCOVERY_DIR:join(scratch,'discovery'),EEF_NODE_PSK:'',...extra},stdio:['ignore','pipe','pipe']});let out='',err='';child.stdout.on('data',b=>out=(out+b).slice(-1048576));child.stderr.on('data',b=>err=(err+b).slice(-1048576));child.result=new Promise((resolve,reject)=>{child.on('error',reject);child.on('close',code=>resolve({code,out,err}));});children.push(child);return child;}
async function cli(name,path,args){const c=launch(name,['--config',path,'--json',...args]),timer=setTimeout(()=>c.kill(),30000);try{const r=await c.result;assert.equal(r.code,0,r.out+r.err);return JSON.parse(r.out);}finally{clearTimeout(timer);}}
const webPort=await port(),devicePort=await port(),nodePort=await port(),modelPort=await port();
const eef=`http://127.0.0.1:${webPort}`,node=`http://127.0.0.1:${devicePort}`;
try{
 const config=JSON.parse(await readFile(join(root,'config/node.example.json'),'utf8'));
 config.dashboard.port=devicePort;config.update.policy='off';config.models.provider='llamacpp';config.models.llamacpp.binary=join(bundle,'tools','llama-server.exe');
 const configPath=join(scratch,'node.json');await writeFile(configPath,JSON.stringify(config));
 const yaml=(await readFile(join(root,'config/default_identity.yaml'),'utf8')).replace('port: 51334',`port: ${webPort}`).replace('port: 51335',`port: ${nodePort}`).replace('policy: prompt','policy: off');
 const eefConfig=join(scratch,'eef.yaml');await writeFile(eefConfig,yaml);
 launch('eef',['--config',eefConfig,'--database',join(scratch,'eef.db'),'--no-brain','--no-ui'],{EEF_NODE_PSK:randomBytes(32).toString('hex')});
 launch('eefn',['--config',configPath,'--no-ui']);
 await until(async()=> (await json(node+'/api/status')).connection?.state==='connected','initial connection');
 const initial=await json(node+'/api/status');assert.equal(initial.models.length,0);
 const invitation=await cli('eef',eefConfig,['credentials','provision','--node',initial.node_id,'--address',`127.0.0.1:${nodePort}`]);
 const privateFile=join(scratch,'PRIVATE-pair.json');await writeFile(privateFile,JSON.stringify(invitation));
 await cli('eef',eefConfig,['credentials','legacy','--allow','false']);
 const old=(await json(eef+'/api/diagnostics')).runtime_id;await cli('eef',eefConfig,['restart']);
 await until(async()=>(await json(eef+'/api/diagnostics')).runtime_id!==old,'apply pinned TLS');
 await cli('eefn',configPath,['connection','pair','--code-file',privateFile]);
 await until(async()=>{const s=await json(node+'/api/status');return s.connection.state==='connected'&&s.execution_grants.available;},'pinned node session');
 const catalog=(await json(node+'/api/models')).catalog;const entry=catalog.find(m=>m.id===(process.env.EEF_TEST_MODEL_ID||catalog[0].id));assert(entry);
 let modelPath=reused,last=-1;
 if(!reused){
 console.log('Explicit test-only model download: '+entry.name+' ('+entry.bytes+' bytes)');
 await json(node+'/api/models/install',{id:entry.id});
 const deadline=Date.now()+900000;
 while(true){const job=(await json(node+'/api/status')).download;
  if(job.state==='error')throw Error(job.error);
  if(job.state==='installed')break;
  const percent=Math.floor(100*(job.completed||0)/(job.total||1));if(percent>=last+10){console.log('Model download '+percent+'%');last=percent;}
  if(Date.now()>deadline)throw Error('Model download timed out');await sleep(1000);
 }
 const model=(await json(node+'/api/models')).installed.find(m=>m.id===entry.id);assert(model);modelPath=model.path;
 }else assert.equal((await json(node+'/api/status')).download.state,'idle');
 assert.equal((await stat(modelPath)).size,entry.bytes);
 const hash=createHash('sha256');for await(const chunk of createReadStream(modelPath))hash.update(chunk);
 assert.equal(hash.digest('hex'),entry.sha256,'existing artifacts must still pass exact verification');
 const selected=(await json(node+'/api/config')).config;selected.models.llamacpp.slots=[{model_id:entry.id,model_path:modelPath,port:modelPort,context:2048,gpu_layers:0}];
 await json(node+'/api/config',{config:selected},'PUT');await cli('eefn',configPath,['restart','--wait-seconds','60']);
 const ready=async()=>{const status=await json(node+'/api/status');const models=(await json(eef+'/api/commands/models')).nodes.find(n=>n.node_id===initial.node_id)?.models;return status.connection.state==='connected'&&models?.some(m=>m.instance.model_id===entry.id&&m.lifecycle==='ready');};
 await until(ready,'real CPU model ready and registered',330000);
 const reviewed=reviewedFixture(eef,node,initial.node_id);
 const infer=async(prompt,max_tokens)=>{
  const plan=await reviewed.save({template:'generate_text',params:{model_id:entry.id,prompt,max_tokens,temperature:0},constraints:{node_id:initial.node_id,backend:'llamacpp'}},prompt);
  await reviewed.start(await reviewed.approve(plan,{max_execution_ms:30000,max_result_bytes:65536}));
  const report=await reviewed.result(plan.id);assert.equal(report.state,'completed',JSON.stringify(report));assert.equal(report.success,true,JSON.stringify(report));
  assert.equal(typeof report.data.content,'string');assert(report.data.content.trim());
  const saved=await reviewed.get(plan.id);assert.equal(saved.tasks[0].attempts,1);assert.equal(saved.tasks[0].assignment.backend,'llamacpp');
  return {id:plan.id,report,metadata:saved};
 };
 const first=await infer('Reply with a short greeting.',24),content=first.report.data.content;
 console.log('Real CPU inference returned: '+content);
 const beforeRestart=(await json(node+'/api/diagnostics')).runtime_id;
 await cli('eefn',configPath,['restart','--wait-seconds','60']);
 assert.notEqual((await json(node+'/api/diagnostics')).runtime_id,beforeRestart);
 await until(ready,'restart selected real model',330000);
 assert.deepEqual(await reviewed.get(first.id),first.metadata,'restart cannot replay completed work');
 assert.deepEqual(await reviewed.result(first.id),first.report,'retained result survives replacement session');
 const second=await infer('Say hello.',16);
 const stopped=(await json(node+'/api/config')).config;stopped.models.llamacpp.slots=[];await json(node+'/api/config',{config:stopped},'PUT');await cli('eefn',configPath,['restart','--wait-seconds','60']);
 await until(async()=>{const s=await json(node+'/api/status');return s.connection?.state==='connected'&&s.models.length===0;},'stop selected model');
 await reviewed.cleanup(first.id);await reviewed.cleanup(second.id);
 await writeFile(join(scratch,'results.json'),JSON.stringify({passed:true,physical_two_pc:false,model:entry.id,bytes:entry.bytes,sha256:entry.sha256,real_cpu_inference:true,model_quality_accepted:false,reused_verified_artifact:!!reused,model_downloads:!reused,pinned_tls:true,reviewed_grants:true,reply:content,stable_identity:true,verified_restart_with_selected_model:true,retained_result_after_restart:true,completed_work_not_replayed:true,model_withdrawal:true},null,2));
 console.log('Real model validation passed: '+scratch);
}finally{
 for(const child of children)if(child.exitCode===null){await new Promise(r=>{const stop=spawn('taskkill.exe',['/PID',String(child.pid),'/T','/F'],{windowsHide:true,stdio:'ignore'});stop.on('exit',r);stop.on('error',r);});}
 await Promise.allSettled(children.map(c=>c.result));
}
