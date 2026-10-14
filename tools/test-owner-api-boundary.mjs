// Read-only real-socket check of owner HTTP APIs, not the node gateway.
import assert from 'node:assert/strict';
import {mkdtemp,writeFile,readFile} from 'node:fs/promises';
import {resolve,join} from 'node:path';
import {spawn} from 'node:child_process';
import {createServer,createConnection} from 'node:net';
import {networkInterfaces} from 'node:os';
import {randomUUID} from 'node:crypto';
const root=resolve(import.meta.dirname,'..'),bin=resolve(process.env.EEF_TEST_BINARY_DIR||join(root,'target/debug'));
const scratch=await mkdtemp(join(root,'.validation/owner-boundary-')),children=[];
const env={...process.env,EEF_NODE_PSK:'',EEF_DISCOVERY_DIR:join(scratch,'discovery'),APPDATA:join(scratch,'appdata'),
 PATH:join(root,'.tooling/llvm-mingw-20260616-ucrt-x86_64/bin')+';'+process.env.PATH};
const candidates=Object.values(networkInterfaces()).flat().filter(v=>v&&v.family==='IPv4'&&!v.internal&&!v.address.startsWith('169.254.'));
// Prefer an owned LAN address. No remote machine or external target is contacted.
candidates.sort((a,b)=>Number(!/^(192\.168\.|10\.|172\.(1[6-9]|2\d|3[01])\.)/.test(a.address))-Number(!/^(192\.168\.|10\.|172\.(1[6-9]|2\d|3[01])\.)/.test(b.address)));
const nonLoopback=candidates[0]?.address;
async function port(){const s=createServer();await new Promise(r=>s.listen(0,'127.0.0.1',r));const p=s.address().port;await new Promise(r=>s.close(r));return p;}
function start(name,args){const child=spawn(join(bin,name+'.exe'),args,{cwd:root,env,windowsHide:true,stdio:['ignore','pipe','pipe']});
 let out='',err='';child.stdout.on('data',b=>out=(out+b).slice(-16384));child.stderr.on('data',b=>err=(err+b).slice(-16384));
 child.result=new Promise((r,j)=>{child.on('error',j);child.on('close',code=>r({code,out,err}));});children.push(child);return child;}
async function until(fn){const deadline=Date.now()+30000;while(Date.now()<deadline){try{if(await fn())return;}catch{}await new Promise(r=>setTimeout(r,100));}throw Error('owner API startup timeout');}
function request(port,path,remote=false,headers={}){return new Promise((resolve,reject)=>{
 const socket=createConnection({host:remote?nonLoopback:'127.0.0.1',port,...(remote?{localAddress:nonLoopback}:{})});let raw='';
 socket.setTimeout(5000,()=>socket.destroy(new Error('read-only socket timeout')));
 socket.on('error',reject);socket.on('data',data=>raw=(raw+data).slice(0,65536));
 socket.on('connect',()=>socket.write(`GET ${path} HTTP/1.1\r\nHost: 127.0.0.1:${port}\r\nConnection: close\r\n${Object.entries(headers).map(([key,value])=>`${key}: ${value}\r\n`).join('')}\r\n`));
 socket.on('end',()=>{const status=Number(/^HTTP\/1\.1 (\d+)/.exec(raw)?.[1]);if(!status)return reject(new Error('invalid HTTP reply'));resolve(status);});
});}
const observations=[];
try{
 assert(nonLoopback,'requires an owned non-loopback IPv4 address; no boundary result was certified');
 const [eefPort,nodePort,gateway]=await Promise.all([port(),port(),port()]);
 const eefConfig=join(scratch,'eef.yaml'),nodeConfig=join(scratch,'node.json');
 await writeFile(eefConfig,JSON.stringify({web:{host:'0.0.0.0',port:eefPort},node:{host:'127.0.0.1',port:gateway,psk:'',authentication:{allow_legacy:false}},update:{policy:'off'}}));
 await writeFile(nodeConfig,JSON.stringify({node_id:'device-'+randomUUID(),name:'Owner boundary fixture',auto_local:false,connection_enabled:false,
  endpoints:[],psk:'',dashboard:{host:'0.0.0.0',port:nodePort,enabled:true},models:{provider:'llamacpp',llamacpp:{slots:[]}},update:{policy:'off'}}));
 start('eef',['--config',eefConfig,'--database',join(scratch,'eef.db'),'--no-brain','--no-ui']);
 start('eefn',['--config',nodeConfig,'--no-ui']);
 for(const p of [eefPort,nodePort])await until(async()=>(await request(p,'/api/diagnostics'))===200);
 const before=[await readFile(eefConfig),await readFile(nodeConfig)];
 for(const [role,p,path] of [['eef',eefPort,'/api/commands/credentials'],['eefn',nodePort,'/api/diagnostics']]){
  assert.equal(await request(p,path),200,'loopback owner access must work');
  for(const headers of [{},{'X-Forwarded-For':'127.0.0.1','X-Real-IP':'127.0.0.1',Forwarded:'for=127.0.0.1;host=localhost'}]){
   const status=await request(p,path,true,headers);observations.push({role,non_loopback_peer:true,forged_loopback_host:true,forwarded_headers:Object.keys(headers).length>0,status});
   await writeFile(join(scratch,'observations.json'),JSON.stringify(observations,null,2));
  }
  assert.equal(await request(p,path,false,{Origin:'https://untrusted.example'}),403);
  assert.equal(await request(p,path,false,{'Sec-Fetch-Site':'cross-site'}),403);
 }
 assert((await readFile(eefConfig)).equals(before[0]));assert((await readFile(nodeConfig)).equals(before[1]));
 assert(observations.every(value=>value.status===403),'non-loopback socket peers must not reach either owner API using forged local headers');
 await writeFile(join(scratch,'results.json'),JSON.stringify({passed:true,real_non_loopback_socket_on_same_pc:true,physical_second_pc:false,
  eef_and_eefn_wildcard_bindings:true,forged_host_and_forwarding_headers_rejected:true,loopback_still_works:true,
  cross_origin_still_rejected:true,read_only_no_config_changes:true,os_user_authentication:false,node_gateway_not_tested:true},null,2));
 console.log('Owner API boundary validation passed: '+scratch);
}catch(error){await writeFile(join(scratch,'failure.json'),JSON.stringify({passed:false,observations},null,2));throw error;}
finally{for(const child of children)if(child.exitCode===null)child.kill();await Promise.allSettled(children.map(child=>child.result));}
