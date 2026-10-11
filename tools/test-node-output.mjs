// Actual API-only node process against a test-only encrypted loopback peer.
// This exercises existing legacy framing, not pinned TLS or workload admission.
import assert from 'node:assert/strict';
import {mkdtemp, readFile, writeFile} from 'node:fs/promises';
import {resolve, join} from 'node:path';
import {spawn} from 'node:child_process';
import {createServer} from 'node:net';
import {createServer as httpServer} from 'node:http';
import {randomBytes, hkdfSync, createCipheriv, createDecipheriv, createHmac} from 'node:crypto';

const root=resolve(import.meta.dirname,'..'), bin=resolve(process.env.EEF_TEST_BINARY_DIR||join(root,'target/debug'));
const scratch=await mkdtemp(join(root,'.validation','node-output-'));
const configPath=join(scratch,'node.json'), children=[], sockets=new Set(), sessions=[];
const secret=randomBytes(24).toString('hex'), id='output-fixture-node';
const key=Buffer.from(hkdfSync('sha256',Buffer.from(secret),Buffer.from('eef-node-salt'),Buffer.from('eef-node-v1'),32));
const env={...process.env,EEF_NODE_PSK:'',APPDATA:join(scratch,'appdata'),EEF_DISCOVERY_DIR:join(scratch,'discovery'),
  PATH:join(root,'.tooling/llvm-mingw-20260616-ucrt-x86_64/bin')+';'+process.env.PATH};
