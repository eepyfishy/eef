// Two logical headless nodes, pinned TLS, reviewed one-shot system-info grants
// and a bounded transport gate. No models, personal apps, shell/device actions
// or physical two-PC claims.
import assert from 'node:assert/strict';
import {mkdtemp,readFile,writeFile} from 'node:fs/promises';
import {resolve,join} from 'node:path';
import {spawn} from 'node:child_process';
import {createServer as tcpServer,connect} from 'node:net';
import {createServer as httpServer} from 'node:http';
import {randomBytes} from 'node:crypto';
import {reviewedFixture} from './reviewed-fixture.mjs';
const root=resolve(import.meta.dirname,'..'), scratch=await mkdtemp(join(root,'.validation','scheduling-'));
const bin=resolve(process.env.EEF_TEST_BINARY_DIR||join(root,'target/debug'));
const config=join(scratch,'eef.yaml'),children=[],sockets=new Set(),upstreams=new Set(),servers=[];
const env={...process.env,EEF_NODE_PSK:'',APPDATA:join(scratch,'appdata'),EEF_DISCOVERY_DIR:join(scratch,'discovery'),
 PATH:join(root,'.tooling/llvm-mingw-20260616-ucrt-x86_64/bin')+';'+process.env.PATH};
function launch(name,args,extra={}) {
 const child=spawn(join(bin,name+'.exe'),args,{cwd:root,windowsHide:true,env:{...env,...extra},stdio:['ignore','pipe','pipe']});
 let out='',err='';child.stdout.on('data',b=>out=(out+b).slice(-1048576));child.stderr.on('data',b=>err=(err+b).slice(-1048576));
 child.result=new Promise((resolve,reject)=>{child.on('error',reject);child.on('close',code=>resolve({code,out,err}));});children.push(child);return child;
}
async function cli(name,path,args,ok=true) {
 const child=launch(name,['--config',path,'--json',...args]),timer=setTimeout(()=>child.kill(),30000);
 try {const r=await child.result;assert.equal(r.code,ok?0:1,r.err);return JSON.parse(r.out);}finally{clearTimeout(timer);}
}
const coordinator=(args,ok=true,path=config)=>cli('eef',path,args,ok);
const scheduling=(args,ok=true,path=config)=>coordinator(['scheduling',...args],ok,path);
async function until(fn,label){const end=Date.now()+45000;while(Date.now()<end){try{if(await fn())return;}catch{}await new Promise(r=>setTimeout(r,100));}throw Error('Timeout: '+label);}
async function listen(server){servers.push(server);await new Promise(r=>server.listen(0,'127.0.0.1',r));return server.address().port;}
async function port(){const server=tcpServer();await new Promise(r=>server.listen(0,'127.0.0.1',r));const p=server.address().port;await new Promise(r=>server.close(r));return p;}
async function request(url,body,method){const r=await fetch(url,{method:method||(body?'POST':'GET'),headers:{'Content-Type':'application/json'},body:body?JSON.stringify(body):undefined,signal:AbortSignal.timeout(35000)});const text=await r.text();let value;try{value=JSON.parse(text);}catch{value={error:text};}return {status:r.status,value};}
async function json(url,body,method){const r=await request(url,body,method);assert(r.status>=200&&r.status<300,JSON.stringify(r.value));return r.value;}
function pause(){assert(upstreams.size>0);for(const stream of upstreams)stream.pause();}
function resume(){for(const stream of upstreams)stream.resume();}
try {
 const [webPort,gatewayPort]=await Promise.all([port(),port()]);const base=`http://127.0.0.1:${webPort}`;
 const bridgePort=await listen(tcpServer(socket=>{
  const upstream=connect(gatewayPort,'127.0.0.1');sockets.add(socket);sockets.add(upstream);upstreams.add(upstream);
  for(const stream of [socket,upstream]){stream.on('error',()=>{socket.destroy();upstream.destroy();});stream.on('close',()=>{sockets.delete(stream);upstreams.delete(stream);socket.destroy();upstream.destroy();});}
  socket.pipe(upstream);upstream.pipe(socket);
 }));
 const secret=randomBytes(24).toString('hex');
 await writeFile(config,(await readFile(join(root,'config/default_identity.yaml'),'utf8')).replace('port: 51334',`port: ${webPort}`).replace('port: 51335',`port: ${gatewayPort}`).replace('policy: prompt','policy: off'));
 launch('eef',['--config',config,'--database',join(scratch,'eef.db'),'--no-ui','--no-brain'],{EEF_NODE_PSK:secret});
 await until(async()=>(await coordinator(['diagnostics'])).initialized,'coordinator startup');
 const nodes=[];
 for(const id of ['sched-a','sched-b']){
  const path=join(scratch,id+'.json'),apiPort=await port();
  await writeFile(path,JSON.stringify({node_id:id,psk:'',endpoints:[],auto_local:false,local_pairing:false,
   dashboard:{enabled:true,host:'127.0.0.1',port:apiPort},models:{provider:'llamacpp',llamacpp:{slots:[]}},update:{policy:'off'}}));
  launch('eefn',['--config',path,'--no-ui']);
  await until(async()=>(await json(`http://127.0.0.1:${apiPort}/api/status`)).node_id===id,'node startup');
  const code=await coordinator(['credentials','provision','--node',id,'--address',`127.0.0.1:${id==='sched-a'?bridgePort:gatewayPort}`]);
  const file=join(scratch,id+'-PRIVATE.json');await writeFile(file,JSON.stringify(code));nodes.push({id,path,file,api:`http://127.0.0.1:${apiPort}`});
 }
 await coordinator(['credentials','legacy','--allow','false']);
 const restart=async()=>{const old=(await coordinator(['diagnostics'])).runtime_id;await coordinator(['restart']);await until(async()=>(await coordinator(['diagnostics'])).runtime_id!==old,'new coordinator generation');};
 await restart();
 for(const node of nodes)await cli('eefn',node.path,['connection','pair','--code-file',node.file]);
 const connected=async()=>(await coordinator(['diagnostics'])).registered_node_count===2;
 await until(connected,'two TLS nodes registered');
 console.log('Scheduling fixture: two headless TLS nodes registered');
 assert.equal((await fetch(base+'/')).status,404);
 const initial=await scheduling(['show']);assert.equal(initial.applied_policy.freshness_seconds,10);assert.equal(initial.applied_policy.max_in_flight,0);
 const endpoint=base+'/api/commands/scheduling';
 const body={schema_version:1,expected_runtime_id:initial.runtime_id,command:{operation:'limits',max_in_flight:1}};
 assert.equal((await request(endpoint,{...body,expected_runtime_id:'stale'})).status,400);
 assert.equal((await request(endpoint,{...body,schema_version:2})).status,400);
 assert.equal((await request(endpoint,{...body,command:{operation:'show',permissions:true}})).status,422);
 assert.equal((await fetch(endpoint,{method:'POST',headers:{'Content-Type':'application/json',Origin:'https://untrusted.example'},body:JSON.stringify(body)})).status,403);
 await scheduling(['limits'],false);await scheduling(['limits','--max-in-flight','4097'],false);await scheduling(['node','--node','*','--max-in-flight','1'],false);
 const saved=await scheduling(['limits','--freshness-seconds','25','--max-in-flight','1','--per-provider-concurrency','1']);
 assert.equal(saved.restart_required,true);assert.equal(saved.applied_policy.max_in_flight,0);assert.equal(saved.execution_authorized,false);
 await scheduling(['node','--node',nodes[0].id,'--max-in-flight','1']);
 assert.equal((await scheduling(['limits','--max-in-flight','1'])).changed,false);
 const view=await json(base+'/api/config');delete view.config.scheduling;await json(base+'/api/config',{config:view.config},'PUT');
 assert.equal((await scheduling(['show'])).saved_policy.max_in_flight,1,'generic saved settings cannot drop scheduling policy');
 const reviewed=nodes.map(n=>reviewedFixture(base,n.api,n.id));
 const prepare=async index=>{const plan=await reviewed[index].save({template:'system_info',params:{},constraints:{node_id:nodes[index].id}});return {plan,command:await reviewed[index].approve(plan,{max_execution_ms:15000,max_result_bytes:65536})};};
 let prepared=await Promise.all([prepare(0),prepare(1)]);
 pause();await reviewed[0].startOwner(prepared[0].command);
 await until(async()=>(await scheduling(['show'])).admission.total_in_flight===1,'held dispatch before policy applies');
 await reviewed[1].startOwner(prepared[1].command);
 assert.equal((await reviewed[1].wait(prepared[1].plan.id)).status,'completed','saved-only ceiling must not affect the old runtime');
 resume();assert.equal((await reviewed[0].wait(prepared[0].plan.id)).status,'completed');
 console.log('Scheduling fixture: saved limits leave running policy unchanged');
 await restart();await until(connected,'nodes reconnect after applying policy');
 const applied=await scheduling(['show']);assert.equal(applied.restart_required,false);assert.equal(applied.applied_policy.max_in_flight,1);assert.equal(applied.applied_policy.freshness_seconds,25);
 assert(applied.admission.providers.filter(p=>p.node_id===nodes[0].id).every(p=>p.effective_per_provider_concurrency===1));
 assert.equal((await request(endpoint,body)).status,400,'old runtime command cannot change new generation');
 prepared=await Promise.all([prepare(0),prepare(1)]);
 pause();await reviewed[0].startOwner(prepared[0].command);
 await until(async()=>(await scheduling(['show'])).admission.total_in_flight===1,'applied capacity reservation');
 const blocked=await request(base+'/api/commands/requests/execute',{schema_version:1,expected_runtime_id:applied.runtime_id,command:prepared[1].command});
 assert.equal(blocked.status,400);assert.match(blocked.value.error,/provider.*unavailable|scheduling capacity/i);
 assert.equal((await reviewed[1].get(prepared[1].plan.id)).status,'approved','denied execution must not issue work');
 assert.equal((await coordinator(['diagnostics','--node',nodes[1].id,'--samples','1'])).passed,1,'direct owner recovery does not consume scheduling capacity');
 resume();assert.equal((await reviewed[0].wait(prepared[0].plan.id)).status,'completed');
 await until(async()=>(await scheduling(['show'])).admission.total_in_flight===0,'capacity released');
 await reviewed[1].startOwner(prepared[1].command);
 assert.equal((await reviewed[1].wait(prepared[1].plan.id)).status,'completed');
 console.log('Scheduling fixture: applied budget enforced and released; owner recovery available');
 // Corrupt an otherwise schema-valid mutation reply after real persistence.
 // The CLI must report uncertainty, with exactly one request and no replay.
 let mode='corrupt',writes=0;
 const proxyPort=await listen(httpServer(async(req,res)=>{
  if(mode==='old'){res.writeHead(404,{'Content-Type':'application/json'});res.end('{}');return;}
  let input='';for await(const chunk of req)input+=chunk;
  const upstream=await fetch(base+req.url,{method:req.method,headers:{'Content-Type':'application/json'},body:input||undefined});
  const value=await upstream.json();if(req.method==='POST'){writes++;value.saved_policy.max_in_flight=3;}
  res.writeHead(upstream.status,{'Content-Type':'application/json'});res.end(JSON.stringify(value));
 }));
 const proxied=join(scratch,'proxied.yaml');await writeFile(proxied,JSON.stringify({web:{host:'127.0.0.1',port:proxyPort}}));
 const uncertain=await scheduling(['limits','--max-in-flight','2'],false,proxied);assert(uncertain.error.includes('unconfirmed'));assert.equal(writes,1);
 assert.equal((await scheduling(['show'])).saved_policy.max_in_flight,2);
 mode='old';await scheduling(['limits','--max-in-flight','4'],false,proxied);assert.equal(writes,1);
 assert.equal((await scheduling(['show'])).saved_policy.max_in_flight,2);
 await scheduling(['remove-node','--node',nodes[0].id]);assert.deepEqual((await scheduling(['show'])).saved_policy.node_limits,{});
 for(const node of nodes){const jobs=await cli('eefn',node.path,['jobs','list']);assert.equal(jobs.data.total,2);assert(jobs.data.jobs.every(j=>j.status==='completed'));assert.equal((await json(node.api+'/api/status')).models.length,0);}
 const results={passed:true,physical_two_pc:false,pinned_tls:true,command_only_policy:true,saved_applied_separation:true,
  restart_applies_policy:true,global_budget_enforced:true,owner_recovery_stays_available:true,capacity_released:true,
  stale_runtime_and_cross_site_rejected:true,generic_config_preserves_policy:true,corrupted_reply_unconfirmed_without_replay:true,
  no_models_created:true,reviewed_grants:true,denied_execution_checkpoint_unchanged:true};
 await writeFile(join(scratch,'results.json'),JSON.stringify(results,null,2));console.log('Scheduling command validation passed: '+scratch);
}finally{
 resume();for(const socket of sockets)socket.destroy();for(const child of children)if(child.exitCode===null)child.kill();
 await Promise.allSettled(children.map(c=>c.result));
 for(const server of servers){server.closeAllConnections?.();await new Promise(r=>server.close(r));}
}
