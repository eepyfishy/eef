// Byte-preservation/privacy/admission checks; synthetic HTTP only, no model.
import assert from 'node:assert/strict';
import {createServer} from 'node:http';
import {mkdtemp,readFile} from 'node:fs/promises';
import {join,resolve} from 'node:path';
import {createHash} from 'node:crypto';
import {observeOllama} from './ollama-test-observer.mjs';
const directory=await mkdtemp(join(resolve(import.meta.dirname,'..'),'.validation/ollama-observer-'));
const input='Synthetic status ไทย',model='fixture';let mode='valid',received=[];
const upstream=createServer(async(req,res)=>{
 const chunks=[];for await(const chunk of req)chunks.push(chunk);received.push({path:req.url,body:Buffer.concat(chunks)});
 if(mode==='large')return res.end('x'.repeat(4*1024*1024+1));
 if(mode==='redirect'){res.writeHead(302,{Location:'http://example.invalid/not-followed'});return res.end('{}');}
 if(mode==='bad-utf8')return res.end(Buffer.from([0xc3,0x28]));
 if(req.url!=='/api/chat')return res.end('{}');
 res.setHeader('Content-Type','application/json');
 res.end(' { "done":true,"done_reason":"PRIVATE_REASON", "message":{"content":"","thinking":"PRIVATE_THINKING"},"eval_count":512 }\n');
});
await new Promise(r=>upstream.listen(0,'127.0.0.1',r));
const observer=await observeOllama({provider:'http://127.0.0.1:'+upstream.address().port,model,input,directory});
const payload={model,messages:[{role:'system',content:'Synthetic instructions'},{role:'user',content:input}],stream:false,
 think:false,options:{num_predict:512,num_ctx:4096},keep_alive:60};
const post=(body,path='/api/chat')=>fetch(observer.origin+path,{method:'POST',body,headers:{'Content-Type':'application/json'},signal:AbortSignal.timeout(10000)});
try{
 for(const path of ['/api/pull','/api/create','/api/chat?query=1'])assert.equal((await post('{}',path)).status,502);
 for(const change of [{model:'other'},{stream:true},{messages:[{role:'system',content:'x'},{role:'user',content:'Other input'}]},
  {tools:[{}]},{think:true},{options:{num_predict:1024,num_ctx:4096}}]){
  assert.equal((await post(JSON.stringify({...payload,...change}))).status,502);
 }
 assert.equal((await post('x'.repeat(256*1024+1))).status,502);assert.equal(received.length,0);
 const bytes=Buffer.from(' \n'+JSON.stringify(payload)+'\n');const response=await post(bytes);assert.equal(response.status,200);
 const raw=await response.text();assert(received[0].body.equals(bytes),'Request bytes changed');
 const captured=JSON.parse(await readFile(join(directory,'PRIVATE-observed-chat-1.json'),'utf8'));
 assert.equal(captured.request_body,bytes.toString());assert.equal(captured.response_body,raw,'Response bytes changed');
 assert.equal(observer.records.length,1);assert.equal(observer.records[0].finish_reason,'other');
 assert.equal(observer.records[0].response_sha256,createHash('sha256').update(raw).digest('hex'));
 assert.equal(observer.records[0].thinking_bytes,Buffer.byteLength('PRIVATE_THINKING'));
 assert(!JSON.stringify(observer.records).includes('PRIVATE_'),'Public metadata exposed provider strings');
 assert.equal((await post(bytes)).status,502);assert.equal(received.length,1,'Observer replayed an inference');
 mode='bad-utf8';const malformed=await post(JSON.stringify({...payload,messages:[],think:undefined}));
 const malformedBytes=Buffer.from(await malformed.arrayBuffer());assert(malformedBytes.equals(Buffer.from([0xc3,0x28])));
 const malformedCapture=JSON.parse(await readFile(join(directory,'PRIVATE-observed-chat-2.json'),'utf8'));
 assert(Buffer.from(malformedCapture.response_body_base64,'base64').equals(malformedBytes));assert.equal(observer.records[1].valid_response_json,false);
 mode='redirect';const redirect=await fetch(observer.origin+'/api/tags',{redirect:'manual'});assert.equal(redirect.status,302);assert.equal(redirect.headers.get('Location'),null);
 assert.equal(received.length,3);
 mode='large';assert.equal((await fetch(observer.origin+'/api/ps')).status,502);
 assert.equal(received.length,4);
 console.log('Ollama observer byte, privacy, bounds, route/model/input and no-replay checks passed with synthetic HTTP.');
}finally{await observer.close();upstream.closeAllConnections();await new Promise(r=>upstream.close(r));}
