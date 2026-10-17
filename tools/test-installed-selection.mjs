// Real owner CLI/API, synthetic node-owned cache bytes. Never load/download weights.
import assert from 'node:assert/strict';
import {mkdtemp,readFile,writeFile,mkdir,realpath,stat} from 'node:fs/promises';
import {resolve,join,toNamespacedPath} from 'node:path';
import {createHash} from 'node:crypto';
import {spawn} from 'node:child_process';
import {createServer} from 'node:http';
import {createServer as tcpServer} from 'node:net';
const root=resolve(import.meta.dirname,'..'),bin=resolve(process.env.EEF_TEST_BINARY_DIR||join(root,'target/debug'));
const scratch=await mkdtemp(join(root,'.validation/installed-selection-')),configPath=join(scratch,'node.json'),marker=configPath+'.api.json';
const children=[],body=Buffer.from('GGUF synthetic cache, not a valid inference model'),digest=createHash('sha256').update(body).digest('hex');
const cache=join(scratch,'models',digest+'.gguf'),indexPath=join(scratch,'models','installed.json');
const env={...process.env,EEF_NODE_PSK:'',APPDATA:join(scratch,'appdata'),EEF_DISCOVERY_DIR:join(scratch,'discovery'),
 PATH:join(root,'.tooling/llvm-mingw-20260616-ucrt-x86_64/bin')+';'+process.env.PATH};
function launch(args){const c=spawn(join(bin,'eefn.exe'),['--config',configPath,...args],{cwd:root,windowsHide:true,env,stdio:['ignore','pipe','pipe']});
 let out='',err='';c.stdout.on('data',b=>out=(out+b).slice(-1048576));c.stderr.on('data',b=>err=(err+b).slice(-1048576));
 c.logs=()=>({out,err});c.result=new Promise((r,j)=>{c.on('error',j);c.on('close',code=>r({code,out,err}));});children.push(c);return c;}
async function nodeCli(args,ok=true){const c=launch(['--json',...args]),timer=setTimeout(()=>c.kill(),85000);
 try{const r=await c.result;assert.equal(r.code,ok?0:1,r.out+r.err);return JSON.parse(r.out);}finally{clearTimeout(timer);}}
const cli=(args,ok=true)=>nodeCli(['models',...args],ok);
async function json(url,body,method){const r=await fetch(url,{method:method||(body?'POST':'GET'),headers:{'Content-Type':'application/json'},
 body:body?JSON.stringify(body):undefined,signal:AbortSignal.timeout(10000)});const v=await r.json();assert(r.ok,JSON.stringify(v));return v;}
async function until(fn,label){const end=Date.now()+45000;while(Date.now()<end){try{if(await fn())return;}catch{}await new Promise(r=>setTimeout(r,100));}throw Error('Timeout: '+label);}
async function listen(s){await new Promise(r=>s.listen(0,'127.0.0.1',r));return s.address().port;}
async function freePort(){const s=tcpServer();await new Promise(r=>s.listen(0,'127.0.0.1',r));const p=s.address().port;await new Promise(r=>s.close(r));return p;}
let inferences=0,node='',proxyMode='drop',forwarded=0,capturedRejection;
const provider=createServer(async(req,res)=>{res.setHeader('Content-Type','application/json');
 if(req.url==='/api/tags')return res.end(JSON.stringify({models:[{name:'other'}]}));
 if(req.url==='/api/show')return res.end(JSON.stringify({capabilities:['completion']}));
 if(req.url==='/api/chat'){inferences++;return res.end('{}');}res.writeHead(404);res.end('{}');});
