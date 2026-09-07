// Fast browser-only regression: real dashboard source with bounded local mocks.
import assert from 'node:assert/strict';
import {readFile} from 'node:fs/promises';
import {chromium} from '../.tooling/ui-tests/node_modules/playwright/index.mjs';
const browser=await chromium.launch({executablePath:process.env.EEF_TEST_BROWSER||'C:\\Program Files\\Google\\Chrome\\Application\\chrome.exe',headless:true});
try{
 const page=await browser.newPage();page.on('pageerror',e=>console.error(e));
 let role='device';
 let job={id:'job-fixture',description:'Paused fixture job',status:'paused',can_resume:true,completed_tasks:0,task_count:1,updated_at_ms:Date.now(),tasks:[],history:[]},jobActions=[];
 let details={capabilities_known:true,modality:'vlm',capabilities:['llm.infer','vlm.analyze']};
 const config=JSON.parse(await readFile(new URL('../config/node.example.json',import.meta.url),'utf8'));
 const source={};for(const name of ['app.html','app.js','app.css'])source[name]=await readFile(new URL('../dashboard/'+name,import.meta.url),'utf8');
 await page.route('http://localhost:51336/**',async route=>{
  const path=new URL(route.request().url()).pathname;
  if(path==='/')return route.fulfill({contentType:'text/html',body:source['app.html']});
  if(path==='/app.js'||path==='/app.css')return route.fulfill({contentType:path.endsWith('.js')?'text/javascript':'text/css',body:source[path.slice(1)]});
  if(path==='/api/models')return route.fulfill({json:{backend:'ollama',ollama_models:[{name:'arbitrary-model',size:1024}],catalog:[],storage:{free_bytes:null,message:'Ollama storage is not reported.'}}});
  if(path==='/api/models/inspect')return route.fulfill({json:details});
  if(path==='/api/jobs')return route.fulfill({json:{jobs:job?[job]:[],total:job?1:0}});
  if(path.startsWith('/api/jobs/job-fixture')){
   if(route.request().method()==='DELETE'){jobActions.push('remove');job=null;return route.fulfill({json:{removed:true}});}
   if(route.request().method()==='POST'){
    const action=path.split('/').at(-1);jobActions.push(action);
    job.status={resume:'running',pause:'paused',stop:'cancelled'}[action];job.can_resume=job.status==='paused';
   }
   return route.fulfill({json:job});
  }
  const result=path==='/api/ui'?{role}:path==='/api/config'?{config}:path==='/api/status'?{connection:{state:'connected'},name:'Fixture node',hardware:{cpu_name:'Fixture CPU'},permissions:{},capabilities:[],world:{devices:[{node_id:'test-node',name:'Fixture node',connected:true}]}}:path==='/api/startup'?{supported:false}:{};
  return route.fulfill({contentType:'application/json',body:JSON.stringify(result)});
 });
 await page.goto('http://localhost:51336');
 try{await page.waitForFunction(()=>document.getElementById('health').textContent==='Connected',{},{timeout:5000});}
 catch(error){console.error('Dashboard notice:',await page.locator('#notice').textContent());throw error;}
 await page.getByRole('link',{name:'Permissions',exact:true}).click({timeout:5000});
 await page.getByRole('link',{name:'Settings',exact:true}).click();
 await page.locator('[data-field="metadata.area"]').fill('Home / Upstairs / Office');
 await page.getByRole('button',{name:'Add resource',exact:true}).click();
 await page.locator('[data-resource-field="name"]').fill('Desk camera');
 await page.locator('#resources details').evaluate(e=>e.open=true);
 await page.locator('[data-resource-field="device"]').fill('2');
 await page.getByRole('link',{name:'Advanced',exact:true}).click();
 let edited=JSON.parse(await page.locator('#rawConfig').inputValue());
 assert.deepEqual(edited.metadata.area,['Home','Upstairs','Office']);
 assert.equal(edited.metadata.resources[0].parameters.device,2);
 assert.equal(edited.metadata.resources[0].name,'Desk camera');
 const resourceId=edited.metadata.resources[0].id;
 await page.getByRole('link',{name:'Settings',exact:true}).click();
 await page.locator('[data-resource-field="name"]').fill('Renamed camera');
 await page.waitForTimeout(3500);
 assert.equal(await page.locator('[data-resource-field="name"]').inputValue(),'Renamed camera');
 await page.getByRole('link',{name:'Advanced',exact:true}).click();
 edited=JSON.parse(await page.locator('#rawConfig').inputValue());
 assert.equal(edited.metadata.resources[0].id,resourceId);
 assert.deepEqual(edited.permissions,config.permissions);
 await page.getByRole('link',{name:'Settings',exact:true}).click();
 await page.getByRole('button',{name:'Remove resource',exact:true}).click();
 assert.equal(await page.locator('[data-resource]').count(),0);
 await page.locator('#discard').click();
 await page.getByRole('link',{name:'Models',exact:true}).click();
 await page.getByRole('button',{name:'Use model',exact:true}).click();
 await page.getByRole('button',{name:'Selected',exact:true}).waitFor();
 await page.getByRole('link',{name:'Advanced',exact:true}).click();
 assert.equal(JSON.parse(await page.locator('#rawConfig').inputValue()).models.ollama.selected[0].modality,'vlm');
 await page.locator('#discard').click();details={capabilities_known:false,modality:null};
 await page.getByRole('link',{name:'Models',exact:true}).click();
 await page.getByRole('button',{name:'Use model',exact:true}).click();
 await page.waitForFunction(()=>document.getElementById('notice').textContent.includes('did not report capabilities'));
 assert.equal(await page.getByRole('button',{name:'Selected',exact:true}).count(),0);
 await page.locator('[data-model-type]').selectOption('text');
 await page.getByRole('button',{name:'Use model',exact:true}).click();
 await page.getByRole('button',{name:'Selected',exact:true}).waitFor();
 await page.getByRole('link',{name:'Advanced',exact:true}).click();
 assert.equal(JSON.parse(await page.locator('#rawConfig').inputValue()).models.ollama.selected[0].modality,'text');
 await page.locator('#discard').click();
 await page.getByRole('link',{name:'Home',exact:true}).click({timeout:5000});
 await page.evaluate(()=>{
  const node=document.querySelector('#homeCards p');window.originalNode=node;
  const range=document.createRange();range.selectNodeContents(node);getSelection().removeAllRanges();getSelection().addRange(range);
 });
 await page.waitForTimeout(6500);
 assert(await page.evaluate(()=>window.originalNode===document.querySelector('#homeCards p')&&getSelection().toString()==='Fixture CPU'));
 await page.evaluate(()=>getSelection().removeAllRanges());
 await page.getByRole('link',{name:'Jobs',exact:true}).click();
 await page.getByRole('button',{name:'Resume',exact:true}).waitFor();
 assert(await page.locator('#jobList .badge').evaluate(e=>e.classList.contains('warn')));
 page.on('dialog',dialog=>dialog.accept());
 await page.getByRole('button',{name:'Resume',exact:true}).click();
 await page.getByRole('button',{name:'Pause',exact:true}).click();
 await page.getByRole('button',{name:'Resume',exact:true}).waitFor();
 await page.getByRole('button',{name:'Stop',exact:true}).click();
 await page.getByRole('button',{name:'Remove history',exact:true}).click();
 await page.waitForFunction(()=>document.getElementById('jobState').textContent.includes('No saved jobs yet'));
 assert.deepEqual(jobActions,['resume','pause','stop','remove']);
 role='eef';await page.goto('about:blank');await page.goto('http://localhost:51336/#devices');
 try{await page.getByRole('button',{name:'Configure node'}).waitFor({timeout:5000});}
 catch(error){console.error('EEF DOM:',await page.locator('#devices').innerHTML(),'Health:',await page.locator('#health').textContent());throw error;}
 console.log('Dashboard initialization and copy stability passed');
}finally{await browser.close();}
