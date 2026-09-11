// Command-driven one-PC integration. No browser, AI model or personal config.
import assert from 'node:assert/strict';
import {mkdtemp,readFile,writeFile} from 'node:fs/promises';
import {resolve,join} from 'node:path';
import {spawn} from 'node:child_process';
import {createServer} from 'node:net';
import {randomBytes} from 'node:crypto';

const root=resolve(import.meta.dirname,'..');
const scratch=await mkdtemp(join(root,'.validation','node-commands-'));
const bin=resolve(process.env.EEF_TEST_BINARY_DIR||join(root,'target/debug'));
const config=join(scratch,'node.json'), children=[];
const env={...process.env,EEF_NODE_PSK:'',APPDATA:join(scratch,'appdata'),EEF_DISCOVERY_DIR:join(scratch,'discovery'),PATH:join(root,'.tooling/llvm-mingw-20260616-ucrt-x86_64/bin')+';'+process.env.PATH};
function launch(name,args,extra={}) {
  const child=spawn(join(bin,name+'.exe'),args,{cwd:root,windowsHide:true,env:{...env,...extra},stdio:['ignore','pipe','pipe']});
  let out='',err='';child.stdout.on('data',b=>out+=b);child.stderr.on('data',b=>err+=b);
  child.result=new Promise((resolve,reject)=>{child.on('error',reject);child.on('close',code=>resolve({code,out,err}));});
  children.push(child);return child;
}
async function command(args,success=true) {
  const child=launch('eefn',['--config',config,'--json','network',...args]);
  const timer=setTimeout(()=>child.kill(),25000);
  try {const r=await child.result;assert.equal(r.code,success?0:1,r.err);const data=JSON.parse(r.out);assert.equal(data.success,success);return data;} finally {clearTimeout(timer);}
}
async function port(){const s=createServer();await new Promise(r=>s.listen(0,'127.0.0.1',r));const p=s.address().port;await new Promise(r=>s.close(r));return p;}
async function until(fn,label){const end=Date.now()+65000;while(Date.now()<end){try{if(await fn())return;}catch{}await new Promise(r=>setTimeout(r,200));}throw Error('Timeout: '+label);}
async function json(url){const r=await fetch(url,{signal:AbortSignal.timeout(2000)});assert(r.ok);return r.json();}
try {
  const first=await command(['set','--name','Command node','--advertise-address','26.1.2.3']);
  const id=first.node_id;assert.match(id,/^device-/);assert.equal(first.running,false);
  assert.equal(first.network.advertised_address,'26.1.2.3');
  assert.equal((await command(['show'])).node_id,id);
  const offlineReport=await command(['diagnose']);assert.equal(offlineReport.connection_state,'stopped');assert.equal(offlineReport.metrics_available,false);assert.equal(offlineReport.service_uptime_ms,null);
  const before=await readFile(config,'utf8');
  await command(['set','--name','Must not save','--advertise-address','http://bad/path'],false);
  assert.equal(await readFile(config,'utf8'),before);
  const [apiPort,nodePort,eefPort]=await Promise.all([port(),port(),port()]);
  const api=`http://127.0.0.1:${apiPort}`,eef=`http://127.0.0.1:${eefPort}`;
  const secret=randomBytes(24).toString('hex');
  const saved=JSON.parse(before);
  Object.assign(saved,{auto_local:false,local_pairing:false,endpoints:[`127.0.0.1:${nodePort}`],psk:secret,dashboard:{enabled:true,host:'127.0.0.1',port:apiPort},update:{policy:'off'},models:{provider:'llamacpp',llamacpp:{slots:[]}}});
  await writeFile(config,JSON.stringify(saved));
  const yaml=(await readFile(join(root,'config/default_identity.yaml'),'utf8')).replace('port: 51334',`port: ${eefPort}`).replace('port: 51335',`port: ${nodePort}`).replace('policy: prompt','policy: off');
  const eefConfig=join(scratch,'eef.yaml');await writeFile(eefConfig,yaml);
  launch('eef',['--config',eefConfig,'--database',join(scratch,'eef.db'),'--no-brain'],{EEF_NODE_PSK:secret});
  launch('eefn',['--config',config,'--no-ui']);
  await until(async()=>(await json(api+'/api/status')).connection?.state==='connected','headless node connected');
  assert.equal((await fetch(api+'/')).status,404);
  assert.equal((await fetch(api+'/app.js')).status,404);
  const online=await command(['show']);assert.equal(online.running,true);assert.equal(online.node_id,id);
  const nodeReport=await command(['diagnose']);assert.equal(nodeReport.report_type,'node_diagnostics');assert.equal(nodeReport.connection_state,'connected');assert(!JSON.stringify(nodeReport).includes(secret));assert(!JSON.stringify(nodeReport).includes('26.1.2.3'));
  assert.equal(nodeReport.node_id,id);assert(nodeReport.successful_connections>=1);
  const coordinatorCommand=async(args,expected=0)=>{const p=launch('eef',['--config',eefConfig,'--json',...args]);const t=setTimeout(()=>p.kill(),30000);try{const r=await p.result;assert.equal(r.code,expected,r.err);return JSON.parse(r.out);}finally{clearTimeout(t);}};
  const coreReport=await coordinatorCommand(['diagnostics']);assert.equal(coreReport.report_type,'coordinator_diagnostics');assert.equal(coreReport.connected_node_count,1);assert(!JSON.stringify(coreReport).includes(secret));
  const inventory=await coordinatorCommand(['models','list']);assert.equal(inventory.scope,'connected_node_registrations');assert.equal(inventory.nodes.length,1);assert.equal(inventory.nodes[0].node_id,id);assert.deepEqual(inventory.nodes[0].models,[]);assert(!JSON.stringify(inventory).includes(secret));assert(!JSON.stringify(inventory).includes('26.1.2.3'));
  assert.equal((await coordinatorCommand(['models','list','--node',id])).nodes.length,1);
  await coordinatorCommand(['models','list','--node','missing-node'],1);
  await coordinatorCommand(['models','list','--limit','33'],1);
  await coordinatorCommand(['models','list','--node',id,'--after',id],1);
  const probe=await coordinatorCommand(['diagnostics','--node',id,'--samples','3']);assert.equal(probe.passed,3);assert.equal(probe.failed,0);assert(probe.samples.every(s=>s.matches_coordinator_version&&s.round_trip_ms>=0));
  await coordinatorCommand(['diagnostics','--node',id,'--samples','11'],1);
  await coordinatorCommand(['diagnostics','--node','missing-node'],1);
  const invite=await coordinatorCommand(['invite','--address','192.0.2.10:51335']);const decoded=JSON.parse(Buffer.from(invite.code,'base64').toString('utf8'));assert.equal(decoded.address,'192.0.2.10:51335');assert.equal(decoded.psk,secret);
  await coordinatorCommand(['invite','--address','http://bad/path'],1);
  assert.equal(online.applied_network.advertised_address,'26.1.2.3');
  assert(!JSON.stringify(online).includes(secret));
  const next=await command(['set','--name','Renamed node','--advertise-address','26.4.5.6','--coordinator-id','eef-local','--coordinator-address',`26.4.5.6:${nodePort}`,'--coordinator-state','standby']);
  assert.equal(next.restart_required,true);assert.equal(next.applied_network.advertised_address,'26.1.2.3');
  assert.equal(next.coordinator_endpoints[0],`127.0.0.1:${nodePort}`);
  let r=await fetch(api+'/api/commands/network',{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify({schema_version:1,expected_node_id:'wrong',command:{operation:'show'}})});assert.equal(r.status,400);
  r=await fetch(api+'/api/commands/network',{method:'POST',headers:{'Content-Type':'application/json',Origin:'https://untrusted.example'},body:JSON.stringify({schema_version:1,expected_node_id:id,command:{operation:'show'}})});assert.equal(r.status,403);
  assert((await fetch(api+'/api/restart',{method:'POST'})).ok);
  await until(async()=>{const nodes=(await json(eef+'/api/status')).world.devices;return nodes.some(n=>n.node_id===id&&n.name==='Renamed node'&&n.network?.advertised_address==='26.4.5.6'&&n.network?.coordinator?.state==='standby');},'new advertisement reaches EEF');
  const after=JSON.parse(await readFile(config,'utf8'));assert.equal(after.psk,secret);assert.deepEqual(after.permissions,saved.permissions);assert.equal(after.node_id,id);
  await command(['set','--clear-address','--clear-coordinator']);
  const cleared=await command(['show']);assert.equal(cleared.network.advertised_address,null);assert.equal(cleared.network.coordinator,null);
  const results={passed:true,offline_commands_without_models:true,headless_running_commands:true,stable_identity:true,distinct_connection_and_advertisement:true,coordinator_metadata_reaches_eef:true,permissions_preserved:true,malformed_address_and_wrong_target_and_origin_rejected:true,physical_two_pc:false};
  await writeFile(join(scratch,'results.json'),JSON.stringify(results,null,2));
  console.log('Node command validation passed: '+scratch);
} finally {
  for(const child of children)if(child.exitCode===null)child.kill();
  await Promise.allSettled(children.map(c=>c.result));
}