const proxy=createServer(async(req,res)=>{try{
 if(proxyMode==='old'){res.writeHead(404,{'Content-Type':'application/json'});return res.end('{}');}
 let raw='';for await(const b of req)raw+=b;forwarded++;
 const r=await fetch(node+req.url,{method:'POST',headers:{'Content-Type':'application/json'},body:raw});
 const response=await r.text();
 if(proxyMode==='capture'){capturedRejection=JSON.parse(response);res.writeHead(r.status,{'Content-Type':'application/json'});return res.end(response);}
 if(proxyMode==='stale'){res.writeHead(200,{'Content-Type':'application/json'});return res.end(JSON.stringify(capturedRejection));}
 res.destroy();
}catch{res.destroy();}});
try{
 const providerPort=await listen(provider),proxyPort=await listen(proxy);
 await mkdir(join(scratch,'models'));await writeFile(cache,body);
 const entry={id:'cache-fixture',name:'Cache fixture',modality:'text',url:'https://example.invalid/never-fetched',sha256:digest,bytes:body.length,license:'test-only'};
 await writeFile(indexPath,JSON.stringify([{...entry,path:toNamespacedPath(await realpath(cache))}]));
 const config=JSON.parse(await readFile(join(root,'config/node.example.json'),'utf8'));
 Object.assign(config,{node_id:'installed-fixture-node',name:'Installed fixture',auto_local:false,local_pairing:false,endpoints:[],connection_enabled:false,
 dashboard:{enabled:true,ui_enabled:false,host:'127.0.0.1',port:await freePort()},update:{policy:'off'},model_catalog:[entry],
 models:{provider:'auto',ollama:{base_url:`http://127.0.0.1:${providerPort}`,selected:[{model_id:'other',modality:'text',selection:{roles:['request_interpreter','other_role']}}]},llamacpp:{slots:[]}}});
 await writeFile(configPath,JSON.stringify(config));
 const offline=await cli(['select-installed-gguf','--model',entry.id]);
 assert.equal(offline.running,false);assert.equal(offline.installed_artifact.verified_at_selection,true);
 assert.equal(children.some(c=>c.spawnargs.includes('--no-ui')),false,'offline edit must not launch a node');
 await cli(['remove','--backend','llamacpp','--model',entry.id]);assert.equal((await stat(cache)).size,body.length);
 const owner=launch(['--no-ui']);
 await until(async()=>{const m=JSON.parse(await readFile(marker,'utf8'));node='http://'+m.address;const s=await json(node+'/api/status');return s.connection.state==='paused'&&!!(await json(node+'/api/diagnostics')).runtime_id;},'running owner');
 assert.equal((await fetch(node+'/')).status,404);
 const runtime=(await json(node+'/api/diagnostics')).runtime_id;
 const appliedBefore=(await nodeCli(['setup'])).applied_interpreter;
 assert.equal(appliedBefore.model.model_id,'other');
 const liveBefore=(await json(node+'/api/status')).models;
 const before=(await json(node+'/api/config')).config;
 const selected=await cli(['select-installed-gguf','--model',entry.id,'--interpreter']);
 assert.equal(selected.running,true);assert.equal(selected.provider,'auto');assert.equal(selected.restart_required,true);
 assert.deepEqual(selected.installed_artifact,{model_id:entry.id,sha256:digest,bytes:body.length,verified_at_selection:true});
 assert.deepEqual(selected.registered_models.map(m=>m.model_id),liveBefore.map(m=>m.model_id));
 assert.equal(selected.saved_selections.filter(m=>m.model_metadata.roles?.includes('request_interpreter')).length,1);
 assert(selected.saved_selections.some(m=>m.backend==='llamacpp'&&m.model_id===entry.id&&m.model_metadata.roles.includes('request_interpreter')));
 const saved=(await json(node+'/api/config')).config;
 assert.deepEqual(saved.permissions,before.permissions);assert.equal(saved.models.provider,'auto');
 assert.deepEqual(saved.models.ollama.selected[0].selection.roles,['other_role']);
 assert(saved.models.llamacpp.slots[0].port>0);assert.equal(saved.models.llamacpp.slots[0].model_path,toNamespacedPath(await realpath(cache)));
 assert.equal((await json(node+'/api/diagnostics')).runtime_id,runtime);assert.equal(owner.exitCode,null);
 assert.deepEqual((await json(node+'/api/status')).models,liveBefore,'saved selection must not load the fake cache');
 assert.deepEqual((await nodeCli(['setup'])).applied_interpreter,appliedBefore);
 assert.equal((await json(node+'/api/status')).download.state,'idle');assert.equal(inferences,0);
 saved.models.llamacpp.slots[0].cpu_threads=2;saved.models.llamacpp.slots[0].context=3072;saved.models.llamacpp.slots[0].extension={owner:'keep'};
 saved.dashboard.port=Number(new URL(node).port);await json(node+'/api/config',{config:saved},'PUT');
 let raw=await readFile(configPath,'utf8');
 assert.equal((await cli(['select-installed-gguf','--model',entry.id])).changed,false);
 assert.equal(await readFile(configPath,'utf8'),raw,'existing tuning/roles/extensions must not be reserialized away');
 await writeFile(cache,Buffer.alloc(body.length,43));
 const corrupt=await cli(['select-installed-gguf','--model',entry.id],false);
 assert.equal(corrupt.error_code,'installed_selection_preflight_rejected');assert.equal(corrupt.automatically_retried,false);
 assert.equal(corrupt.cache_issue,'checksum_mismatch');assert.equal(corrupt.outcome_unknown,false);assert.equal(corrupt.configuration_write_attempted,false);
 assert.equal(await readFile(configPath,'utf8'),raw);assert.equal((await stat(cache)).size,body.length);
 await writeFile(cache,body);
 const indexRaw=await readFile(indexPath,'utf8'),altered=JSON.parse(indexRaw);altered[0].path=join(scratch,'not-a-cache-file');
 await writeFile(indexPath,JSON.stringify(altered));
 const mismatch=await cli(['select-installed-gguf','--model',entry.id],false);
 assert.equal(mismatch.cache_issue,'cache_path_mismatch');assert.equal(mismatch.outcome_unknown,false);assert.equal(mismatch.configuration_write_attempted,false);
 assert.equal(await readFile(configPath,'utf8'),raw);
 await writeFile(indexPath,indexRaw);
 await cli(['clear-interpreter']);
 const originalMarker=await readFile(marker,'utf8');
 try{
  const changed=JSON.parse(originalMarker);changed.address=`127.0.0.1:${proxyPort}`;await writeFile(marker,JSON.stringify(changed));
  const lost=await cli(['select-installed-gguf','--model',entry.id,'--interpreter'],false);
  assert.equal(lost.outcome_unknown,true);assert.equal(lost.automatically_retried,false);assert.equal(forwarded,1);
  await writeFile(marker,originalMarker);
  assert((await cli(['show'])).saved_selections.some(m=>m.model_id===entry.id&&m.model_metadata.roles.includes('request_interpreter')));
  await cli(['clear-interpreter']);
  await writeFile(cache,Buffer.alloc(body.length,43));raw=await readFile(configPath,'utf8');
  proxyMode='capture';await writeFile(marker,JSON.stringify(changed));
  const captured=await cli(['select-installed-gguf','--model',entry.id,'--interpreter'],false);
  assert.equal(captured.outcome_unknown,false);assert.equal(captured.cache_issue,'checksum_mismatch');
  assert.equal(await readFile(configPath,'utf8'),raw);
  await writeFile(cache,body);proxyMode='stale';
  const stale=await cli(['select-installed-gguf','--model',entry.id,'--interpreter'],false);
  assert.equal(stale.error_code,'local_model_installed_selection_reply_unconfirmed');assert.equal(stale.outcome_unknown,true);
  assert.notEqual(stale.command.request_id,capturedRejection.command.request_id);assert.equal(stale.automatically_retried,false);
  assert.equal(forwarded,3,'each explicit command is forwarded once, never replayed');
  await writeFile(marker,originalMarker);
  assert((await cli(['show'])).saved_selections.some(m=>m.model_id===entry.id&&m.model_metadata.roles.includes('request_interpreter')));
  proxyMode='old';await writeFile(marker,JSON.stringify(changed));raw=await readFile(configPath,'utf8');
  const unsupported=await cli(['select-installed-gguf','--model',entry.id],false);
  assert.equal(unsupported.error_code,'unsupported_command');assert.equal(unsupported.outcome_unknown,false);assert.equal(forwarded,3);
  assert.equal(await readFile(configPath,'utf8'),raw);
 }finally{await writeFile(marker,originalMarker);}
 assert.equal(inferences,0);assert.equal((await json(node+'/api/diagnostics')).runtime_id,runtime);
 await writeFile(join(scratch,'results.json'),JSON.stringify({passed:true,api_only:true,synthetic_cache_only:true,
  offline_no_launch:true,local_cli_no_paths_or_ports:true,verified_selection:true,explicit_interpreter_role:true,
  provider_permissions_preserved:true,saved_not_applied:true,owner_tuning_extensions_preserved:true,corruption_and_index_rejected:true,
  preflight_rejection_confirmed:true,stale_rejection_refused:true,lost_reply_no_replay:true,unsupported_no_fallback:true,model_downloads:false,real_model_loading:false,physical_two_pc:false},null,2));
 console.log('Installed selection checks passed: '+scratch);
}catch(error){
 await writeFile(join(scratch,'PRIVATE-failure.json'),JSON.stringify({error:String(error),children:children.map(c=>({pid:c.pid,exit_code:c.exitCode,...c.logs()}))},null,2));
 throw error;
}finally{
 for(const c of children)if(c.exitCode===null)c.kill();await Promise.allSettled(children.map(c=>c.result));
 for(const s of [provider,proxy]){s.closeAllConnections();await new Promise(r=>s.close(r));}
}
