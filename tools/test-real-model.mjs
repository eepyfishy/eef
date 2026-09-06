// Opt-in real inference check. Downloads the catalog's ~491 MB model into an
// isolated test directory. Never selects, removes, or downloads personal models.
import assert from 'node:assert/strict';
import {mkdtemp,readFile,writeFile,stat} from 'node:fs/promises';
import {resolve,join} from 'node:path';
import {spawn} from 'node:child_process';
import {createServer} from 'node:net';
import {randomBytes} from 'node:crypto';
const root=resolve(import.meta.dirname,'..'),bundle=resolve(process.argv[2]||'bundle/eef-windows-x86_64-v0.3.2');
const scratch=await mkdtemp(join(root,'.validation','real-model-')),children=[];
const sleep=ms=>new Promise(r=>setTimeout(r,ms));
async function port(){const s=createServer();await new Promise(r=>s.listen(0,'127.0.0.1',r));const p=s.address().port;await new Promise(r=>s.close(r));return p;}
async function json(url,body,method){const r=await fetch(url,{method:method||(body?'POST':'GET'),headers:{'Content-Type':'application/json'},body:body?JSON.stringify(body):undefined});const v=await r.json();assert(r.ok,JSON.stringify(v));return v;}
async function until(fn,label,timeout=60000){const deadline=Date.now()+timeout;while(Date.now()<deadline){try{const v=await fn();if(v)return v;}catch{}await sleep(500);}throw Error('Timed out: '+label);}
function launch(name,args,extra={}){const child=spawn(join(bundle,name+'.exe'),args,{cwd:bundle,windowsHide:true,env:{...process.env,APPDATA:join(scratch,'appdata'),EEF_DISCOVERY_DIR:join(scratch,'discovery'),EEF_NODE_PSK:'',...extra},stdio:'ignore'});children.push(child);return child;}
const webPort=await port(),devicePort=await port(),nodePort=await port(),modelPort=await port();
const eef=`http://127.0.0.1:${webPort}`,node=`http://127.0.0.1:${devicePort}`;
try{
 const config=JSON.parse(await readFile(join(root,'config/node.example.json'),'utf8'));
 config.dashboard.port=devicePort;config.update.policy='off';config.models.provider='llamacpp';config.models.llamacpp.binary=join(bundle,'tools','llama-server.exe');
 const configPath=join(scratch,'node.json');await writeFile(configPath,JSON.stringify(config));
 const yaml=(await readFile(join(root,'config/default_identity.yaml'),'utf8')).replace('port: 51334',`port: ${webPort}`).replace('port: 51335',`port: ${nodePort}`).replace('policy: prompt','policy: off');
 const eefConfig=join(scratch,'eef.yaml');await writeFile(eefConfig,yaml);
 launch('eef',['--config',eefConfig,'--database',join(scratch,'eef.db'),'--no-brain'],{EEF_NODE_PSK:randomBytes(32).toString('hex')});
 launch('eefn',['--config',configPath]);
 await until(async()=> (await json(node+'/api/status')).connection?.state==='connected','initial connection');
 const initial=await json(node+'/api/status');assert.equal(initial.models.length,0);
 const catalog=(await json(node+'/api/models')).catalog;const entry=catalog[0];
 console.log('Explicit test-only model download: '+entry.name+' ('+entry.bytes+' bytes)');
 await json(node+'/api/models/install',{id:entry.id});let last=-1;
 const deadline=Date.now()+900000;
 while(true){const job=(await json(node+'/api/status')).download;
  if(job.state==='error')throw Error(job.error);
  if(job.state==='installed')break;
  const percent=Math.floor(100*(job.completed||0)/(job.total||1));if(percent>=last+10){console.log('Model download '+percent+'%');last=percent;}
  if(Date.now()>deadline)throw Error('Model download timed out');await sleep(1000);
 }
 const model=(await json(node+'/api/models')).installed.find(m=>m.id===entry.id);assert(model);assert.equal((await stat(model.path)).size,entry.bytes);
 const selected=(await json(node+'/api/config')).config;selected.models.llamacpp.slots=[{model_id:entry.id,model_path:model.path,port:modelPort,context:2048,gpu_layers:0}];
 await json(node+'/api/config',{config:selected},'PUT');await json(node+'/api/restart',{});
 await until(async()=> (await json(eef+'/api/status')).models?.some(m=>m.model_id===entry.id),'real model readiness and registration',330000);
 const reply=await json(eef+`/api/node/${initial.node_id}/invoke`,{capability:'llm.infer',action:'run',params:{model:entry.id,prompt:'Reply with a short greeting.',max_tokens:24,temperature:0,timeout:120},timeout:150});
 assert.notEqual(reply.success,false,JSON.stringify(reply));const content=reply.data?.content||reply.result?.content||reply.content;assert.equal(typeof content,'string',JSON.stringify(reply));assert(content.trim());
 console.log('Real CPU inference returned: '+content);
 await json(node+'/api/restart',{});await sleep(1500);
 await until(async()=>{const s=await json(node+'/api/status');return s.connection?.state==='connected'&&s.models.some(m=>m.model_id===entry.id);},'restart selected real model',330000);
 const second=await json(eef+`/api/node/${initial.node_id}/invoke`,{capability:'llm.infer',action:'run',params:{model:entry.id,prompt:'Say hello.',max_tokens:16,temperature:0,timeout:120},timeout:150});
 assert((second.data?.content||second.result?.content||second.content)?.trim(),JSON.stringify(second));
 const stopped=(await json(node+'/api/config')).config;stopped.models.llamacpp.slots=[];await json(node+'/api/config',{config:stopped},'PUT');await json(node+'/api/restart',{});
 await until(async()=>{const s=await json(node+'/api/status');return s.connection?.state==='connected'&&s.models.length===0;},'stop selected model');
 await writeFile(join(scratch,'results.json'),JSON.stringify({passed:true,bundled_runtime:bundle,model:entry.id,bytes:entry.bytes,sha256:entry.sha256,real_cpu_inference:true,reply:content,stable_identity:true,restart_with_selected_model:true},null,2));
 console.log('Real model validation passed: '+scratch);
}finally{
 for(const child of children)if(child.exitCode===null){await new Promise(r=>{const stop=spawn('taskkill.exe',['/PID',String(child.pid),'/T','/F'],{windowsHide:true,stdio:'ignore'});stop.on('exit',r);stop.on('error',r);});}
}
