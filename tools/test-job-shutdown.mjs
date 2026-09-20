// Isolated real-process regression for gateway shutdown vs durable job interruption.
// Uses a held loopback HTTP response, not a shell command or personal installation.
import assert from 'node:assert/strict';
import {mkdtemp,readFile,writeFile} from 'node:fs/promises';
import {resolve,join} from 'node:path';
import {spawn} from 'node:child_process';
import {createServer} from 'node:http';
import {randomBytes} from 'node:crypto';
const root=resolve(import.meta.dirname,'..'),scratch=await mkdtemp(join(root,'.validation/job-shutdown-'));
const bin=resolve(process.env.EEF_TEST_BINARY_DIR||join(root,'target/debug')),children=[];
const env={...process.env,EEF_NODE_PSK:'',APPDATA:join(scratch,'appdata'),EEF_DISCOVERY_DIR:join(scratch,'discovery'),PATH:join(root,'.tooling/llvm-mingw-20260616-ucrt-x86_64/bin')+';'+process.env.PATH};
function launch(name,args,extra={}){const c=spawn(join(bin,name+'.exe'),args,{cwd:root,env:{...env,...extra},windowsHide:true,stdio:['ignore','pipe','pipe']});let out='',err='';c.stdout.on('data',b=>out=(out+b).slice(-1048576));c.stderr.on('data',b=>err=(err+b).slice(-1048576));c.result=new Promise((r,j)=>{c.on('error',j);c.on('close',code=>r({code,out,err}));});children.push(c);return c;}
async function port(){const s=createServer();await new Promise(r=>s.listen(0,'127.0.0.1',r));const p=s.address().port;await new Promise(r=>s.close(r));return p;}
async function response(url,body){const r=await fetch(url,{method:body?'POST':'GET',headers:{'Content-Type':'application/json'},body:body?JSON.stringify(body):undefined,signal:AbortSignal.timeout(5000)});return {status:r.status,data:await r.json()};}
async function json(url,body){const r=await response(url,body);assert.equal(r.status,200,JSON.stringify(r));assert.notEqual(r.data.success,false,JSON.stringify(r));return r.data;}
async function until(fn,label){const end=Date.now()+45000;while(Date.now()<end){try{const v=await fn();if(v)return v;}catch{}await new Promise(r=>setTimeout(r,100));}throw Error('Timeout: '+label);}
let dispatches=0,held;
const fixture=createServer((req,res)=>{dispatches++;if(req.url==='/hold'){held=res;return;}res.end('healthy');});
try {
 const [eefPort,nodePort,gateway]=await Promise.all([port(),port(),port()]);await new Promise(r=>fixture.listen(0,'127.0.0.1',r));
 const fixturePort=fixture.address().port,eef=`http://127.0.0.1:${eefPort}`,node=`http://127.0.0.1:${nodePort}`,id='shutdown-fixture',secret=randomBytes(24).toString('hex');
 const config=JSON.parse(await readFile(join(root,'config/node.example.json'),'utf8'));
 Object.assign(config,{node_id:id,name:'Shutdown fixture',auto_local:false,local_pairing:false,psk:secret,endpoints:[`127.0.0.1:${gateway}`],heartbeat_seconds:0.5,dashboard:{enabled:true,host:'127.0.0.1',port:nodePort},update:{policy:'off'},models:{provider:'llamacpp',llamacpp:{slots:[]}}});
 Object.assign(config.permissions.http,{enabled:true,allowed_hosts:['127.0.0.1'],allow_private_networks:true});
 const nodeConfig=join(scratch,'node.json'),eefConfig=join(scratch,'eef.yaml');await writeFile(nodeConfig,JSON.stringify(config));
 const yaml=(await readFile(join(root,'config/default_identity.yaml'),'utf8')).replace('port: 51334',`port: ${eefPort}`).replace('port: 51335',`port: ${gateway}`).replace('host: 0.0.0.0','host: 127.0.0.1').replace('policy: prompt','policy: off');await writeFile(eefConfig,yaml);
 launch('eef',['--config',eefConfig,'--database',join(scratch,'eef.db'),'--no-brain','--no-ui'],{EEF_NODE_PSK:secret});launch('eefn',['--config',nodeConfig,'--no-ui']);
 await until(async()=>(await json(node+'/api/diagnostics')).connection_state==='connected','fixture registration');
 const before=await json(eef+'/api/diagnostics');
 const job=await json(eef+'/api/jobs',{description:'Held HTTP shutdown fixture',template:'http_request',params:{url:`http://127.0.0.1:${fixturePort}/hold`,method:'GET',timeout_seconds:30},constraints:{node_id:id}});
 await until(()=>held,'remote request reached fixture');assert.equal(dispatches,1);
 await json(eef+'/api/restart',{});
 await until(async()=>{const d=await json(eef+'/api/diagnostics');return d.runtime_id!==before.runtime_id&&d.connected_node_count===1;},'coordinator restart and reconnect');
 const interrupted=await json(eef+'/api/jobs/'+job.id);await writeFile(join(scratch,'job-after-restart.json'),JSON.stringify(interrupted,null,2));
 assert.equal(interrupted.status,'interrupted',JSON.stringify(interrupted));assert.equal(interrupted.tasks[0].status,'interrupted');assert.equal(interrupted.tasks[0].attempts,1);assert.equal(interrupted.can_resume,false);
 const denied=await response(eef+'/api/jobs/'+job.id+'/resume',{});assert.equal(denied.status,400);assert.match(denied.data.error,/cannot safely resume/);assert.equal(dispatches,1);
 held.end('late completion');await new Promise(r=>setTimeout(r,500));assert.deepEqual(await json(eef+'/api/jobs/'+job.id),interrupted,'Late response changed the interrupted checkpoint');
 const healthy=await json(eef+'/api/jobs',{description:'Healthy work after recovery',template:'http_request',params:{url:`http://127.0.0.1:${fixturePort}/healthy`,method:'GET'},constraints:{node_id:id}});
 await until(async()=>(await json(eef+'/api/jobs/'+healthy.id)).status==='completed','healthy job after recovery');assert.equal(dispatches,2);
 const stopped=await json(eef+'/api/jobs/'+job.id+'/stop',{});assert.equal(stopped.status,'cancelled');assert.equal(stopped.tasks[0].attempts,1);
 assert.equal(await readFile(nodeConfig,'utf8'),JSON.stringify(config));assert.equal(await readFile(eefConfig,'utf8'),yaml);
 await writeFile(join(scratch,'results.json'),JSON.stringify({passed:true,real_process_transport:true,physical_two_pc:false,shutdown_checkpoints_before_gateway_close:true,unsafe_resume_refused:true,no_replay:true,late_response_ignored:true,healthy_after_recovery:true,configuration_unchanged:true},null,2));console.log('Job shutdown regression passed: '+scratch);
} finally {
 fixture.closeAllConnections();fixture.close();for(const c of children)if(c.exitCode===null)c.kill();const results=await Promise.allSettled(children.map(c=>c.result));await writeFile(join(scratch,'process-results.json'),JSON.stringify(results,null,2));
}
