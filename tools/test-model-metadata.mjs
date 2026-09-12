// Isolated one-PC protocol test with a fake Ollama HTTP backend. No real inference,
// model downloads, personal configuration, or physical cross-node proof.
import assert from 'node:assert/strict';
import {mkdtemp,readFile,writeFile} from 'node:fs/promises';
import {resolve,join} from 'node:path';
import {spawn} from 'node:child_process';
import {createServer as tcpServer} from 'node:net';
import {createServer} from 'node:http';
import {randomBytes} from 'node:crypto';
const root=resolve(import.meta.dirname,'..');
const scratch=await mkdtemp(join(root,'.validation','model-metadata-'));
const bin=resolve(process.env.EEF_TEST_BINARY_DIR||join(root,'target/debug'));
const children=[],requests=[];
// Optional compatibility run: selected old executable, still isolated config/data.
const legacyNode=process.env.EEF_TEST_LEGACY_NODE_BINARY;
const env={...process.env,EEF_NODE_PSK:'',APPDATA:join(scratch,'appdata'),EEF_DISCOVERY_DIR:join(scratch,'discovery'),PATH:join(root,'.tooling/llvm-mingw-20260616-ucrt-x86_64/bin')+';'+process.env.PATH};
function launch(name,args,extra={}) {
 const executable=name==='eefn'&&legacyNode?resolve(legacyNode):join(bin,name+'.exe');
 const child=spawn(executable,args,{cwd:root,windowsHide:true,env:{...env,...extra},stdio:['ignore','pipe','pipe']});
 let out='',err='';child.stdout.on('data',b=>{out=(out+b).slice(-1024*1024);});child.stderr.on('data',b=>{err=(err+b).slice(-1024*1024);});
 child.result=new Promise((resolve,reject)=>{child.on('error',reject);child.on('close',code=>resolve({code,out,err}));});children.push(child);return child;
}
async function port(){const s=tcpServer();await new Promise(r=>s.listen(0,'127.0.0.1',r));const p=s.address().port;await new Promise(r=>s.close(r));return p;}
async function until(fn,label){const end=Date.now()+65000;while(Date.now()<end){try{if(await fn())return;}catch{}await new Promise(r=>setTimeout(r,200));}throw Error('Timeout: '+label);}
async function json(url,body,method){const r=await fetch(url,{method:method||(body?'POST':'GET'),headers:{'Content-Type':'application/json'},body:body?JSON.stringify(body):undefined,signal:AbortSignal.timeout(10000)});const value=await r.json();assert(r.ok,JSON.stringify(value));return value;}
const fake=createServer(async(req,res)=>{
 res.setHeader('Content-Type','application/json');
 if(req.url==='/api/tags')return res.end(JSON.stringify({models:[{name:'fixture-text'},{name:'fixture-vision'}]}));
 if(req.url==='/api/show')return res.end(JSON.stringify({capabilities:['completion','vision']}));
 if(req.url==='/api/chat'){
  let body='';for await(const chunk of req){body+=chunk;if(body.length>65536){res.writeHead(413);return res.end('{}');}}
  requests.push(JSON.parse(body));return res.end(JSON.stringify({message:{content:'fixture response; not real inference'}}));
 }
 res.writeHead(404);res.end('{}');
});
try{
 await new Promise(r=>fake.listen(0,'127.0.0.1',r));
 const [eefPort,nodePort,apiPort]=await Promise.all([port(),port(),port()]);
 const eef=`http://127.0.0.1:${eefPort}`;let node=`http://127.0.0.1:${apiPort}`;
 const secret=randomBytes(24).toString('hex'),id='metadata-fixture-node';
 const config=JSON.parse(await readFile(join(root,'config/node.example.json'),'utf8'));
 Object.assign(config,{node_id:id,name:'Metadata fixture',psk:secret,auto_local:false,local_pairing:false,endpoints:[`127.0.0.1:${nodePort}`],heartbeat_seconds:1,dashboard:{enabled:true,host:'127.0.0.1',port:apiPort},update:{policy:'off'},models:{provider:'ollama',ollama:{base_url:`http://127.0.0.1:${fake.address().port}`,selected:[{model_id:'fixture-text',modality:'text'},{model_id:'fixture-vision',modality:'vlm'}]},llamacpp:{slots:[]}}});
 const nodeConfig=join(scratch,'node.json'),eefConfig=join(scratch,'eef.yaml');
 const nodeCommand=async(args,success=true)=>{const child=launch('eefn',['--config',nodeConfig,'models',...args,'--json']);const timer=setTimeout(()=>child.kill(),20000);try{const result=await child.result;assert.equal(result.code,success?0:1,result.err);return JSON.parse(result.out);}finally{clearTimeout(timer);}};
 const localRestart=async()=>{const child=launch('eefn',['--config',nodeConfig,'restart','--wait-seconds','60','--json']);const timer=setTimeout(()=>child.kill(),70000);try{const result=await child.result;assert.equal(result.code,0,result.out+' '+result.err);const reply=JSON.parse(result.out);assert.equal(reply.completed,true);assert.notEqual(reply.runtime_id,reply.previous_runtime_id);return reply;}finally{clearTimeout(timer);}};
 await writeFile(nodeConfig,JSON.stringify(config));
 const yaml=(await readFile(join(root,'config/default_identity.yaml'),'utf8')).replace('port: 51334',`port: ${eefPort}`).replace('port: 51335',`port: ${nodePort}`).replace('policy: prompt','policy: off');
 await writeFile(eefConfig,yaml);
 launch('eef',['--config',eefConfig,'--database',join(scratch,'eef.db'),'--no-brain'],{EEF_NODE_PSK:secret});
 launch('eefn',['--config',nodeConfig,'--no-ui']);
 const inventory=()=>json(eef+'/api/commands/models?node_id='+id);
 await until(async()=>(await inventory()).nodes[0]?.models.length===2,'versioned registration');
 const models=(await inventory()).nodes[0].models;
 assert(models.every(m=>m.metadata_source===(legacyNode?'legacy_registration':'model_metadata_v1')&&m.lifecycle===null&&m.resource_estimates.ram_mb===null));
 assert.deepEqual(models.find(m=>m.instance.model_id==='fixture-vision').capabilities,['llm.infer','vlm.analyze']);
 assert.equal((await json(eef+'/api/commands/models?capability=vlm.analyze')).nodes[0].models.length,1);
 assert.equal((await fetch(eef+'/api/commands/models',{headers:{Origin:'https://untrusted.example'}})).status,403);
 const owner=launch('eef',['--config',eefConfig,'models','list','--json']);
 const timer=setTimeout(()=>owner.kill(),20000);const cli=await owner.result;clearTimeout(timer);
 assert.equal(cli.code,0,cli.err);assert.equal(JSON.parse(cli.out).nodes[0].models.length,2);
 const invoke=params=>json(eef+`/api/node/${id}/invoke`,{capability:'llm.infer',action:'run',params:{model:'fixture-text',prompt:'protocol fixture',...params},timeout:10});
 const reply=await invoke({backend:'ollama'});assert.equal(reply.success,true);assert.equal(requests.length,1);
 if(!legacyNode){const wrong=await invoke({backend:'llamacpp'});assert.equal(wrong.success,false);assert.equal(requests.length,1,'wrong backend must not fall back');}
 const status=await json(eef+'/api/status');assert.equal(status.models.length,2);assert(status.models.every(m=>m.model_metadata.schema_version===1));
 if(!legacyNode){
  const before=await readFile(nodeConfig,'utf8');
  await nodeCommand(['hints','--backend','ollama','--model','fixture-text','--capability','vlm.analyze'],false);
  assert.equal(await readFile(nodeConfig,'utf8'),before);
  const saved=await nodeCommand(['hints','--backend','ollama','--model','fixture-vision','--capability','llm.infer','--role','request_interpreter']);
  assert.equal(saved.changed,true);assert.equal(saved.restart_required,true);assert.equal(saved.running,true);
  assert(saved.registered_models.find(m=>m.model_id==='fixture-vision').model_metadata.capabilities.includes('vlm.analyze'),'saved is not applied');
  assert.deepEqual(saved.saved_selections.find(m=>m.model_id==='fixture-vision').model_metadata.roles,['request_interpreter']);
  assert.equal((await fetch(node+'/api/commands/models',{method:'POST',headers:{'Content-Type':'application/json',Origin:'https://untrusted.example'},body:JSON.stringify({schema_version:1,expected_node_id:id,command:{operation:'show'}})})).status,403);
  assert.equal((await fetch(node+'/api/commands/models',{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify({schema_version:1,expected_node_id:'wrong',command:{operation:'show'}})})).status,400);
  await localRestart();
  await until(async()=>{const m=(await inventory()).nodes[0]?.models.find(m=>m.instance.model_id==='fixture-vision');return m?.roles?.includes('request_interpreter')&&!m.capabilities.includes('vlm.analyze');},'owner hints apply after restart');
  const denied=await json(eef+`/api/node/${id}/invoke`,{capability:'vlm.analyze',action:'run',params:{model:'fixture-vision',backend:'ollama'},timeout:10});
  assert.equal(denied.success,false);assert.equal(requests.length,1,'restricted capability must not reach backend');
  const newPort=await port(),pending=(await json(node+'/api/config')).config;
  pending.dashboard.port=newPort;await json(node+'/api/config',{config:pending},'PUT');
  assert.equal((await nodeCommand(['show'])).restart_required,true,'commands must reach actual API despite pending port');
  const oldRuntime=(await json(node+'/api/diagnostics')).runtime_id;
  const changed=await localRestart();assert.equal(changed.previous_runtime_id,oldRuntime);
  node=`http://127.0.0.1:${newPort}`;
  assert.equal((await json(node+'/api/diagnostics')).runtime_id,changed.runtime_id);
  await until(async()=>(await inventory()).nodes[0]?.models.length===2,'reconnect after local API port change');
  for(const model of ['fixture-text','fixture-vision'])await nodeCommand(['remove','--backend','ollama','--model',model]);
  const removed=await nodeCommand(['show']);assert.equal(removed.saved_selections.length,0);assert.equal(removed.registered_models.length,2);
 }else{
  const current=(await json(node+'/api/config')).config;current.models.ollama.selected=[];
  await json(node+'/api/config',{config:current},'PUT');
 }
 if(legacyNode)await json(node+'/api/restart',{});else await localRestart();
 await until(async()=>(await inventory()).nodes[0]?.models.length===0&&(await json(eef+'/api/status')).models.length===0,'empty snapshot replaces old models');
 assert.equal((await json(node+'/api/diagnostics')).node_id,id);
 await writeFile(join(scratch,'results.json'),JSON.stringify({passed:true,physical_two_pc:false,backend:'fake HTTP Ollama fixture',real_inference:false,model_downloads:false,legacy_node:!!legacyNode,versioned_inventory:!legacyNode,owner_cli:true,origin_guard:true,selection_commands:!legacyNode,local_restart:!legacyNode,pending_api_port_change:!legacyNode,capability_restrictions_enforced:!legacyNode,explicit_backend_no_fallback:legacyNode?'not supported by old node':true,empty_snapshot_replacement:true,stable_node_identity:true},null,2));
 console.log('Model metadata protocol checks passed: '+scratch);
}finally{
 for(const child of children)if(child.exitCode===null)child.kill();
 await Promise.allSettled(children.map(c=>c.result));
 fake.closeAllConnections();await new Promise(r=>fake.close(r));
}
