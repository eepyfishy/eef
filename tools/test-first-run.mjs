// Integration + browser regression. Fixtures stay in .validation, never personal config.
import assert from 'node:assert/strict';
import {mkdtemp,mkdir,readFile,writeFile} from 'node:fs/promises';
import {resolve,join} from 'node:path';
import {spawn} from 'node:child_process';
import {createServer} from 'node:net';
import {request as httpRequest,createServer as httpServer} from 'node:http';
import {randomBytes} from 'node:crypto';
import {chromium} from '../.tooling/ui-tests/node_modules/playwright/index.mjs';
const root=resolve(import.meta.dirname,'..'),scratch=await mkdtemp(join(root,'.validation','first-run-'));
const children=[],errors=[];let browser,modelServer;
const sleep=ms=>new Promise(r=>setTimeout(r,ms));
async function port(){const server=createServer();await new Promise(r=>server.listen(0,'127.0.0.1',r));const p=server.address().port;await new Promise(r=>server.close(r));return p;}
const webPort=await port(),devicePort=await port(),nodePort=await port();
const eef=`http://127.0.0.1:${webPort}`,node=`http://127.0.0.1:${devicePort}`;
async function json(url,body,method){const r=await fetch(url,{method:method||(body?'POST':'GET'),headers:{'Content-Type':'application/json'},body:body?JSON.stringify(body):undefined});const data=await r.json();assert(r.ok,JSON.stringify(data));return data;}
async function until(fn,label,timeout=30000){const end=Date.now()+timeout;while(Date.now()<end){try{const v=await fn();if(v)return v;}catch{}await sleep(250);}throw Error('Timed out: '+label);}
function launch(name,args,extra={}){const child=spawn(join(root,'target/debug',name+'.exe'),args,{cwd:root,windowsHide:true,env:{...process.env,PATH:join(root,'.tooling/llvm-mingw-20260616-ucrt-x86_64/bin')+';'+process.env.PATH,APPDATA:join(scratch,'appdata'),EEF_DISCOVERY_DIR:join(scratch,'discovery'),EEF_NODE_PSK:'',...extra},stdio:['ignore','pipe','pipe']});child.stdout.on('data',()=>{});child.stderr.on('data',b=>errors.push(name+': '+b.toString()));children.push(child);return child;}
const configPath=join(scratch,'node.json');
try{
 let installed=false,stallPull=false,pulls=0;
 modelServer=httpServer(async(req,res)=>{
  if(req.url==='/api/tags'){res.setHeader('Content-Type','application/json');res.end(JSON.stringify({models:installed?[{name:'test-model:tiny',size:1024}]:[]}));return;}
  if(req.url==='/api/pull'){pulls++;res.setHeader('Content-Type','application/x-ndjson');res.write(JSON.stringify({status:'pulling',completed:128,total:1024})+'\n');if(stallPull)return;await sleep(300);installed=true;res.end(JSON.stringify({status:'success',completed:1024,total:1024})+'\n');return;}
  res.writeHead(404);res.end();
 });await new Promise(r=>modelServer.listen(0,'127.0.0.1',r));
 const config=JSON.parse(await readFile(join(root,'config/node.example.json'),'utf8'));config.dashboard.port=devicePort;config.update.policy='off';
 config.models.ollama.base_url=`http://127.0.0.1:${modelServer.address().port}`;config.model_catalog=[{id:'test-model',name:'Test model',ollama:'test-model:tiny',bytes:1024,ram_recommended_gb:1}];
 await writeFile(configPath,JSON.stringify(config));await mkdir(join(scratch,'Documents'));
 launch('eefn',['--config',configPath]);
 const waiting=await until(async()=>{const s=await json(node+'/api/status');return s.connection?.state==='waiting'&&s;},'device starts without EEF');
 const id=waiting.node_id;assert.match(id,/^device-/);assert.notEqual(waiting.name,'Example node');
 assert.equal(pulls,0);assert.equal(waiting.models.length,0);assert.equal((await json(node+'/api/models')).backend,'ollama');
 const duplicate=launch('eefn',['--config',configPath]);await until(()=>duplicate.exitCode===0,'second launch reuses running device');
 const yaml=(await readFile(join(root,'config/default_identity.yaml'),'utf8')).replace('port: 51334',`port: ${webPort}`).replace('port: 51335',`port: ${nodePort}`).replace('policy: prompt','policy: off');
 const eefConfig=join(scratch,'eef.yaml');await writeFile(eefConfig,yaml);
 launch('eef',['--config',eefConfig,'--database',join(scratch,'eef.db'),'--no-brain'],{EEF_NODE_PSK:randomBytes(32).toString('hex')});
 await until(async()=>{const s=await json(node+'/api/status');return s.connection?.state==='connected';},'automatic local pairing');
 await until(async()=>{const s=await json(eef+'/api/status');return s.world?.devices?.some(n=>n.node_id===id&&n.connected);},'normal protocol registration');
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
 await page.getByRole('link',{name:'Permissions',exact:true}).click();await page.locator('[data-field="permissions.http.allowed_hosts"]').fill('example.com');await page.locator('[data-field="permissions.http.enabled"]').check();assert(await page.locator('#saveBar').isVisible());
 await page.locator('#save').click();await page.locator('#restartBanner').waitFor({state:'visible'});await page.locator('#restartBanner [data-action=restart]').click();
 await until(async()=>{const s=await json(node+'/api/status');return s.permissions?.http?.enabled&&!s.pending_restart&&s.connection?.state==='connected';},'form-based permissions and restart');
 await page.goto(node+'/#home');await page.locator('#homeTitle').waitFor();await page.screenshot({path:join(scratch,'device-home.png'),fullPage:true});
 await page.getByRole('link',{name:'Models',exact:true}).click();await page.getByRole('button',{name:'Install model',exact:true}).click();
 await until(async()=> (await json(node+'/api/status')).download.state==='installed','model installation progress');
 await page.locator('#refreshModels').click();await page.getByRole('button',{name:'Use model',exact:true}).click();await page.locator('#saveRestart').click();
 await until(async()=>{const s=await json(eef+'/api/status');return s.models?.some(m=>m.model_id==='test-model:tiny');},'selected model advertised to EEF');
 assert.equal(pulls,1);stallPull=true;await json(node+'/api/models/install',{id:'test-model'});await json(node+'/api/models/cancel',{});
 await until(async()=> (await json(node+'/api/status')).download.state==='error','stalled model download cancellation',5000);
 const coordinator=await context.newPage();coordinator.on('pageerror',e=>{throw e;});await coordinator.goto(eef+'/#devices');await coordinator.getByRole('button',{name:'Configure device'}).click();await coordinator.locator('#remoteBanner').waitFor({state:'visible'});await coordinator.getByRole('link',{name:'Settings',exact:true}).click();await coordinator.locator('[data-field=name]').fill('Named from EEF UI');await coordinator.locator('#saveRestart').click();
 await until(async()=>{const s=await json(eef+'/api/status');return s.world?.devices?.some(n=>n.node_id===id&&n.name==='Named from EEF UI'&&n.connected);},'configuration from EEF UI');
 await coordinator.locator('#backToEEF').click();await coordinator.goto(eef+'/#home');await coordinator.screenshot({path:join(scratch,'eef-home.png'),fullPage:true});
 await page.setViewportSize({width:390,height:844});await page.screenshot({path:join(scratch,'device-mobile.png'),fullPage:true});assert(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth));
 await json(eef+'/api/restart',{});await until(async()=>{const s=await json(eef+'/api/status');return s.world?.devices?.some(n=>n.node_id===id&&n.connected);},'EEF restart and device reconnect');
 await writeFile(join(scratch,'results.json'),JSON.stringify({passed:true,stable_identity:true,local_pairing:true,remote_approval:true,remote_configuration:true,device_restart:true,eef_restart:true,browser_forms:true,mobile_layout:true,no_initial_model_download:true,mock_ollama_install_select_and_register:true,stalled_download_cancellation:true},null,2));
 console.log('First-run and browser validation passed: '+scratch);
}catch(e){console.error(errors.slice(-12).join('\n'));throw e;}finally{if(browser)await browser.close();for(const child of children)if(child.exitCode===null)child.kill();if(modelServer){modelServer.closeAllConnections();modelServer.close();}}
