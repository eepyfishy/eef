// Actual CLI/API private-input adapters, isolated one-PC processes. No models,
// installed nodes, private invitation files, workload grants or UI dependency.
import assert from 'node:assert/strict';
import {mkdtemp,readFile,writeFile} from 'node:fs/promises';
import {resolve,join} from 'node:path';
import {spawn} from 'node:child_process';
import {createServer} from 'node:net';
import {createServer as httpServer} from 'node:http';
import {randomBytes,randomUUID} from 'node:crypto';
const root=resolve(import.meta.dirname,'..'),bin=resolve(process.env.EEF_TEST_BINARY_DIR||join(root,'target/debug'));
const scratch=await mkdtemp(join(root,'.validation/private-pairing-'));
const nodeConfig=join(scratch,'node.json'),eefConfig=join(scratch,'eef.yaml'),children=[];
const env={...process.env,EEF_NODE_PSK:'',APPDATA:join(scratch,'appdata'),EEF_DISCOVERY_DIR:join(scratch,'discovery'),
 PATH:join(root,'.tooling/llvm-mingw-20260616-ucrt-x86_64/bin')+';'+process.env.PATH};
function launch(name,args,extra={},input,hold=false){
 const child=spawn(join(bin,name+'.exe'),args,{cwd:root,windowsHide:true,env:{...env,...extra},stdio:[input===undefined?'ignore':'pipe','pipe','pipe']});
 let out='',err='';child.stdout.on('data',b=>out=(out+b).slice(-1048576));child.stderr.on('data',b=>err=(err+b).slice(-1048576));
 child.result=new Promise((r,j)=>{child.on('error',j);child.on('close',code=>r({code,out,err}));});children.push(child);
 if(input!==undefined){child.stdin.on('error',()=>{});if(hold)child.stdin.write(input);else child.stdin.end(input);}
 return child;
}
async function cli(name,config,args,success=true,input,hold=false){
 const child=launch(name,['--config',config,'--json',...args],{},input,hold),timer=setTimeout(()=>child.kill(),30000);
 try{const result=await child.result;assert.equal(result.code,success?0:1,'CLI exit code mismatch (private output withheld)');
  return {value:JSON.parse(result.out),output:result.out+result.err};}finally{clearTimeout(timer);}
}
const owner=async(args)=>(await cli('eef',eefConfig,args)).value;
const local=async(args)=>(await cli('eefn',nodeConfig,args)).value;
async function port(){const s=createServer();await new Promise(r=>s.listen(0,'127.0.0.1',r));const p=s.address().port;await new Promise(r=>s.close(r));return p;}
async function json(url){const r=await fetch(url,{signal:AbortSignal.timeout(5000)});assert(r.ok);return r.json();}
async function until(fn,label){const end=Date.now()+45000;while(Date.now()<end){try{if(await fn())return;}catch{}await new Promise(r=>setTimeout(r,150));}throw Error('Timeout: '+label);}
let node,eef,invitation,credential,proxy,proxyMode='drop',forwarded=0,proxyError;
function noSecret(output){assert(!output.includes(invitation.code),'private code leaked');assert(!output.includes(credential),'private credential leaked');}
try{
 const [apiPort,eefPort,gatewayPort]=await Promise.all([port(),port(),port()]);node=`http://127.0.0.1:${apiPort}`;eef=`http://127.0.0.1:${eefPort}`;
 const yaml=(await readFile(join(root,'config/default_identity.yaml'),'utf8')).replace('port: 51334',`port: ${eefPort}`).replace('port: 51335',`port: ${gatewayPort}`).replace('policy: prompt','policy: off');
 await writeFile(eefConfig,yaml);
 launch('eef',['--config',eefConfig,'--database',join(scratch,'eef.db'),'--no-brain','--no-ui'],{EEF_NODE_PSK:randomBytes(24).toString('hex')});
 await until(async()=>(await json(eef+'/api/diagnostics')).initialized,'coordinator startup');
 const config=JSON.parse(await readFile(join(root,'config/node.example.json'),'utf8'));
 Object.assign(config,{node_id:'private-pairing-node',name:'Private pairing fixture',connection_enabled:false,auto_local:false,local_pairing:false,
  endpoints:[],psk:'',dashboard:{enabled:true,ui_enabled:false,host:'127.0.0.1',port:apiPort},update:{policy:'off'},
  models:{provider:'llamacpp',llamacpp:{slots:[]}},custom:{preserve:true}});
 await writeFile(nodeConfig,JSON.stringify(config));launch('eefn',['--config',nodeConfig,'--no-ui']);
 await until(async()=>(await json(node+'/api/status')).connection.state==='paused','paused API-only node');
 assert.equal((await fetch(node+'/')).status,404);
 invitation=await owner(['credentials','provision','--node',config.node_id,'--address',`127.0.0.1:${gatewayPort}`]);
 const decoded=JSON.parse(Buffer.from(invitation.code,'base64'));credential=decoded.psk;
 const previous=(await json(node+'/api/diagnostics')).runtime_id;
 const request={schema_version:1,expected_node_id:config.node_id,expected_runtime_id:previous,request_id:randomUUID(),code:invitation.code};
 const endpoint=node+'/api/commands/connection/pair',before=await readFile(nodeConfig);
 for(const patch of [{expected_node_id:'wrong'},{expected_runtime_id:randomUUID()},{code:'private-malformed-token'},{permissions:true}]){
  const reply=await fetch(endpoint,{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify({...request,...patch})});
  assert([400,422].includes(reply.status));noSecret(await reply.text());assert.deepEqual(await readFile(nodeConfig),before);
 }
 const cross=await fetch(endpoint,{method:'POST',headers:{'Content-Type':'application/json',Origin:'https://untrusted.example'},body:JSON.stringify(request)});
 assert.equal(cross.status,403);assert.deepEqual(await readFile(nodeConfig),before);
 const oversized=await fetch(endpoint,{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify({...request,code:'x'.repeat(8192)})});
 assert.equal(oversized.status,413);assert.deepEqual(await readFile(nodeConfig),before);
 const invalid=await cli('eefn',nodeConfig,['connection','pair-stdin'],false,'private-malformed-token');
 noSecret(invalid.output);assert(!invalid.output.includes('private-malformed-token'));assert.deepEqual(await readFile(nodeConfig),before);
 // Piped UTF-16 report through the actual CLI; no private file or argv secret.
 const input=Buffer.concat([Buffer.from([0xff,0xfe]),Buffer.from(JSON.stringify(invitation),'utf16le')]);
 const paired=await cli('eefn',nodeConfig,['connection','pair-stdin'],true,input);noSecret(paired.output);
 assert.equal(paired.value.report_type,'node_credential_pairing');assert.equal(paired.value.saved,true);
 assert.equal(paired.value.previous_runtime_id,previous);assert.equal(paired.value.restart_requested,true);
 assert.equal(paired.value.completed,false);assert.equal(paired.value.execution_authorized,false);
 assert.equal(paired.value.saved_connection_enabled,false);assert.equal(paired.value.durable_receipt,false);
 await until(async()=>{const d=await json(node+'/api/diagnostics');return d.runtime_id!==previous&&d.connection_state==='paused';},'requested node restart, still paused');
 let saved=JSON.parse(await readFile(nodeConfig,'utf8'));
 for(const field of ['node_id','name','permissions','models','custom','connection_enabled'])assert.deepEqual(saved[field],config[field]);
 assert.equal(saved.psk,credential);assert.equal(saved.coordinator_certificate,decoded.coordinator_certificate);
 assert.equal((await owner(['credentials','show'])).restart_required,true,'pairing cannot apply coordinator policy');
 assert.equal((await json(eef+'/api/diagnostics')).registered_node_count,0);
 let old=(await json(eef+'/api/diagnostics')).runtime_id;await owner(['restart']);
 await until(async()=>(await json(eef+'/api/diagnostics')).runtime_id!==old,'explicit coordinator restart');
 assert.equal((await json(node+'/api/status')).connection.state,'paused');
 await local(['connection','resume']);
 await until(async()=>(await json(eef+'/api/diagnostics')).registered_node_count===1,'explicit resume and pinned registration');
 assert.equal((await json(node+'/api/status')).connection.transport,'pinned_tls13');
 await local(['connection','pause']);await until(async()=>(await json(node+'/api/status')).connection.state==='paused','pause before reply-path tests');
 const marker=nodeConfig+'.api.json';
 proxy=httpServer(async(req,res)=>{
  try{
   let bytes='';for await(const b of req){bytes+=b;assert(bytes.length<=8192);}
   if(req.url==='/api/commands/connection/pair'&&proxyMode==='old'){res.writeHead(404);return res.end('{}');}
   if(req.url==='/api/commands/connection/pair'&&proxyMode==='unsafe-error'){res.writeHead(400);return res.end(JSON.stringify({error:invitation.code}));}
   const response=await fetch(node+req.url,{method:req.method,headers:{'Content-Type':'application/json'},body:req.method==='POST'?bytes:undefined,signal:AbortSignal.timeout(10000)});
   const text=await response.text();
   if(req.url==='/api/commands/connection/pair'){
    assert(response.ok,'real pairing save must succeed');forwarded++;const accepted=JSON.parse(text);assert.equal(accepted.saved,true);noSecret(text);
    if(proxyMode==='drop'){res.destroy();return;}
    const wrong=structuredClone(accepted);
    if(proxyMode==='corrupt')wrong.request_id=randomUUID();
    if(proxyMode==='extra-private')wrong.code=invitation.code;
    if(proxyMode==='private-note')wrong.note=credential;
    res.setHeader('Content-Type','application/json');return res.end(JSON.stringify(wrong));
   }
   res.writeHead(response.status,{'Content-Type':'application/json'});res.end(text);
  }catch(error){proxyError=error;res.destroy();}
 });
 await new Promise(r=>proxy.listen(0,'127.0.0.1',r));
 const useProxy=async()=>{const value=JSON.parse(await readFile(marker,'utf8'));value.address=`127.0.0.1:${proxy.address().port}`;await writeFile(marker,JSON.stringify(value));};
 for(const mode of ['drop','corrupt','extra-private','private-note']){
  proxyMode=mode;await useProxy();old=(await json(node+'/api/diagnostics')).runtime_id;
  const count=forwarded,result=await cli('eefn',nodeConfig,['connection','pair-stdin'],false,invitation.code+'\n');noSecret(result.output);
  assert.equal(result.value.error_code,'local_pairing_reply_unconfirmed');assert.equal(result.value.outcome_unknown,true);
  assert.equal(result.value.saved,null);assert.equal(result.value.automatically_retried,false);assert.equal(forwarded,count+1);assert.equal(proxyError,undefined);
  await until(async()=>(await json(node+'/api/diagnostics')).runtime_id!==old,'real save/restart despite unconfirmed reply');
  assert.equal(JSON.parse(await readFile(nodeConfig,'utf8')).psk,credential);
 }
 for(const mode of ['old','unsafe-error']){
  proxyMode=mode;await useProxy();const count=forwarded,before=await readFile(nodeConfig);
  const result=await cli('eefn',nodeConfig,['connection','pair-stdin'],false,JSON.stringify(invitation));noSecret(result.output);
  assert.equal(result.value.error_code,mode==='old'?'unsupported_command':'local_pairing_reply_unconfirmed');
  assert.equal(result.value.outcome_unknown,mode!=='old');assert.equal(forwarded,count);assert.deepEqual(await readFile(nodeConfig),before);
 }
 // A stalled pipe must exit on the input deadline, without ever posting pairing.
 proxyMode='drop';await useProxy();const count=forwarded,stalledStart=performance.now(),beforeStall=await readFile(nodeConfig);
 const stalled=await cli('eefn',nodeConfig,['connection','pair-stdin'],false,'private-incomplete-input',true);
 assert(performance.now()-stalledStart<24000,'stdin timeout cannot wait for EOF forever');
 assert(!stalled.output.includes('private-incomplete-input'));assert.equal(forwarded,count);assert.deepEqual(await readFile(nodeConfig),beforeStall);
 const original={schema_version:1,node_id:config.node_id,address:`127.0.0.1:${apiPort}`};await writeFile(marker,JSON.stringify(original));
 const inspected=await local(['connection','show']);noSecret(JSON.stringify(inspected));assert.equal(inspected.saved_auth_mode,'node_credential');
 const results={passed:true,api_only:true,private_file_free:true,stdin_utf16_and_report:true,exact_node_runtime_and_tls:true,
  paused_and_feature_policy_preserved:true,restart_requested_not_completed:true,explicit_coordinator_restart_and_resume:true,
  body_and_input_bounds:true,cross_origin_denied:true,lost_and_substituted_reply_no_replay:true,unsupported_no_fallback:true,
  arbitrary_error_body_not_echoed:true,stalled_pipe_timeout:true,physical_two_pc:false,model_quality_accepted:false};
 await writeFile(join(scratch,'results.json'),JSON.stringify(results,null,2));console.log('Private pairing validation passed: '+scratch);
}finally{
 for(const child of children)if(child.exitCode===null)child.kill();await Promise.allSettled(children.map(c=>c.result));
 if(proxy){proxy.closeAllConnections();await new Promise(r=>proxy.close(r));}
}
