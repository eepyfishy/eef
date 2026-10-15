// Read-only onboarding through a headless running node. Synthetic backend only;
// no real model, download, personal configuration or physical two-PC claim.
import assert from 'node:assert/strict';
import {mkdtemp,readFile,writeFile} from 'node:fs/promises';
import {join,resolve} from 'node:path';
import {spawn} from 'node:child_process';
import {createServer as tcpServer} from 'node:net';
import {createServer} from 'node:http';
import {randomBytes,randomUUID} from 'node:crypto';
const root=resolve(import.meta.dirname,'..'),bin=resolve(process.env.EEF_TEST_BINARY_DIR||join(root,'target/debug'));
const scratch=await mkdtemp(join(root,'.validation/setup-check-')),children=[];
const nodeConfig=join(scratch,'node.json'),eefConfig=join(scratch,'eef.yaml');
const env={...process.env,EEF_NODE_PSK:'',APPDATA:join(scratch,'appdata'),EEF_DISCOVERY_DIR:join(scratch,'discovery'),PATH:join(root,'.tooling/llvm-mingw-20260616-ucrt-x86_64/bin')+';'+process.env.PATH};
let modelCalls=0,proxyCalls=0,proxyMode='corrupt';
const fake=createServer((req,res)=>{res.setHeader('Content-Type','application/json');if(req.url==='/api/tags')return res.end(JSON.stringify({models:[{name:'fixture-interpreter'}]}));modelCalls++;res.writeHead(409);res.end('{}');});
let node;
const proxy=createServer(async(req,res)=>{
 try{
  let body='';for await(const part of req)body+=part;
  if(req.url==='/api/commands/setup/check'){
   proxyCalls++;
   if(proxyMode==='old'){res.writeHead(404);return res.end('{}');}
  }
  const response=await fetch(node+req.url,{method:req.method,headers:{'Content-Type':'application/json'},body:body||undefined,signal:AbortSignal.timeout(10000)});
  const value=await response.json();
  if(req.url==='/api/commands/setup/check'){
   if(proxyMode==='drop')return res.destroy();
   if(proxyMode==='corrupt')value.execution_authorized=true;
   if(proxyMode==='prose')value.note='PRIVATE-MUST-NOT-ECHO';
   if(proxyMode==='legacy-input')delete value.input_channel;
   if(proxyMode==='legacy-registration')delete value.registration;
   if(proxyMode==='bad-registration')value.registration='PRIVATE-INVALID-REGISTRATION';
  }
  res.writeHead(response.status,{'Content-Type':'application/json'});res.end(JSON.stringify(value));
 }catch{res.destroy();}
});
async function port(){const s=tcpServer();await new Promise(r=>s.listen(0,'127.0.0.1',r));const p=s.address().port;await new Promise(r=>s.close(r));return p;}
function start(name,args,extra={}){
 const c=spawn(join(bin,name+'.exe'),args,{cwd:root,windowsHide:true,env:{...env,...extra},stdio:['ignore','pipe','pipe']});let out='',err='';
 c.stdout.on('data',b=>out=(out+b).slice(-1048576));c.stderr.on('data',b=>err=(err+b).slice(-1048576));
 c.logs=()=>({out,err});c.result=new Promise((r,j)=>{c.on('error',j);c.on('close',code=>r({code,out,err}));});children.push(c);return c;
}
async function cli(args,jsonOutput=true,ok=true){const c=start('eefn',['--config',nodeConfig,...(jsonOutput?['--json']:[]),...args]),timer=setTimeout(()=>c.kill(),30000);try{const r=await c.result;assert.equal(r.code,ok?0:1,'CLI failed; private '+scratch);return jsonOutput?JSON.parse(r.out):r;}finally{clearTimeout(timer);}}
async function owner(args){const c=start('eef',['--config',eefConfig,'--json',...args]),timer=setTimeout(()=>c.kill(),30000);try{const r=await c.result;assert.equal(r.code,0,'Owner CLI failed; private '+scratch);return JSON.parse(r.out);}finally{clearTimeout(timer);}}
async function json(url,body){const r=await fetch(url,{method:body?'POST':'GET',headers:{'Content-Type':'application/json'},body:body?JSON.stringify(body):undefined,signal:AbortSignal.timeout(10000)});const v=await r.json();assert(r.ok,'HTTP failed; private '+scratch);return v;}
async function until(fn,label){const deadline=Date.now()+30000;while(Date.now()<deadline){try{if(await fn())return;}catch{}await new Promise(r=>setTimeout(r,100));}throw Error('Timeout: '+label);}
async function check(){const before=await readFile(nodeConfig,'utf8'),v=await cli(['setup']);assert.equal(await readFile(nodeConfig,'utf8'),before);for(const flag of ['changed','model_inference_requested','execution_authorized','restart_requested','automatically_retried'])assert.equal(v[flag],false);return v;}
try{
 await Promise.all([new Promise(r=>fake.listen(0,'127.0.0.1',r)),new Promise(r=>proxy.listen(0,'127.0.0.1',r))]);
 const [api,gateway,web]=await Promise.all([port(),port(),port()]);node=`http://127.0.0.1:${api}`;
 const config=JSON.parse(await readFile(join(root,'config/node.example.json'),'utf8'));config.node_id='setup-fixture';config.dashboard.port=api;config.update.policy='off';
 config.models.provider='ollama';config.models.ollama={base_url:`http://127.0.0.1:${fake.address().port}`,selected:[]};
 await writeFile(nodeConfig,JSON.stringify(config));
 const yaml=(await readFile(join(root,'config/default_identity.yaml'),'utf8')).replace('port: 51334',`port: ${web}`).replace('port: 51335',`port: ${gateway}`).replace('policy: prompt','policy: off');await writeFile(eefConfig,yaml);
 start('eef',['--config',eefConfig,'--database',join(scratch,'eef.db'),'--no-brain','--no-ui'],{EEF_NODE_PSK:randomBytes(32).toString('hex')});
 const nodeProcess=start('eefn',['--config',nodeConfig,'--no-ui']);await until(async()=>(await json(node+'/api/status')).connection.state==='connected','legacy connected');
 let report=await check();assert.equal(report.connection,'connected');assert.equal(report.transport,'legacy_shared_key');assert.equal(report.saved_auth_mode,'legacy');assert.equal(report.execution_receipts_available,false);
 assert.equal(report.input_channel,'ready');assert.equal((await json(node+'/api/diagnostics')).input_channel,'ready');
 assert.equal(report.applied_interpreter.state,'missing');assert(report.next_steps.includes('pair_private'));assert(report.next_steps.includes('choose_interpreter'));
 const plain=await cli(['setup'],false);assert(plain.out.includes('Node setup: setup-fixture'));assert(plain.out.includes('connection alone cannot enable reviewed work'));assert(plain.out.includes('No interpreter selected'));assert(!plain.out.trim().startsWith('{'));
 await cli(['models','select-ollama','--model','fixture-interpreter','--modality','text']);
 await cli(['models','interpreter','--backend','ollama','--model','fixture-interpreter']);
 report=await check();assert.equal(report.saved_interpreter.model.model_id,'fixture-interpreter');assert.equal(report.applied_interpreter.state,'missing');assert.equal(report.saved_matches_applied,false);assert(report.next_steps.includes('restart_for_saved_settings'));
 await cli(['restart']);report=await check();assert.equal(report.applied_interpreter.model.model_id,'fixture-interpreter');assert.equal(report.saved_matches_applied,true);
 assert((await cli(['setup'],false)).out.includes('inference and quality not tested'));
 const invitation=await owner(['credentials','provision','--node','setup-fixture','--address',`127.0.0.1:${gateway}`]);const privateFile=join(scratch,'PRIVATE-pair.json');await writeFile(privateFile,JSON.stringify(invitation));
 const old=(await json(`http://127.0.0.1:${web}/api/diagnostics`)).runtime_id;await owner(['restart']);await until(async()=>(await json(`http://127.0.0.1:${web}/api/diagnostics`)).runtime_id!==old,'coordinator restart');
 await cli(['connection','pair','--code-file',privateFile]);await until(async()=>{const s=await json(node+'/api/status');return s.connection.state==='connected'&&s.connection.transport==='pinned_tls13'&&s.execution_grants.available===true&&s.input_channel==='ready'&&s.registration==='accepted';},'connected private input channel and acknowledged registration');
 report=await check();assert.equal(report.transport,'pinned_tls13');assert.equal(report.saved_auth_mode,'node_credential');assert.equal(report.execution_receipts_available,true);assert(!report.next_steps.includes('pair_private'));assert.equal(report.applied_interpreter.model.model_id,'fixture-interpreter');
 await cli(['connection','pause']);await until(async()=>(await json(node+'/api/status')).connection.state==='paused','paused');report=await check();assert.equal(report.input_channel,'unavailable');assert.equal(report.saved_connection_enabled,false);assert(report.next_steps.includes('resume_connection'));assert.equal(report.applied_interpreter.state,'selected');
 await cli(['connection','resume']);await until(async()=>{const s=await json(node+'/api/status');return s.connection.state==='connected'&&s.registration==='accepted';},'resumed and registration acknowledged');
 assert.equal((await check()).input_channel,'ready');
 const diag=await json(node+'/api/diagnostics');const request={schema_version:1,expected_node_id:'setup-fixture',expected_runtime_id:diag.runtime_id,request_id:randomUUID()};
 for(const patch of [{expected_node_id:'other'},{expected_runtime_id:randomUUID()},{unexpected:true}]){const r=await fetch(node+'/api/commands/setup/check',{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify({...request,...patch})});assert.equal(r.status,patch.unexpected?422:400);}
 const cross=await fetch(node+'/api/commands/setup/check',{method:'POST',headers:{'Content-Type':'application/json',Origin:'https://untrusted.example'},body:JSON.stringify(request)});assert.equal(cross.status,403);
 const oversize=await fetch(node+'/api/commands/setup/check',{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify({...request,padding:'x'.repeat(4096)})});assert.equal(oversize.status,413);
 for(const mode of ['corrupt','prose','drop','old','bad-registration']){
  const marker=nodeConfig+'.api.json',original=await readFile(marker,'utf8'),endpoint=JSON.parse(original);endpoint.address=`127.0.0.1:${proxy.address().port}`;proxyMode=mode;const before=proxyCalls;
  await writeFile(marker,JSON.stringify(endpoint));try{const failure=await cli(['setup'],true,false);assert.equal(failure.success,false);assert(!JSON.stringify(failure).includes('PRIVATE-MUST-NOT-ECHO'));assert(!JSON.stringify(failure).includes('PRIVATE-INVALID-REGISTRATION'));assert.equal(proxyCalls,before+1);}finally{await writeFile(marker,original);}
 }
 {
  const marker=nodeConfig+'.api.json',original=await readFile(marker,'utf8'),endpoint=JSON.parse(original);endpoint.address=`127.0.0.1:${proxy.address().port}`;proxyMode='legacy-input';
  await writeFile(marker,JSON.stringify(endpoint));try{const legacy=await cli(['setup']);assert.equal(legacy.input_channel,undefined);assert((await cli(['setup'],false)).out.includes('Unknown on this node build; not assumed ready'));}finally{await writeFile(marker,original);}
 }
 assert.equal(modelCalls,0,'Inspection/setup controls never infer, load or download a model');
 {
  const marker=nodeConfig+'.api.json',original=await readFile(marker,'utf8'),endpoint=JSON.parse(original);endpoint.address=`127.0.0.1:${proxy.address().port}`;proxyMode='legacy-registration';
  await writeFile(marker,JSON.stringify(endpoint));try{const legacy=await cli(['setup']);assert.equal(legacy.registration,undefined);assert((await cli(['setup'],false)).out.includes('Registration: unknown'));}finally{await writeFile(marker,original);}
 }
 nodeProcess.kill();await nodeProcess.result;const stoppedBefore=await readFile(nodeConfig,'utf8');
 const stopped=await cli(['setup'],true,false);assert.equal(stopped.success,false);assert.equal(await readFile(nodeConfig,'utf8'),stoppedBefore);
 await assert.rejects(fetch(node+'/api/status',{signal:AbortSignal.timeout(1000)}),'Stopped inspection cannot start a new node API');
 await writeFile(join(scratch,'results.json'),JSON.stringify({passed:true,physical_two_pc:false,no_ui_controller:true,real_model_inference:false,plain_text_and_json:true,
  read_only_config_preserved:true,legacy_connection_not_reviewed_work:true,saved_applied_interpreter_distinct:true,pinned_transport_configuration_observed:true,
  paused_core_remains_inspectable:true,exact_node_runtime_and_cross_origin_checks:true,lost_corrupt_unsupported_no_retry_or_fallback:true,
  bounded_request_body:true,stopped_node_not_launched:true,input_queue_observed_without_inference:true,legacy_missing_input_channel_stays_unknown:true,
  registration_ack_observed:true,legacy_missing_registration_stays_unknown:true,invalid_registration_not_echoed:true,
  model_downloads:false,automatic_permissions:false,automatic_pairing:false,automatic_bootstrap:false},null,2));
 console.log('Setup check validation passed: '+scratch);
}catch(error){await writeFile(join(scratch,'PRIVATE-failure.json'),JSON.stringify({message:String(error)},null,2));await writeFile(join(scratch,'PRIVATE-logs.json'),JSON.stringify(children.map(c=>({exe:c.spawnfile,code:c.exitCode,...c.logs()})),null,2));throw error;
}finally{for(const c of children)if(c.exitCode===null)c.kill();await Promise.allSettled(children.map(c=>c.result));for(const server of [fake,proxy]){server.closeAllConnections();await new Promise(r=>server.close(r));}}