let fault, blocked, blockedAt, runtime;
function encode(value){
  const nonce=randomBytes(12), cipher=createCipheriv('aes-256-gcm',key,nonce);
  const payload=Buffer.concat([cipher.update(JSON.stringify(value)),cipher.final()]);
  return Buffer.concat([nonce,payload,cipher.getAuthTag()]).toString('base64')+'\n';
}
function decode(line){
  const bytes=Buffer.from(line,'base64'), decipher=createDecipheriv('aes-256-gcm',key,bytes.subarray(0,12));
  decipher.setAuthTag(bytes.subarray(-16));
  return JSON.parse(Buffer.concat([decipher.update(bytes.subarray(12,-16)),decipher.final()]));
}
function launch(args){
  const child=spawn(join(bin,'eefn.exe'),['--config',configPath,...args],{cwd:root,windowsHide:true,env,stdio:['ignore','pipe','pipe']});
  let out='',err='';child.stdout.on('data',b=>out=(out+b).slice(-1048576));child.stderr.on('data',b=>err=(err+b).slice(-1048576));
  child.result=new Promise((r,j)=>{child.on('error',j);child.on('close',code=>r({code,out,err}));});children.push(child);return child;
}
async function cli(args){
  const child=launch(['--json',...args]), timer=setTimeout(()=>child.kill(),20000);
  try{const result=await child.result;assert.equal(result.code,0,'node CLI failed (private output withheld)');return JSON.parse(result.out);}
  finally{clearTimeout(timer);}
}
async function until(fn,label,ms=20000){
  const end=Date.now()+ms;while(Date.now()<end){if(fault)throw fault;try{if(await fn())return;}catch(error){if(fault)throw fault;}
    await new Promise(r=>setTimeout(r,100));}throw Error('Timeout: '+label);
}
async function port(){const s=createServer();await new Promise(r=>s.listen(0,'127.0.0.1',r));const p=s.address().port;await new Promise(r=>s.close(r));return p;}
const tags=httpServer((req,res)=>{res.setHeader('Content-Type','application/json');res.end('{"models":[]}');});
const peer=createServer(socket=>{
  sockets.add(socket);socket.on('close',()=>sockets.delete(socket));socket.on('error',()=>{});
  const session={messages:[],registered:false}, first=sessions.length===0; sessions.push(session);
  let pending='';
  socket.on('data',bytes=>{
    if(first&&session.registered)return; // Deliberately never drain blocked output.
    try{
      pending+=bytes.toString('ascii');assert(pending.length<8*1024*1024);
      while(pending.includes('\n')){
        const cut=pending.indexOf('\n'), value=decode(pending.slice(0,cut));pending=pending.slice(cut+1);
        session.messages.push(value.type);
        if(value.type==='probe')socket.write(encode({type:'pong',nonce:value.nonce,protocol:1}));
        else if(value.type==='auth'){
          assert.equal(value.node_id,id);
          assert.equal(value.signature,createHmac('sha256',key).update(`${value.nonce}|${value.node_id}|${value.version}|${value.timestamp}`).digest('base64'));
          socket.write(encode({type:'ok'}));
        }else if(value.type==='register'){
          assert.equal(value.node_id,id);session.registered=true;
          if(first){
            blocked=socket;socket.pause();blockedAt=Date.now();
            socket.write(encode({type:'request',id:'saved-before-loss',capability:'node.configure',action:'model_command',
              params:{schema_version:1,expected_node_id:id,command:{operation:'cpu_threads',model_id:'cpu-fixture',threads:2}}}));
            for(let i=0;i<64;i++)socket.write(encode({type:'request',id:`inspection-${i}`,capability:'node.configure',action:'model_command',
              params:{schema_version:1,expected_node_id:id,command:{operation:'show'}}}));
            return;
          }
        }else assert.equal(value.type,'heartbeat','reconnection must not replay requests or responses');
      }
    }catch(error){fault=error;socket.destroy();}
  });
});
try{
  await new Promise(r=>tags.listen(0,'127.0.0.1',r));await new Promise(r=>peer.listen(0,'127.0.0.1',r));
  const apiPort=await port(), config=JSON.parse(await readFile(join(root,'config/node.example.json'),'utf8'));
  const roles=Array.from({length:32},(_,i)=>`r${String(i).padStart(2,'0')}`+'x'.repeat(60));
  // Inactive selections still need valid existing paths and distinct saved ports.
  // This tiny fixture is never opened by a model backend (provider is Ollama).
  const inactiveFile=join(scratch,'inactive-fixture.gguf');await writeFile(inactiveFile,'test-only; not model weights');
  const slots=Array.from({length:64},(_,i)=>({model_id:i===0?'cpu-fixture':`m${String(i).padStart(2,'0')}`+'x'.repeat(250),
    model_path:inactiveFile,port:20000+i,selection:{roles}}));
  Object.assign(config,{node_id:id,name:'Output fixture',auto_local:false,local_pairing:false,psk:secret,
    endpoints:[`127.0.0.1:${peer.address().port}`],heartbeat_seconds:0.5,request_limits:{work:128,control:2},
    dashboard:{enabled:true,ui_enabled:false,host:'127.0.0.1',port:apiPort},update:{policy:'off'},management:{allow_remote:true},
    models:{provider:'ollama',ollama:{base_url:`http://127.0.0.1:${tags.address().port}`,selected:[]},llamacpp:{slots}}});
  await writeFile(configPath,JSON.stringify(config));launch(['--no-ui']);
  await until(async()=>{const report=await cli(['network','diagnose']);runtime=report.runtime_id;return blocked&&runtime;},'initial connection and runtime');
  await until(async()=>JSON.parse(await readFile(configPath,'utf8')).models.llamacpp.slots[0].cpu_threads===2,'owner-approved save before output loss');
  const saved=await readFile(configPath,'utf8');
  await until(async()=>{const report=await cli(['network','diagnose']);return report.successful_connections>=2&&report.connection_state==='connected';},'node retires blocked output and reconnects',20000);
  const reconnectMs=Date.now()-blockedAt;
  assert(reconnectMs<20000);assert(sessions.length>=2);
  assert.equal((await cli(['network','diagnose'])).runtime_id,runtime,'connection retirement must not restart the node runtime');
  const models=await cli(['models','show']);assert.equal(models.running,true);
  assert.equal(models.saved_selections.find(m=>m.model_id==='cpu-fixture').cpu_threads,2);
  assert.equal(await readFile(configPath,'utf8'),saved,'reconnection must preserve saved settings');
  // Read-only polling observes a healthy reconnected session; no new mutations.
  await until(async()=>sessions.slice(1).some(s=>s.messages.includes('heartbeat')),'heartbeat after reconnection');
  for(const session of sessions.slice(1))assert(session.messages.every(t=>['probe','auth','register','heartbeat'].includes(t)));
  assert.equal((await cli(['network','diagnose'])).busy_rejections,0,'test must exercise ordinary output, not busy rejection');
  assert.equal((await fetch(`http://127.0.0.1:${apiPort}/`)).status,404);
  if(fault)throw fault;
  await writeFile(join(scratch,'results.json'),JSON.stringify({passed:true,actual_node_process:true,local_commands_after_reconnect:true,
    saved_change_survives:true,reconnect_ms:reconnectMs,no_response_or_work_replay:true,legacy_encrypted_loopback:true,
    pinned_tls:false,physical_two_pc:false,native_termination_proven:false},null,2));
  console.log('Node output validation passed: '+scratch);
}finally{
  for(const child of children)if(child.exitCode===null)child.kill();await Promise.allSettled(children.map(c=>c.result));
  for(const socket of sockets)socket.destroy();await Promise.all([new Promise(r=>peer.close(r)),new Promise(r=>tags.close(r))]);
}
