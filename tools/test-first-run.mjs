// Integration + browser regression. Fixtures stay in .validation, never personal config.
import {readLoopbackCoordinatorTemplate} from './local-test-network.mjs';
import assert from 'node:assert/strict';
import {mkdtemp,mkdir,readFile,writeFile} from 'node:fs/promises';
import {resolve,join} from 'node:path';
import {spawn} from 'node:child_process';
import {createServer} from 'node:net';
import {request as httpRequest,createServer as httpServer} from 'node:http';
import {randomBytes} from 'node:crypto';
import {chromium} from '../.tooling/ui-tests/node_modules/playwright/index.mjs';
import {reviewedFixture} from './reviewed-fixture.mjs';
const root=resolve(import.meta.dirname,'..'),scratch=await mkdtemp(join(root,'.validation','first-run-'));
const binaryDirectory=resolve(process.env.EEF_TEST_BINARY_DIR||join(root,'target/debug'));
const children=[],errors=[];let browser,modelServer;
const sleep=ms=>new Promise(r=>setTimeout(r,ms));
async function port(){const server=createServer();await new Promise(r=>server.listen(0,'127.0.0.1',r));const p=server.address().port;await new Promise(r=>server.close(r));return p;}
const webPort=await port(),devicePort=await port(),nodePort=await port();
const eef=`http://127.0.0.1:${webPort}`,node=`http://127.0.0.1:${devicePort}`;
async function json(url,body,method){const r=await fetch(url,{method:method||(body?'POST':'GET'),headers:{'Content-Type':'application/json'},body:body?JSON.stringify(body):undefined});const data=await r.json();assert(r.ok,JSON.stringify(data));return data;}
async function until(fn,label,timeout=60000){const end=Date.now()+timeout;while(Date.now()<end){try{const v=await fn();if(v)return v;}catch{}await sleep(250);}throw Error('Timed out: '+label);}
function launch(name,args,extra={}){const child=spawn(join(binaryDirectory,name+'.exe'),args,{cwd:root,windowsHide:true,env:{...process.env,PATH:join(root,'.tooling/llvm-mingw-20260616-ucrt-x86_64/bin')+';'+process.env.PATH,APPDATA:join(scratch,'appdata'),EEF_DISCOVERY_DIR:join(scratch,'discovery'),EEF_NODE_PSK:'',...extra},stdio:['ignore','pipe','pipe']});let out='';child.stdout.on('data',b=>out=(out+b).slice(-1048576));child.stderr.on('data',b=>errors.push(name+': '+b.toString()));child.result=new Promise((resolve,reject)=>{child.on('error',reject);child.on('close',code=>resolve({code,out}));});children.push(child);return child;}
async function cli(name,path,args,success=true){const child=launch(name,['--config',path,'--json',...args]),timer=setTimeout(()=>child.kill(),30000);try{const r=await child.result;assert.equal(r.code,success?0:1,r.out);return JSON.parse(r.out);}finally{clearTimeout(timer);}}
const configPath=join(scratch,'node.json');
try{
 let installed=false,stallPull=false,pulls=0,interpretations=0;
 modelServer=httpServer(async(req,res)=>{
  if(req.url==='/api/tags'){res.setHeader('Content-Type','application/json');res.end(JSON.stringify({models:installed?[{name:'test-model:tiny',size:1024}]:[]}));return;}
  if(req.url==='/api/show'){res.setHeader('Content-Type','application/json');res.end(JSON.stringify({capabilities:['completion']}));return;}
  if(req.url==='/api/pull'){pulls++;res.setHeader('Content-Type','application/x-ndjson');res.write(JSON.stringify({status:'pulling',completed:128,total:1024})+'\n');if(stallPull)return;await sleep(300);installed=true;res.end(JSON.stringify({status:'success',completed:1024,total:1024})+'\n');return;}
  if(req.url==='/api/chat'){
   let body='';for await(const chunk of req){body+=chunk;assert(body.length<=65536);}
   const payload=JSON.parse(body);assert.equal(payload.model,'test-model:tiny');interpretations++;
   const proposal=payload.messages[0].content.startsWith('Propose one method')
    ? {schema_version:1,method:{schema_version:1,template:'system_info',params:{},constraints:{}},clarification:null}
    : {schema_version:1,intent:'information',goal:'Inspect node status',context_hints:[],constraints:[],suggested_capabilities:['system.info'],complexity:'simple',workload:'one_shot',needs_clarification:false,clarification:null};
   res.setHeader('Content-Type','application/json');return res.end(JSON.stringify({message:{content:JSON.stringify(proposal)},done:true,done_reason:'stop'}));
  }
  res.writeHead(404);res.end();
 });await new Promise(r=>modelServer.listen(0,'127.0.0.1',r));
 const config=JSON.parse(await readFile(join(root,'config/node.example.json'),'utf8'));config.dashboard.port=devicePort;config.update.policy='off';
 config.metadata={area:['Home','Office'],resources:[{id:'fixture-file',name:'Test file',capability:'filesystem',parameters:{path:join(scratch,'Documents','fixture.txt')}}]};
 config.models.ollama.base_url=`http://127.0.0.1:${modelServer.address().port}`;config.model_catalog=[{id:'test-model',name:'Test model',ollama:'test-model:tiny',bytes:1024,ram_recommended_gb:1}];
 await writeFile(configPath,JSON.stringify(config));await mkdir(join(scratch,'Documents'));
 await writeFile(join(scratch,'Documents','fixture.txt'),'Resource binding fixture');
 launch('eefn',['--config',configPath]);
 const waiting=await until(async()=>{const s=await json(node+'/api/status');return s.connection?.state==='waiting'&&s;},'device starts without eef');
 const id=waiting.node_id;assert.match(id,/^device-/);assert.notEqual(waiting.name,'Example node');
 assert.equal(pulls,0);assert.equal(waiting.models.length,0);assert.equal((await json(node+'/api/models')).backend,'ollama');
 const duplicate=launch('eefn',['--config',configPath]);await until(()=>duplicate.exitCode===0,'second launch reuses running device');
 const yaml=(await readLoopbackCoordinatorTemplate()).replace('port: 51334',`port: ${webPort}`).replace('port: 51335',`port: ${nodePort}`).replace('policy: prompt','policy: off');
 const eefConfig=join(scratch,'eef.yaml');await writeFile(eefConfig,yaml);
 launch('eef',['--config',eefConfig,'--database',join(scratch,'eef.db'),'--no-brain'],{EEF_NODE_PSK:randomBytes(32).toString('hex')});
 await until(async()=>{const s=await json(node+'/api/status');return s.connection?.state==='connected';},'automatic local pairing');
 await until(async()=>{const s=await json(eef+'/api/status');return s.world?.devices?.some(n=>n.node_id===id&&n.connected);},'normal protocol registration');
 // Automatic local registration is separately tested above. Grants require
 // explicit owner-provisioned TLS pairing, not authority inherited from discovery.
 const invitation=await cli('eef',eefConfig,['credentials','provision','--node',id,'--address',`127.0.0.1:${nodePort}`]);
 const privateFile=join(scratch,'PRIVATE-pair.json');await writeFile(privateFile,JSON.stringify(invitation));
 await cli('eef',eefConfig,['credentials','legacy','--allow','false']);
 const initialRuntime=(await json(eef+'/api/diagnostics')).runtime_id;
 await cli('eef',eefConfig,['restart']);
 await until(async()=>(await json(eef+'/api/diagnostics')).runtime_id!==initialRuntime,'apply private TLS pairing');
 await cli('eefn',configPath,['connection','pair','--code-file',privateFile]);
 await until(async()=>{const s=await json(node+'/api/status');return s.execution_grants.available&&s.connection.state==='connected'&&(await json(eef+'/api/status')).world.devices.some(n=>n.node_id===id&&n.connected);},'pinned grant registration');
 const reviewed=reviewedFixture(eef,node,id);
 const advertised=(await json(eef+'/api/status')).world.devices.find(n=>n.node_id===id).metadata;
 assert.deepEqual(advertised.area,['Home','Office']);
 assert.equal(advertised.resources[0].available,false);
 assert.equal(advertised.resources[0].parameters,undefined);
 let local=(await json(node+'/api/config')).config;assert.equal(local.psk,'__KEEP_EXISTING_SECRET__');
 local.name='Studio PC';local.permissions.filesystem.read=true;local.permissions.filesystem.roots=[join(scratch,'Documents')];
 await json(node+'/api/config',{config:local},'PUT');assert.equal((await json(node+'/api/status')).pending_restart,true);await json(node+'/api/restart',{});
 await until(async()=>{const s=await json(eef+'/api/status');return s.world?.devices?.some(n=>n.node_id===id&&n.name==='Studio PC'&&n.connected);},'restart applies rename and preserves ID');
 const manage=(action,params={})=>json(eef+`/api/devices/${id}/manage`,{action,params});
 const remote=(await manage('get')).config;remote.name='Proposed device name';remote.management={allow_remote:true};
 assert.equal((await manage('save',{config:remote})).approval_required,true);assert.equal((await json(node+'/api/config')).config.name,'Studio PC');assert.equal((await json(node+'/api/status')).proposal_pending,true);
 local=(await json(node+'/api/config')).config;local.management={allow_remote:true};await json(node+'/api/config',{config:local},'PUT');
 assert.equal((await manage('save',{config:{name:'Remote renamed PC',node_id:'invalid',management:{allow_remote:false}}})).saved,true);
 assert.equal((await json(node+'/api/config')).config.node_id,id);assert.equal((await json(node+'/api/config')).config.management.allow_remote,true);
 await manage('restart');await until(async()=>{const s=await json(eef+'/api/status');return s.world?.devices?.some(n=>n.node_id===id&&n.name==='Remote renamed PC'&&n.connected);},'remote restart');
 await fetch(node+'/api/proposal',{method:'DELETE'});
 assert.equal((await fetch(node+'/api/restart',{method:'POST',headers:{Origin:'https://untrusted.example'}})).status,403);
 const rebindingStatus=await new Promise((resolve,reject)=>{const r=httpRequest(node+'/api/status',{headers:{Host:'untrusted.example'}},response=>{response.resume();resolve(response.statusCode);});r.on('error',reject);r.end();});assert.equal(rebindingStatus,403);
 browser=await chromium.launch({executablePath:process.env.EEF_TEST_BROWSER||'C:\\Program Files\\Google\\Chrome\\Application\\chrome.exe',headless:true});
 const context=await browser.newContext({viewport:{width:1280,height:900}});const page=await context.newPage();page.on('pageerror',e=>{throw e;});
 await page.goto(node);await page.waitForFunction(()=>document.getElementById('health')?.textContent==='Connected');assert.equal(await page.locator('#rawConfig').isVisible(),false);
 await page.getByRole('link',{name:'Permissions',exact:true}).click();
 assert.equal(await page.locator('[data-field="permissions.http.allowed_hosts"]').count(),0);
 await page.locator('[data-scope="filesystem.read"]').waitFor({state:'visible'});
 assert(!(await json(node+'/api/config')).config.permissions.full_access?.includes('filesystem.read'));
 await page.locator('[data-field="permissions.http.enabled"]').check();assert(await page.locator('#saveBar').isVisible());
 await page.locator('#save').click();await page.locator('#restartBanner').waitFor({state:'visible'});await page.locator('#restartBanner [data-action=restart]').click();
 await until(async()=>{const s=await json(node+'/api/status');return s.permissions?.http?.enabled&&!s.pending_restart&&s.connection?.state==='connected';},'form-based permissions and restart');
 const permissionConfig=(await json(node+'/api/config')).config.permissions;
 assert(permissionConfig.full_access.includes('http.request'));
 assert(!permissionConfig.full_access.includes('filesystem.read'));
 assert.equal(permissionConfig.filesystem.roots.length,1);
 // Names and areas are edited without JSON; IDs and grants remain stable.
 await page.getByRole('link',{name:'Settings',exact:true}).click();
 await page.locator('[data-field="metadata.area"]').fill('Home / Upstairs / Study');
 await page.getByRole('button',{name:'Add resource',exact:true}).click();
 await page.locator('[data-resource-field="name"]').last().fill('Desk camera');
 await page.locator('#saveRestart').click();
 await until(async()=>{const s=await json(node+'/api/status');return s.connection?.state==='connected'&&s.metadata?.area?.at(-1)==='Study';},'area form saved and applied');
 const configuredMetadata=(await json(node+'/api/config')).config.metadata;
 assert.equal(configuredMetadata.resources[0].id,'fixture-file');
 assert.equal(configuredMetadata.resources[1].name,'Desk camera');
 assert.deepEqual((await json(node+'/api/config')).config.permissions,permissionConfig);
 await page.screenshot({path:join(scratch,'node-resources.png'),fullPage:true});
 const forged=await fetch(node+'/api/chat',{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify({message:'read file caller-cannot-override-binding',request_context:{origin_node:'forged'}})});
 assert.equal(forged.status,422,'browser input cannot provide origin authority');
 const routed=await reviewed.save({template:'read_file',params:{},constraints:{resource_id:id+'::fixture-file',node_id:id}},'Read the owner-selected fixture file');
 assert.equal(routed.request_context.origin_node,id);
 assert.deepEqual(routed.request_context.origin_area,['Home','Upstairs','Study']);
 assert.equal(interpretations,0);await reviewed.start(await reviewed.approve(routed));
 const fileReport=await reviewed.result(routed.id);assert.equal(fileReport.state,'completed');assert.equal(fileReport.success,true);
 assert.equal(fileReport.data.content,'Resource binding fixture');
 const execution=await json(node+'/api/status');
 assert.deepEqual(execution.last_workload.request_context,routed.request_context);
 assert.equal(execution.last_workload.resource_id,id+'::fixture-file');
 assert.equal(execution.last_workload.grant_id,fileReport.grant_id);
 assert.equal(execution.last_workload.execution_may_have_started,true);
 assert(!JSON.stringify(execution.last_workload).includes('Resource binding fixture'));
 const source=await reviewed.inspect(routed.id);assert.equal(source.request_review.original_text,'Read the owner-selected fixture file');
 const durable=(await json(node+'/api/jobs')).jobs.find(j=>j.request_context?.request_id===routed.request_context.request_id);
 assert(durable);assert.equal(durable.status,'completed');
 const savedJob=await json(node+'/api/jobs/'+durable.id);
 assert.equal(savedJob.tasks[0].assignment.node_id,id);assert.equal(savedJob.tasks[0].attempts,1);
 assert(!JSON.stringify(savedJob).includes('Resource binding fixture'));
 await page.getByRole('link',{name:'Jobs',exact:true}).click();
 await page.getByRole('button',{name:'View steps',exact:true}).click();
 await page.locator('#jobDetailsPanel').waitFor({state:'visible'});
 await page.locator('#jobDetails details').evaluate(e=>{e.open=true;window.savedJobDetails=e;});
 await page.evaluate(()=>{const e=document.querySelector('#jobDetails code');const r=document.createRange();r.selectNodeContents(e);getSelection().removeAllRanges();getSelection().addRange(r);window.jobCopy=getSelection().toString();});
 await sleep(6500);
 assert(await page.evaluate(()=>window.savedJobDetails===document.querySelector('#jobDetails details')&&window.savedJobDetails.open&&getSelection().toString()===window.jobCopy));
 await page.screenshot({path:join(scratch,'node-jobs.png'),fullPage:true});
 await page.evaluate(()=>getSelection().removeAllRanges());
 await page.goto(node+'/#home');await page.locator('#homeTitle').waitFor();
 // Without a selected interpreter, input must fail honestly, not execute a
 // remembered keyword or start another runtime. Successful review is tested
 // below only after the owner installs/selects the fake fixture model.
 await page.locator('#message').fill('remember running-node browser input works');
 await page.locator('#sendMessage').click();
 await page.waitForFunction(()=>document.getElementById('reply').textContent.includes('Input workflow not confirmed'));
 const missingInterpreter=await cli('eefn',configPath,['--ask','remember running-node CLI input works'],false);
 assert.equal(missingInterpreter.execution_authorized,false);assert.equal(missingInterpreter.dispatched,false);
 assert.equal(interpretations,0);assert.equal((await json(node+'/api/jobs')).jobs.length,1);
 assert.equal((await json(eef+'/api/status')).world.devices.filter(n=>n.connected).length,1);
 // Polls retain DOM identity, selection, and input caret.
 await page.evaluate(()=>{
  window.copyNode=document.querySelector('#homeCards p');
  const range=document.createRange();range.selectNodeContents(window.copyNode);
  getSelection().removeAllRanges();getSelection().addRange(range);window.copyText=getSelection().toString();
 });
 await sleep(6500);
 assert(await page.evaluate(()=>window.copyNode===document.querySelector('#homeCards p')&&getSelection().toString()===window.copyText));
 await page.evaluate(()=>getSelection().removeAllRanges());
 await page.locator('#message').fill('Keep this editable');
 await page.locator('#message').evaluate(e=>{e.focus();e.setSelectionRange(2,6);});
 await sleep(3200);
 assert(await page.locator('#message').evaluate(e=>e===document.activeElement&&e.selectionStart===2&&e.selectionEnd===6));
 await page.goto(node+'/#advanced');
 await page.evaluate(()=>{
  const range=document.createRange();range.selectNodeContents(document.getElementById('diagnostics'));
  getSelection().removeAllRanges();getSelection().addRange(range);window.copyText=getSelection().toString();
 });
 await sleep(6500);
 assert(await page.evaluate(()=>getSelection().toString()===window.copyText));
 await page.evaluate(()=>getSelection().removeAllRanges());
 await page.goto(node+'/#home');
 // Stop is persistent and local input cannot accidentally reconnect the node.
 await page.getByRole('link',{name:'Network',exact:true}).click();
 await page.locator('#pauseConnection').click();
 await until(async()=> (await json(node+'/api/status')).connection.state==='paused','stop connection');
 await json(node+'/api/restart',{});
 await until(async()=> (await json(node+'/api/status')).connection.state==='paused','stop persists after restart');
 await page.waitForFunction(()=>document.getElementById('pauseConnection').textContent==='Resume connection');
 await page.locator('#pauseConnection').click();
 await until(async()=> (await json(node+'/api/status')).connection.state==='connected','resume connection');
 await page.getByRole('link',{name:'Home',exact:true}).click();
 await page.screenshot({path:join(scratch,'device-home.png'),fullPage:true});
 await page.getByRole('link',{name:'Models',exact:true}).click();await page.getByRole('button',{name:'Install model',exact:true}).click();
 await until(async()=> (await json(node+'/api/status')).download.state==='installed','model installation progress');
 await page.locator('#refreshModels').click();await page.getByRole('button',{name:'Use model',exact:true}).click();await page.locator('#saveRestart').click();
 await until(async()=>{const s=await json(eef+'/api/status');return s.models?.some(m=>m.model_id==='test-model:tiny');},'selected model advertised to eef');
 assert.equal(pulls,1);stallPull=true;await json(node+'/api/models/install',{id:'test-model'});await json(node+'/api/models/cancel',{});
 await until(async()=> (await json(node+'/api/status')).download.state==='cancelled','stalled model download cancellation',5000);
 assert.equal((await json(node+'/api/models')).storage.scope,'ollama');
 assert.equal((await json(node+'/api/models')).storage.free_bytes,null);
 assert.equal((await json(node+'/api/models/inspect',{id:'test-model:tiny'})).modality,'text');
 const withoutRole=await cli('eefn',configPath,['--ask','Inspect status without an owner-selected interpreter role'],false);
 assert.equal(withoutRole.error_code,'interpreter_unavailable');assert.equal(interpretations,0);
 assert(withoutRole.setup_hint.includes('request_interpreter'));assert(withoutRole.setup_hint.includes('restart'));
 // The existing model picker does not configure interpreter roles. This is an
 // explicit command setup step, not a claimed complete non-coder workflow.
 await cli('eefn',configPath,['models','hints','--backend','ollama','--model','test-model:tiny','--role','request_interpreter']);
 await cli('eefn',configPath,['restart','--wait-seconds','60']);
 await until(async()=>{const s=await json(node+'/api/status');return s.connection.state==='connected'&&(await json(eef+'/api/commands/models/route?capability=llm.infer&role=request_interpreter')).candidates.length===1;},'explicit interpreter role applied');
 // Browser and CLI are adapters over the same durable review service. The fake
 // backend tests the protocol only, not real-model understanding or auto-work.
 await page.goto(node+'/#home');
 await page.reload();
 await page.waitForFunction(()=>document.getElementById('health')?.textContent==='Connected');
 // The existing browser field is single-line; verify its leading/trailing
 // spaces. CLI/HTTP original multiline preservation is tested separately.
 const browserText='  Inspect running-node browser status  ';
 const browserReply=page.waitForResponse(r=>r.url()===node+'/api/chat'&&r.request().postDataJSON().message===browserText);
 await page.locator('#message').fill(browserText);await page.locator('#sendMessage').click();
 const browserReport=await (await browserReply).json();assert.equal(browserReport.success,true,JSON.stringify(browserReport));
 await page.waitForFunction(()=>document.getElementById('reply').textContent.includes('Prepared request'));
 const cliText='  Inspect running-node CLI status\n';
 const opened=await cli('eefn',configPath,['--ask',cliText]);
 assert.equal(opened.coordinator_acknowledged,true);assert.equal(opened.execution_authorized,false);assert.equal(opened.dispatched,false);
 assert.equal(opened.result.draft.original_text,cliText);assert.equal(opened.result.status,'awaiting_approval');
 assert.equal(opened.preparation.checkpoint_saved,true);assert.equal(browserReport.preparation.checkpoint_saved,true);
 const inputs=(await json(node+'/api/jobs')).jobs.filter(j=>j.status==='awaiting_approval');assert.equal(inputs.length,2);
 const drafts=await Promise.all(inputs.map(j=>reviewed.inspect(j.id)));
 assert(drafts.some(p=>p.request_review.original_text===browserText));
 assert(drafts.every(p=>p.goal.request_context.origin_node===id&&p.execution===undefined));
 assert.equal(interpretations,4);assert.equal((await json(eef+'/api/status')).world.devices.filter(n=>n.connected).length,1);
 const coordinator=await context.newPage();coordinator.on('pageerror',e=>{throw e;});await coordinator.goto(eef+'/#devices');await coordinator.getByRole('button',{name:'Configure node'}).click();await coordinator.locator('#remoteBanner').waitFor({state:'visible'});await coordinator.getByRole('link',{name:'Settings',exact:true}).click();await coordinator.locator('[data-field=name]').fill('Named from eef UI');await coordinator.locator('[data-field="metadata.area"]').fill('Home / Remote study');await coordinator.locator('[data-resource-field="name"]').last().fill('Remotely named camera');await coordinator.locator('#saveRestart').click();
 await until(async()=>{const s=await json(eef+'/api/status');return s.world?.devices?.some(n=>n.node_id===id&&n.name==='Named from eef UI'&&n.connected);},'configuration from eef UI');
 assert.deepEqual((await json(node+'/api/status')).metadata.area,['Home','Remote study']);
 assert.equal((await json(node+'/api/config')).config.metadata.resources[1].name,'Remotely named camera');
 const remoteResource=(await manage('status')).metadata;
 assert.deepEqual(remoteResource.area,['Home','Remote study']);
 await coordinator.locator('#backToEEF').click();await coordinator.goto(eef+'/#devices');
 await coordinator.locator('#devices details').evaluate(e=>{e.open=true;window.keptDetails=e;});
 await sleep(3500);assert(await coordinator.evaluate(()=>window.keptDetails===document.querySelector('#devices details')&&window.keptDetails.open));
 await coordinator.goto(eef+'/#home');await coordinator.screenshot({path:join(scratch,'eef-home.png'),fullPage:true});
 await page.setViewportSize({width:390,height:844});await page.screenshot({path:join(scratch,'device-mobile.png'),fullPage:true});assert(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth));
 const oldRuntime=(await json(eef+'/api/status')).runtime_id;assert(oldRuntime);
 await json(eef+'/api/restart',{});await until(async()=>{const s=await json(eef+'/api/status');return s.runtime_id!==oldRuntime&&s.world?.devices?.some(n=>n.node_id===id&&n.connected)&&(await json(node+'/api/status')).connection?.state==='connected';},'new eef instance and node reconnect');
 const retained=await reviewed.inspect(routed.id);
 assert.equal(retained.goal.request_context.origin_area.at(-1),'Study');
 assert.equal(retained.request_review.original_text,source.request_review.original_text);
 const reopenedJob=await json(node+'/api/jobs/'+durable.id);
 assert.equal(reopenedJob.status,'completed');assert.equal(reopenedJob.revision,savedJob.revision);
 assert.deepEqual(reopenedJob.request_context,savedJob.request_context);assert.deepEqual(reopenedJob.tasks,savedJob.tasks);
 // Retired owner raw-job creation cannot borrow a node's input context.
 // Cross-origin visibility/control is covered by the two-logical-node fixture;
 // this single-node browser test does not manufacture an owner workload.
 const beforeLegacy=(await json(eef+'/api/jobs')).jobs.length;
 const legacy=await fetch(eef+'/api/jobs',{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify({description:'owner-only fixture',template:'read_file',params:{resource_id:id+'::fixture-file'},request_context:routed.request_context})});
 assert.equal(legacy.status,400);assert.equal((await legacy.json()).error_code,'execution_grant_required');
 assert.equal((await json(eef+'/api/jobs')).jobs.length,beforeLegacy);
 await coordinator.goto(eef+'/#settings');
 await coordinator.locator('[data-field="jobs.max_count"]').fill('250');
 await coordinator.locator('[data-field="jobs.max_bytes"]').fill('32');
 await coordinator.locator('#save').click();
 await coordinator.locator('#saveBar').waitFor({state:'hidden'});
 const jobSettings=(await json(eef+'/api/config')).config.jobs;
 assert.equal(jobSettings.max_count,250);assert.equal(jobSettings.max_bytes,32*1048576);
 assert.equal((await json(eef+'/api/jobs')).storage.max_count,500);
 await coordinator.reload();await coordinator.locator('[data-field="jobs.max_count"]').waitFor({state:'visible'});
 await coordinator.waitForFunction(()=>document.querySelector('[data-field="jobs.max_count"]').value==='250');
 assert.equal(await coordinator.locator('[data-field="jobs.max_bytes"]').inputValue(),'32');
 const beforeSettingsRestart=(await json(eef+'/api/status')).runtime_id;
 await json(eef+'/api/restart',{});
 await until(async()=>{const s=await json(eef+'/api/status');return s.runtime_id!==beforeSettingsRestart&&s.world?.devices?.some(n=>n.node_id===id&&n.connected)&&(await json(node+'/api/status')).connection?.state==='connected';},'job storage preferences applied after restart');
 const jobUsage=(await json(node+'/api/jobs')).storage;
 assert.equal(jobUsage.max_count,250);assert.equal(jobUsage.max_bytes,32*1048576);
 assert.equal(jobUsage.reserved_bytes,inputs.length*1024,'retained prepared requests reserve exact future checkpoint space');
 for(const draft of drafts){assert.deepEqual(await reviewed.inspect(draft.plan_id),draft,'prepared input survives restart without execution');await reviewed.cleanup(draft.plan_id);}
 const released=(await json(node+'/api/jobs')).storage;
 assert.equal(released.reserved_bytes,0);assert.equal(released.count,jobUsage.count-inputs.length);
 assert(released.available_bytes>jobUsage.available_bytes,'explicit removal releases accounted headroom');
 await page.goto(node+'/#jobs');await page.waitForFunction(()=>document.getElementById('jobStorage').textContent.includes('32.00 MiB'));
 await page.screenshot({path:join(scratch,'node-job-storage.png'),fullPage:true});
 await writeFile(join(scratch,'jobs-results.json'),JSON.stringify({passed:true,physical_two_pc:false,pinned_tls:true,reviewed_grants:true,completed_jobs_survive_verified_restart:true,node_origin_preserved:true,retired_raw_creation_refused:true,copy_and_details_retained:true,storage_form_and_applied_limits:true,fresh_input_awaits_approval_without_dispatch:true,retained_prepared_request_count:inputs.length,reserved_before_removal:jobUsage.reserved_bytes,reserved_after_removal:released.reserved_bytes},null,2));
 await writeFile(join(scratch,'results.json'),JSON.stringify({passed:true,physical_two_pc:false,real_inference:false,non_coder_setup_complete:false,owner_cli_interpreter_role_setup_required:true,stable_identity:true,local_registration:true,explicit_owner_tls_pairing:true,remote_approval:true,remote_configuration:true,device_restart:true,eef_restart:true,browser_forms:true,copy_selection_and_focus_stable:true,running_node_browser_and_cli_review_input:true,missing_interpreter_no_keyword_fallback:true,feature_wide_permissions:true,mobile_layout:true,no_initial_model_download:true,mock_ollama_install_select_and_register:true,stalled_download_cancellation:true,origin_survives_routing_and_restart:true,resource_bindings_private_and_owner_selected:true,area_and_resource_forms:true},null,2));
 console.log('First-run and browser validation passed: '+scratch);
}catch(e){console.error(errors.slice(-12).join('\n'));console.error('Child exit codes:',children.map(c=>({pid:c.pid,exit:c.exitCode})));for(const url of [eef,node]){try{console.error(url,await (await fetch(url+'/api/status',{signal:AbortSignal.timeout(2000)})).json());}catch(error){console.error(url,error.message);}}throw e;}finally{if(browser)await browser.close();for(const child of children)if(child.exitCode===null)child.kill();await Promise.allSettled(children.map(c=>c.result));if(modelServer){modelServer.closeAllConnections();await new Promise(r=>modelServer.close(r));}}
