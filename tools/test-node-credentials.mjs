// Public-command two-logical-node fixture, on one PC. No models or installed nodes.
import assert from 'node:assert/strict';
import {mkdtemp, readFile, writeFile} from 'node:fs/promises';
import {resolve, join} from 'node:path';
import {spawn} from 'node:child_process';
import {createServer} from 'node:net';
import {randomBytes} from 'node:crypto';
const root = resolve(import.meta.dirname, '..');
const scratch = await mkdtemp(join(root, '.validation/node-credentials-'));
const bin = resolve(process.env.EEF_TEST_BINARY_DIR || join(root, 'target/debug'));
const children = [], eefConfig = join(scratch, 'eef.yaml');
const secret = randomBytes(24).toString('hex');
const env = {...process.env, EEF_NODE_PSK:'', APPDATA:join(scratch,'appdata'),
  EEF_DISCOVERY_DIR:join(scratch,'discovery'), PATH:join(root,'.tooling/llvm-mingw-20260616-ucrt-x86_64/bin')+';'+process.env.PATH};
function launch(name,args,extra={}) {
  const child = spawn(join(bin,name+'.exe'),args,{cwd:root,windowsHide:true,env:{...env,...extra},stdio:['ignore','pipe','pipe']});
  let out='',err='';child.stdout.on('data',b=>out=(out+b).slice(-1048576));child.stderr.on('data',b=>err=(err+b).slice(-1048576));
  child.result=new Promise((resolve,reject)=>{child.on('error',reject);child.on('close',code=>resolve({code,out,err}));});
  children.push(child);return child;
}
async function cli(name,config,args,success=true) {
  const child=launch(name,['--config',config,'--json',...args]);const timer=setTimeout(()=>child.kill(),30000);
  try {const r=await child.result;assert.equal(r.code,success?0:1,r.err);return JSON.parse(r.out);} finally {clearTimeout(timer);}
}
const coordinator=(args,success=true)=>cli('eef',eefConfig,args,success);
async function until(fn,label) {const deadline=Date.now()+45000;while(Date.now()<deadline){try{if(await fn())return;}catch{}await new Promise(r=>setTimeout(r,150));}throw Error('Timeout: '+label);}
async function port(){const s=createServer();await new Promise(r=>s.listen(0,'127.0.0.1',r));const p=s.address().port;await new Promise(r=>s.close(r));return p;}
async function json(url,body,method){const r=await fetch(url,{method:method||(body?'POST':'GET'),headers:{'Content-Type':'application/json'},body:body?JSON.stringify(body):undefined,signal:AbortSignal.timeout(5000)});assert(r.ok,`${url}: HTTP ${r.status}`);return r.json();}
try {
  const [webPort,gatewayPort]=await Promise.all([port(),port()]);const base=`http://127.0.0.1:${webPort}`;
  const yaml=(await readFile(join(root,'config/default_identity.yaml'),'utf8')).replace('port: 51334',`port: ${webPort}`).replace('port: 51335',`port: ${gatewayPort}`).replace('policy: prompt','policy: off');
  await writeFile(eefConfig,yaml);
  launch('eef',['--config',eefConfig,'--database',join(scratch,'eef.db'),'--no-brain','--no-ui'],{EEF_NODE_PSK:secret});
  await until(async()=>(await json(base+'/api/diagnostics')).initialized,'coordinator startup');
  const nodes=[];
  for(const name of ['a','b']) {
    const config=join(scratch,name+'.json'),apiPort=await port();
    const id=(await cli('eefn',config,['network','set','--name','Credential fixture '+name])).node_id;
    await writeFile(config,JSON.stringify({node_id:id,name:'Credential fixture '+name,psk:'',endpoints:[],auto_local:false,
      local_pairing:false,dashboard:{enabled:true,host:'127.0.0.1',port:apiPort},permissions:{remote_updates:false},
      models:{provider:'llamacpp',llamacpp:{slots:[]}},update:{policy:'off'},custom:{preserve:true}}));
    launch('eefn',['--config',config,'--no-ui']);
    const api=`http://127.0.0.1:${apiPort}`;
    await until(async()=>(await json(api+'/api/status')).node_id===id,'standalone node startup');
    const invitation=await coordinator(['credentials','provision','--node',id,'--address',`127.0.0.1:${gatewayPort}`]);
    const privateFile=join(scratch,name+'-PRIVATE.json');
    await writeFile(privateFile,name==='b'?Buffer.concat([Buffer.from([0xff,0xfe]),Buffer.from(JSON.stringify(invitation),'utf16le')]):'\ufeff'+JSON.stringify(invitation));
    assert.equal(invitation.private,true);assert.equal(invitation.restart_required,true);
    const decoded=JSON.parse(Buffer.from(invitation.code,'base64'));assert.equal(decoded.node_id,id);assert.notEqual(decoded.psk,secret);
    nodes.push({id,config,api,privateFile,credential:decoded.psk});
  }
  assert.notEqual(nodes[0].credential,nodes[1].credential);
  const show=await coordinator(['credentials','show']);assert.equal(show.saved_policy.identities.length,2);assert.equal(show.applied_policy.identities.length,0);
  for(const node of nodes) assert(!JSON.stringify(show).includes(node.credential));
  const previous=(await coordinator(['diagnostics'])).runtime_id;
  await coordinator(['restart']);
  await until(async()=>(await coordinator(['diagnostics'])).runtime_id!==previous,'apply provisioned credentials');
  for(const node of nodes) {
    const report=await cli('eefn',node.config,['connection','pair','--code-file',node.privateFile]);
    assert.equal(report.saved_auth_mode,'node_credential');assert.equal(report.restart_requested,true);
    assert(!JSON.stringify(report).includes(node.credential));
    await until(async()=>(await json(node.api+'/api/status')).connection?.state==='connected','credential node connected');
    const config=JSON.parse(await readFile(node.config,'utf8'));
    assert.equal(config.node_id,node.id);assert.equal(config.psk,node.credential);assert.equal(config.auth_mode,'node_credential');
    assert.equal(config.custom.preserve,true);assert.equal(config.permissions.remote_updates,false);assert.equal(config.auto_local,false);
    assert.equal((await cli('eefn',node.config,['connection','show'])).saved_auth_mode,'node_credential');
    await cli('eefn',node.config,['connection','pair-local'],false);
  }
  await until(async()=>(await coordinator(['diagnostics'])).registered_node_count===2,'two registered nodes');
  for(const node of nodes) assert.equal((await coordinator(['diagnostics','--node',node.id,'--samples','1'])).passed,1);
  const configView=await json(base+'/api/config');
  for(const node of nodes) assert(!JSON.stringify(configView).includes(node.credential));
  // A stale generic configuration client cannot drop the authentication policy.
  delete configView.config.node.authentication;
  await json(base+'/api/config',{config:configView.config},'PUT');
  assert.equal((await coordinator(['credentials','show'])).saved_policy.identities.length,2);
  await coordinator(['credentials','legacy','--allow','false']);
  let old=(await coordinator(['diagnostics'])).runtime_id;await coordinator(['restart']);
  await until(async()=>{const d=await coordinator(['diagnostics']);return d.runtime_id!==old&&d.registered_node_count===2;},'independent reconnect with legacy disabled');
  const policy=await coordinator(['credentials','show']);assert.equal(policy.applied_policy.allow_legacy,false);
  await coordinator(['invite','--address',`127.0.0.1:${gatewayPort}`],false);
  await coordinator(['credentials','revoke','--node',nodes[0].id]);
  assert.equal((await coordinator(['diagnostics','--node',nodes[0].id,'--samples','1'])).passed,1,'saved revocation is not immediate');
  old=(await coordinator(['diagnostics'])).runtime_id;await coordinator(['restart']);
  await until(async()=>{const d=await coordinator(['diagnostics']);return d.runtime_id!==old&&d.registered_node_count===1;},'revocation applied');
  const inventory=await coordinator(['models','list']);assert.deepEqual(inventory.nodes.map(n=>n.node_id),[nodes[1].id]);
  assert.equal((await coordinator(['diagnostics','--node',nodes[1].id,'--samples','1'])).passed,1);
  await coordinator(['credentials','provision','--node',nodes[0].id,'--address',`127.0.0.1:${gatewayPort}`],false);
  // Re-provision is explicit rotation, followed by independent restart/pairing.
  const rotated=await coordinator(['credentials','provision','--node',nodes[0].id,'--address',`127.0.0.1:${gatewayPort}`,'--rotate']);
  const decoded=JSON.parse(Buffer.from(rotated.code,'base64'));assert.notEqual(decoded.psk,nodes[0].credential);
  await writeFile(nodes[0].privateFile,JSON.stringify(rotated));
  old=(await coordinator(['diagnostics'])).runtime_id;await coordinator(['restart']);
  await until(async()=>(await coordinator(['diagnostics'])).runtime_id!==old,'rotation apply');
  await cli('eefn',nodes[0].config,['connection','pair','--code-file',nodes[0].privateFile]);
  await until(async()=>(await coordinator(['diagnostics'])).registered_node_count===2,'rotated node reconnect');
  const results={passed:true,command_pairing:true,two_independent_credentials:true,legacy_disable:true,revocation_and_rotation:true,
    redacted_inspection:true,generic_save_preserves_policy:true,node_id_and_permissions_preserved:true,physical_two_pc:false,
    execution_grants:false,session_replay_fencing:false};
  await writeFile(join(scratch,'results.json'),JSON.stringify(results,null,2));console.log('Node credential validation passed: '+scratch);
} finally {
  for(const child of children)if(child.exitCode===null)child.kill();
  await Promise.allSettled(children.map(c=>c.result));
}
