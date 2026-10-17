// Test the opt-in observer with a synthetic provider, not real-model acceptance.
import assert from 'node:assert/strict';
import {createServer} from 'node:http';
import {spawn} from 'node:child_process';
import {readFile} from 'node:fs/promises';
import {resolve,join} from 'node:path';
const root=resolve(import.meta.dirname,'..'),model='synthetic-context-fixture',digest='b'.repeat(64);
let mode='valid',calls=[];
const server=createServer(async(req,res)=>{
 res.setHeader('Content-Type','application/json');
 if(req.url==='/api/tags')return res.end(JSON.stringify({models:[{name:model,digest,size:1}]}));
 if(req.url==='/api/version')return res.end(JSON.stringify({version:'synthetic-fixture'}));
 if(req.url==='/api/ps')return res.end(JSON.stringify({models:calls.length?[{name:model,digest,context_length:4096,size:1,size_vram:0}]:[]}));
 if(req.url==='/api/show')return res.end(JSON.stringify({capabilities:['completion','thinking']}));
 if(req.url==='/api/chat'){
  let raw='';for await(const bytes of req)raw+=bytes;const body=JSON.parse(raw);calls.push(body);
  if(body.messages.length===0)return res.end(JSON.stringify({done:true,done_reason:'load',message:{role:'assistant',content:''}}));
  const proposal={schema_version:1,intent:'information',goal:'Inspect node status',context_hints:['this node'],constraints:[],
   suggested_capabilities:['system.info'],complexity:'simple',workload:'one_shot',needs_clarification:false,clarification:null};
  if(mode==='rejected')proposal.PRIVATE_INVALID_MODEL_FIELD=true;
  return res.end(JSON.stringify({message:{content:JSON.stringify(proposal)},done:true,done_reason:'stop'}));
 }
 res.writeHead(404);res.end('{}');
});
try{
 await new Promise(r=>server.listen(0,'127.0.0.1',r));
 for(mode of ['valid','rejected','prepared']){
  calls=[];
  const child=spawn(process.execPath,[join(root,'tools/test-real-ollama-context.mjs'),model,'http://127.0.0.1:'+server.address().port,...(mode==='prepared'?['--prepare']:[])],
   {cwd:root,windowsHide:true,env:{...process.env,EEF_TEST_ALLOW_SHARED_OLLAMA:'1'},stdio:['ignore','pipe','pipe']});
  let out='',err='';child.stdout.on('data',b=>out+=b);child.stderr.on('data',b=>err+=b);
  const timer=setTimeout(()=>child.kill(),45000);
  let code;try{code=await new Promise((r,j)=>{child.on('error',j);child.on('close',r);});}finally{clearTimeout(timer);}
  assert.equal(code,mode==='rejected'?1:0,err);
  const path=out.match(/Shared-Ollama context evidence: (.+)/)?.[1]?.trim();assert(path,'Missing observation evidence');
  const report=JSON.parse(await readFile(join(path,'results.json'),'utf8'));
  assert.equal(report.passed,mode!=='rejected');assert.equal(report.owner_preview_commands,1);assert.equal(calls.length,mode==='prepared'?2:1);
  assert.equal(report.observed_context_tokens,4096);assert.equal(report.saved_configuration_unchanged,true);
  assert.equal(report.model_quality_approved,false);assert.equal(report.model_file_hash_verified,false);
  assert.equal(report.execution_authorized,false);assert.equal(report.preview.automatically_retried,false);
  for(const call of calls){assert.equal(call.options.num_ctx,4096);assert.equal(call.keep_alive,60);}
  assert.equal(calls.at(-1).think,false);
  assert.equal(calls.at(-1).messages.at(-1).content,'Show the operating system and memory usage of this node.');
  if(mode==='prepared'){
   assert.equal(report.preparation_commands,1);assert.equal(report.preparation_report.receipt.phase,'completed');
   assert.deepEqual(calls[0].messages,[]);assert.equal(calls[0].think,undefined);
  }
  if(mode==='rejected'){
   assert.equal(report.preview.success,false);assert.equal(report.preview.error_code,'interpretation_invalid');
   assert(!JSON.stringify(report.preview).includes('PRIVATE_INVALID_MODEL_FIELD'));
  }
 }
 console.log('Shared-Ollama observer success/rejection checks passed with synthetic metadata/inference; no real provider used.');
}finally{server.closeAllConnections();await new Promise(r=>server.close(r));}
