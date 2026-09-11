// Two logical nodes on ONE PC: authenticated discovery, not physical mesh proof.
import assert from 'node:assert/strict';
import {mkdtemp,readFile,writeFile} from 'node:fs/promises';
import {resolve,join} from 'node:path';
import {spawn} from 'node:child_process';
import {createServer} from 'node:net';
import {randomBytes} from 'node:crypto';
const root=resolve(import.meta.dirname,'..'),scratch=await mkdtemp(join(root,'.validation','peer-discovery-'));
const bin=resolve(process.env.EEF_TEST_BINARY_DIR||join(root,'target/debug')),children=[];
const env={...process.env,EEF_NODE_PSK:'',APPDATA:join(scratch,'appdata'),EEF_DISCOVERY_DIR:join(scratch,'discovery'),PATH:join(root,'.tooling/llvm-mingw-20260616-ucrt-x86_64/bin')+';'+process.env.PATH};
function launch(name,args,extra={}) {
 const child=spawn(join(bin,name+'.exe'),args,{cwd:root,windowsHide:true,env:{...env,...extra},stdio:['ignore','pipe','pipe']});let out='',err='';
 child.stdout.on('data',b=>out+=b);child.stderr.on('data',b=>err+=b);
 child.result=new Promise((resolve,reject)=>{child.on('error',reject);child.on('close',code=>resolve({code,out,err}));});children.push(child);return child;
}
async function port(){const s=createServer();await new Promise(r=>s.listen(0,'127.0.0.1',r));const p=s.address().port;await new Promise(r=>s.close(r));return p;}
async function until(fn,label){const end=Date.now()+65000;while(Date.now()<end){try{if(await fn())return;}catch{}await new Promise(r=>setTimeout(r,200));}throw Error('Timeout: '+label);}
async function json(url,body,method){const r=await fetch(url,{method:method||(body?'POST':'GET'),headers:{'Content-Type':'application/json'},body:body?JSON.stringify(body):undefined,signal:AbortSignal.timeout(5000)});const v=await r.json();assert(r.ok,JSON.stringify(v));return v;}
async function peers(id,args=[],success=true){const child=launch('eefn',['--config',join(scratch,id+'.json'),'--json','network','peers',...args]);const timer=setTimeout(()=>child.kill(),20000);try {const r=await child.result;assert.equal(r.code,success?0:1,r.err);const v=JSON.parse(r.out);assert.equal(v.success,success);return v;}finally{clearTimeout(timer);}}
try {
 const [nodePort,eefPort,aPort,bPort]=await Promise.all([port(),port(),port(),port()]);
 const eef=`http://127.0.0.1:${eefPort}`,a=`http://127.0.0.1:${aPort}`,b=`http://127.0.0.1:${bPort}`;
 const secret=randomBytes(24).toString('hex');
 for(const [id,p,address,heartbeat] of [['node-a',aPort,'26.1.2.3',1],['node-b',bPort,'26.4.5.6',60]]) {
  const c=JSON.parse(await readFile(join(root,'config/node.example.json'),'utf8'));
  Object.assign(c,{node_id:id,name:id,psk:secret,endpoints:[`127.0.0.1:${nodePort}`],auto_local:false,local_pairing:false,heartbeat_seconds:heartbeat,
   dashboard:{enabled:true,host:'127.0.0.1',port:p},update:{policy:'off'},models:{provider:'llamacpp',llamacpp:{slots:[]}},
   network:{advertised_address:address,coordinator:{coordinator_id:'eef-'+id,address:address+':51335',state:'standby'}}});
  await writeFile(join(scratch,id+'.json'),JSON.stringify(c));
 }
 const yaml=(await readFile(join(root,'config/default_identity.yaml'),'utf8')).replace('port: 51334',`port: ${eefPort}`).replace('port: 51335',`port: ${nodePort}`).replace('policy: prompt','policy: off');
 const config=join(scratch,'eef.yaml');await writeFile(config,yaml);
 launch('eef',['--config',config,'--database',join(scratch,'eef.db'),'--no-brain'],{EEF_NODE_PSK:secret});
 launch('eefn',['--config',join(scratch,'node-a.json'),'--no-ui']);
 const nodeB=launch('eefn',['--config',join(scratch,'node-b.json'),'--no-ui']);
 const connected=async()=> (await json(a+'/api/status')).connection?.state==='connected'&&(await json(b+'/api/status')).connection?.state==='connected';
 await until(connected,'two logical nodes connected');
 assert.equal((await fetch(a+'/')).status,404);
 const self=await peers('node-a');assert.deepEqual(self.nodes.map(n=>n.node_id),['node-a']);assert.equal(self.direct_access_authorized,false);
 const denied=await peers('node-a',['--node','node-b'],false);const absent=await peers('node-a',['--node','missing-node'],false);assert.equal(denied.error,absent.error);
 // Owner-configured disclosure is applied only after an actual EEF restart.
 async function policy(grants,freshness_seconds=30){const saved=(await json(eef+'/api/config')).config;saved.discovery={grants,freshness_seconds};await json(eef+'/api/config',{config:saved},'PUT');}
 async function restart(){const old=(await json(eef+'/api/status')).runtime_id;await json(eef+'/api/restart',{});await until(async()=>(await json(eef+'/api/status')).runtime_id!==old&&await connected(),'verified restart with both nodes');}
 await policy({'node-a':['node-b']});
 await peers('node-a',['--node','node-b'],false);
 await restart();
 const allowed=await peers('node-a',['--node','node-b']);assert.equal(allowed.nodes[0].network.advertised_address,'26.4.5.6');assert.equal(allowed.nodes[0].network.coordinator.state,'standby');
 assert(!JSON.stringify(allowed).includes(secret));assert.equal(allowed.nodes[0].source_ip,undefined);
 await peers('node-b',['--node','node-a'],false); // grants are directional
 const first=await peers('node-a',['--limit','1']);assert.deepEqual(first.nodes.map(n=>n.node_id),['node-a']);assert.equal(first.next_after,'node-a');
 const next=await peers('node-a',['--after',first.next_after,'--limit','1']);assert.deepEqual(next.nodes.map(n=>n.node_id),['node-b']);assert.equal(next.next_after,null);
 const malformed=await fetch(a+'/api/commands/network',{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify({schema_version:1,expected_node_id:'node-a',command:{operation:'peers',query:{node_id:'node-b',origin_node:'forged'}}})});assert.equal(malformed.status,422);
 await policy({'node-a':['node-b']},1);await restart();
 await new Promise(r=>setTimeout(r,1800));
 const stale=await peers('node-a',['--node','node-b']);assert.equal(stale.nodes[0].status,'stale');assert.equal(stale.nodes[0].network.advertised_address,null);assert.equal(stale.nodes[0].network.coordinator,null);assert.equal(stale.nodes[0].remaining_fresh_ms,0);
 await policy({});await restart();
 await peers('node-a',['--node','node-b'],false); // revocation is effective after restart
 const revoked=await peers('node-a',['--after','node-a']);assert.deepEqual(revoked.nodes,[]);
 await policy({'node-a':['node-b']});await restart();
 nodeB.kill();await nodeB.result;
 await until(async()=>(await json(eef+'/api/status')).world.devices.some(n=>n.node_id==='node-b'&&!n.connected),'node B disconnected');
 await peers('node-a',['--node','node-b'],false);
 assert.deepEqual((await peers('node-a')).nodes.map(n=>n.node_id),['node-a']);
 const report={passed:true,logical_nodes:2,physical_two_pc:false,no_ui_or_models:true,self_only_default:true,owner_scoped_directional_grants:true,pagination:true,forged_origin_rejected:true,stale_endpoints_redacted:true,restart_applies_revocation:true,disconnect_removes_advertisement:true};
 await writeFile(join(scratch,'results.json'),JSON.stringify(report,null,2));console.log('Peer discovery validation passed: '+scratch);
}finally{for(const child of children)if(child.exitCode===null)child.kill();await Promise.allSettled(children.map(c=>c.result));}
