// End-to-end evaluator protocol/safety checks with a synthetic shared provider.
// Deliberately unhelpful proposals exercise the frozen scorer, not model quality.
import assert from 'node:assert/strict';
import {createServer} from 'node:http';
import {spawn,execFile} from 'node:child_process';
import {readFile} from 'node:fs/promises';
import {join,resolve} from 'node:path';
import {caseSet} from './interpreter-generalization-cases.mjs';
import {grade} from './interpreter-quality-cases.mjs';
const root=resolve(import.meta.dirname,'..'),model='synthetic-quality-fixture',digest='e'.repeat(64);
let mode='valid',calls=[],preparations=0,resident=false,proxyCalls=0;const gates=new Set();
const proxy=createServer((req,res)=>{proxyCalls++;res.writeHead(502);res.end('{}');});
const fake=createServer(async(req,res)=>{
 res.setHeader('Content-Type','application/json');
 if(req.url==='/api/version')return res.end(JSON.stringify({version:'synthetic-evaluator-fixture'}));
 if(req.url==='/api/tags'){
  if(mode==='metadata-large')return res.end(JSON.stringify({models:[{name:model,size:1,digest}]})+' '.repeat(4*1024*1024));
  if(mode==='metadata-redirect'){res.writeHead(302,{Location:'http://example.invalid/not-followed'});return res.end('{}');}
  if(mode==='metadata-drip'){res.write(' ');const interval=setInterval(()=>res.write(' '),100);res.on('close',()=>clearInterval(interval));return;}
  return res.end(JSON.stringify({models:[{name:model,size:1,digest:mode==='changed'&&calls.length?'b'.repeat(64):digest}]}));
 }
 if(req.url==='/api/show')return res.end(JSON.stringify({capabilities:['completion']}));
 if(req.url==='/api/ps'){
  const value={name:model,digest:mode==='unrelated'&&calls.length?'c'.repeat(64):digest,
   context_length:mode==='valid-unknown-context'?null:mode==='context'&&calls.length?8192:4096,size:1,size_vram:0};
  return res.end(JSON.stringify({models:resident?(mode==='duplicate'&&calls.length?[value,value]:[value]):[]}));
 }
 if(req.url==='/api/chat'){
  let raw='';for await(const bytes of req)raw+=bytes;const value=JSON.parse(raw);
  assert.equal(value.model,model);assert.equal(value.stream,false);assert.equal(value.options.num_ctx,4096);assert.equal(value.keep_alive,60);
  resident=true;if(value.messages.length===0){preparations++;return res.end(JSON.stringify({done:true,done_reason:'load',message:{content:''}}));}
  assert.equal(value.options.num_predict,512);assert.equal(value.think,false);calls.push(value);
  if(mode==='timeout'){res.write(' ');gates.add(res);res.on('close',()=>gates.delete(res));return;}
  const proposal={schema_version:1,intent:'information',goal:'Synthetic protocol observation',context_hints:[],constraints:[],
   suggested_capabilities:['system.info'],complexity:'simple',workload:'one_shot',needs_clarification:false,clarification:null};
  return res.end(JSON.stringify({done:true,done_reason:'stop',message:{content:JSON.stringify(proposal)}}));
 }
 res.writeHead(404);res.end('{}');
});
try{
 await new Promise(r=>fake.listen(0,'127.0.0.1',r));
 await new Promise(r=>proxy.listen(0,'127.0.0.1',r));
 for(mode of ['valid','valid-proxy','valid-unknown-context','timeout','changed','context','unrelated','duplicate','metadata-large','metadata-redirect','metadata-drip']){
  calls=[];preparations=0;resident=false;
  const success=mode.startsWith('valid'),metadata=mode.startsWith('metadata-'),started=Date.now();
  const child=spawn(process.execPath,[join(root,'tools/test-interpreter-quality.mjs'),'--ollama',model,'http://127.0.0.1:'+fake.address().port],
   {cwd:root,windowsHide:true,env:{...process.env,EEF_TEST_ALLOW_SHARED_OLLAMA:'1',EEF_TEST_MODEL_FILE:'',EEF_TEST_MODEL_ID:'',
    EEF_TEST_QUALITY_SUITE:'original-v1',EEF_TEST_INFERENCE_TIMEOUT_SECONDS:'1',EEF_TEST_THINKING_MODE:'disabled',EEF_TEST_INTERPRETER_SAMPLING_ARGS:'[]',
    ...(mode==='valid-proxy'?{NODE_USE_ENV_PROXY:'1',HTTP_PROXY:'http://127.0.0.1:'+proxy.address().port,
     http_proxy:'http://127.0.0.1:'+proxy.address().port,NO_PROXY:'',no_proxy:''}:{})},stdio:['ignore','pipe','pipe']});
  let out='',err='';child.stdout.on('data',b=>out+=b);child.stderr.on('data',b=>err+=b);
  const timer=setTimeout(()=>{if(Number.isInteger(child.pid)&&child.pid>0){
   if(process.platform==='win32')execFile('taskkill.exe',['/PID',String(child.pid),'/T','/F'],{windowsHide:true},()=>{});else child.kill();
  }},metadata?10000:60000);
  let code;try{code=await new Promise((r,j)=>{child.on('error',j);child.on('close',r);});}finally{clearTimeout(timer);}
  assert.equal(code,success?0:1,err);
  if(metadata){
   assert.equal(calls.length,0);assert.equal(preparations,0);assert(!out.includes('Production interpreter evaluation:'));
   assert(Date.now()-started<8000);assert.match(err,mode==='metadata-large'?/metadata too large/:mode==='metadata-drip'?/metadata timeout/:/302/);continue;
  }
  const path=out.match(/Production interpreter evaluation: (.+)/)?.[1]?.trim();assert(path,'Missing evaluator evidence');
  const report=JSON.parse(await readFile(join(path,'results.json'),'utf8'));
  assert.equal(report.backend,'ollama');assert.equal(report.shared_ollama_provider,true);assert.equal(report.real_cpu_interpretation,false);
  assert.equal(report.shared_service_owned,false);assert.equal(report.model_file_hash_verified,false);assert.equal(report.model_process_sample,null);
  for(const field of ['sha256','bytes','runtime_sha256','gpu_layers','threads'])assert(!Object.hasOwn(report,field));
  assert.equal(report.provider_reported_model_digest,digest);assert.equal(report.preparation_commands,1);assert.equal(preparations,1);
  assert.equal(report.default_approved,false);assert.equal(report.shared_service_stopped,false);assert.equal(report.force_unload_requested,false);
  const cases=caseSet('original-v1').cases;
  if(success){
   assert.equal(report.harness_completed,true);assert.equal(report.accepted_count,20);assert.equal(calls.length,20);
   assert.deepEqual(calls.map(c=>c.messages.at(-1).content),cases.map(c=>c.input));
   assert.equal(report.matched_count,cases.reduce((total,item,index)=>total+Number(grade(item,report.cases[index].reply).matched),0));
   assert(report.matched_count<20,'The scorer must not silently approve deliberately unhelpful proposals');
   if(mode==='valid-unknown-context')assert.equal(report.prepared_residency.context_tokens,null);
  }else{
   assert.equal(report.harness_completed,false);assert.equal(calls.length,1,'No case after uncertain provider state');assert.equal(report.cases.length,1);
   if(mode==='timeout')assert.equal(report.cases[0].reply.error_code,'interpretation_timeout');
  }
 }
 assert.equal(proxyCalls,0,'Provider and owner inspection must bypass process proxy settings');
 console.log('Shared-provider quality evaluator checks passed with synthetic inference, frozen grading and abort/no-replay checks.');
}finally{for(const gate of gates)gate.destroy();for(const server of [fake,proxy]){server.closeAllConnections();await new Promise(r=>server.close(r));}}
