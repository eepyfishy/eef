// Test-only loopback observer: forwards bounded bytes without rewriting model
// requests/responses. Never load this from production or proxy arbitrary inputs.
import assert from 'node:assert/strict';
import {createServer,request} from 'node:http';
import {writeFile} from 'node:fs/promises';
import {join} from 'node:path';
import {createHash} from 'node:crypto';

export async function observeOllama({provider,model,input,directory}){
 const origin=new URL(provider);
 assert(origin.protocol==='http:'&&['127.0.0.1','[::1]'].includes(origin.hostname)&&!origin.username&&!origin.password&&origin.pathname==='/'&&!origin.search&&!origin.hash);
 const records=[],upstreams=new Set(),pending=new Set();let inference=0,preparation=0,sequence=0;
 const server=createServer((incoming,outgoing)=>{
  const task=(async()=>{
   if(!['127.0.0.1','::1','::ffff:127.0.0.1'].includes(incoming.socket.remoteAddress))throw Error('Non-loopback peer');
   const route=incoming.url,method=incoming.method;
   assert((method==='GET'&&['/api/tags','/api/ps'].includes(route))||(method==='POST'&&['/api/show','/api/chat'].includes(route)),'Observer route refused');
   const chunks=[];let size=0;
   for await(const bytes of incoming){size+=bytes.length;assert(size<=256*1024,'Observer request too large');chunks.push(bytes);}
   const body=Buffer.concat(chunks);let chat=false,index;
   if(method==='POST'){
    const parsed=JSON.parse(new TextDecoder('utf-8',{fatal:true}).decode(body));assert.equal(parsed.model,model,'Observer model mismatch');
    if(route==='/api/chat'){
     assert.equal(parsed.stream,false);assert(Array.isArray(parsed.messages));
     assert.equal(parsed.options?.num_ctx,4096);assert.equal(parsed.keep_alive,60);
     assert.equal(parsed.tools,undefined);assert.equal(parsed.images,undefined);
     if(parsed.messages.length===0){assert.equal(preparation++,0,'No preparation retry allowed');}
     else {assert.equal(parsed.messages.length,2);assert.equal(parsed.messages.at(-1).role,'user');
      assert.equal(parsed.messages[0].role,'system');assert.equal(parsed.options.num_predict,512);
      assert(parsed.think===undefined||parsed.think===false,'Observer refuses an enabled thinking override');
      assert.equal(parsed.messages.at(-1).content,input,'Only the declared synthetic input may be observed');
      assert.equal(inference++,0,'No inference retry allowed');}
     chat=true;index=++sequence;
    }
   }
   await new Promise((done,fail)=>{
    const upstream=request(new URL(route,origin),{method,headers:method==='POST'?{'Content-Type':'application/json','Content-Length':body.length}:{}},response=>{
     let length=0,blocked=false;const received=[];
     response.on('data',bytes=>{length+=bytes.length;if(length>4*1024*1024){blocked=true;fail(Error('Observer response too large'));upstream.destroy();}else received.push(bytes);});
     response.on('error',fail);
     response.on('end',()=>{(async()=>{
      assert(!blocked,'Observer response was rejected');
      const raw=Buffer.concat(received);
      if(chat){
       const parsed=JSON.parse(body);let value;try{value=JSON.parse(new TextDecoder('utf-8',{fatal:true}).decode(raw));}catch{}
       const content=typeof value?.message?.content==='string'?value.message.content:null;
       const thinking=typeof value?.message?.thinking==='string'?value.message.thinking:null;
       const hash=value=>value===null?null:createHash('sha256').update(value).digest('hex');
       const reason=value?.done_reason;
       records.push({index,preparation:parsed.messages.length===0,status:response.statusCode,
        request_sha256:hash(body),response_sha256:hash(raw),response_bytes:raw.length,
        requested_think:parsed.think??null,requested_output_tokens:parsed.options?.num_predict??null,
        requested_context_tokens:parsed.options?.num_ctx??null,valid_response_json:value!==undefined,
        done:typeof value?.done==='boolean'?value.done:null,
        finish_reason:['stop','length','load','tool_calls','function_call','content_filter'].includes(reason)?reason:reason==null?null:'other',
        eval_count:Number.isSafeInteger(value?.eval_count)&&value.eval_count>=0?value.eval_count:null,
        content_bytes:content===null?null:Buffer.byteLength(content),thinking_bytes:thinking===null?null:Buffer.byteLength(thinking),
        content_sha256:hash(content),thinking_sha256:hash(thinking)});
       await writeFile(join(directory,`PRIVATE-observed-chat-${index}.json`),JSON.stringify({request_body:body.toString('utf8'),response_body:raw.toString('utf8'),
        request_body_base64:body.toString('base64'),response_body_base64:raw.toString('base64')},null,2),{flag:'wx'});
      }
      outgoing.writeHead(response.statusCode,{'Content-Type':response.headers['content-type']||'application/json'});outgoing.end(raw);done();
     })().catch(fail);});
    });
    upstreams.add(upstream);upstream.on('close',()=>upstreams.delete(upstream));
    upstream.on('error',fail);upstream.setTimeout(310000,()=>upstream.destroy(Error('Observer deadline exceeded')));
    outgoing.on('close',()=>{if(!outgoing.writableFinished)upstream.destroy(Error('Observer client cancelled'));});
    upstream.end(body);
   });
  })();
  pending.add(task);task.catch(()=>{if(!outgoing.headersSent)outgoing.writeHead(502);outgoing.end('{}');}).finally(()=>pending.delete(task));
 });
 await new Promise(r=>server.listen(0,'127.0.0.1',r));
 return {origin:'http://127.0.0.1:'+server.address().port,records,
  async close(){server.closeAllConnections();for(const req of upstreams)req.destroy();await new Promise(r=>server.close(r));await Promise.allSettled([...pending]);}};
}
