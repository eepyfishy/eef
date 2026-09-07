'use strict';
const $=id=>document.getElementById(id), esc=v=>String(v??'').replace(/[&<>"']/g,c=>({'&':'&amp;','<':'&lt;','>':'&gt;','"':'&quot;',"'":'&#39;'}[c]));

// Live updates preserve unchanged DOM nodes, expanded details and keyboard focus.
// A selected region keeps its displayed snapshot until selection is cleared.
// The next normal poll applies deferred data; selection never blocks the network.
const renderedHTML=new WeakMap();
function selectedWithin(element){
 const selection=getSelection();
 if(!selection||selection.isCollapsed)return false;
 for(let i=0;i<selection.rangeCount;i++)if(selection.getRangeAt(i).intersectsNode(element))return true;
 return false;
}
function reconcile(parent,desired){
 const wanted=[...desired.childNodes];
 for(let i=0;i<wanted.length;i++){
  const incoming=wanted[i],current=parent.childNodes[i];
  if(!current){parent.append(incoming.cloneNode(true));continue;}
  if(current.nodeType!==incoming.nodeType||current.nodeName!==incoming.nodeName){
   current.replaceWith(incoming.cloneNode(true));continue;
  }
  if(current.nodeType===Node.TEXT_NODE){if(current.data!==incoming.data)current.data=incoming.data;continue;}
  if(current.nodeType!==Node.ELEMENT_NODE)continue;
  for(const attr of [...current.attributes]){
   if(current.tagName==='DETAILS'&&attr.name==='open')continue;
   if(!incoming.hasAttribute(attr.name))current.removeAttribute(attr.name);
  }
  for(const attr of incoming.attributes)if(current.getAttribute(attr.name)!==attr.value)current.setAttribute(attr.name,attr.value);
  reconcile(current,incoming);
 }
 while(parent.childNodes.length>wanted.length)parent.lastChild.remove();
}
function live(id){
 const element=$(id);
 return {
  set textContent(value){
   const text=String(value??'');
   if(element.textContent!==text&&!selectedWithin(element))element.textContent=text;
  },
  set innerHTML(value){
   const html=String(value??'');
   if(renderedHTML.get(element)===html||selectedWithin(element))return;
   const active=document.activeElement;
   if(element.contains(active)&&active.matches('input,textarea,select,[contenteditable=true]'))return;
   const template=document.createElement('template');template.innerHTML=html;
   reconcile(element,template.content);renderedHTML.set(element,html);
  }
 };
}

const clone=v=>JSON.parse(JSON.stringify(v)), get=(o,p)=>p.split('.').reduce((v,k)=>v?.[k],o);
function put(o,p,v){const keys=p.split('.');let item=o;for(const key of keys.slice(0,-1))item=item[key]??={};item[keys.at(-1)]=v;}
let role='device',config={},saved={},dirty=false,latest={},network={},remote=null,modelData={},polling=false,reviewing=false,noticeTimer;
const isDevice=()=>role==='device'||remote!==null;
async function request(path,options={}){const r=await fetch(path,{...options,headers:{'Content-Type':'application/json',...options.headers}});let value;try{value=await r.json();}catch{throw Error('The app is restarting or is not reachable. It will reconnect automatically.');}if(!r.ok)throw Error(value.error||'The request could not be completed. Try again.');return value;}
const send=(path,value={},method='POST')=>request(path,{method,body:JSON.stringify(value)});
async function api(path,options={}){
  if(!remote)return request(path,options);
  const actions={'/api/config':options.method==='PUT'?'save':'get','/api/status':'status','/api/models':'models','/api/restart':'restart','/api/models/install':'install','/api/models/cancel':'cancel','/api/models/inspect':'inspect'};
  if(!actions[path])throw Error('Open the node app on that PC to change this setting.');
  return send('/api/devices/'+encodeURIComponent(remote.id)+'/manage',{action:actions[path],params:options.body?JSON.parse(options.body):{}});
}
const act=(path,value={},method='POST')=>api(path,{method,body:JSON.stringify(value)});
function notice(message,error=false){clearTimeout(noticeTimer);$('notice').hidden=false;$('notice').className=error?'error':'success';live('notice').textContent=message;if(!error)noticeTimer=setTimeout(()=>$('notice').hidden=true,6500);}
function attempt(fn){return async event=>{event?.preventDefault();try{await fn(event);}catch(e){notice(e.message,true);}};}
function changed(){dirty=JSON.stringify(config)!==JSON.stringify(saved);$('saveBar').hidden=!dirty;live('saveState').textContent='Unsaved changes';live('permissionState').textContent=dirty?'Unsaved — these changes are not active yet.':'Only applied settings are active. Saved changes may need a restart.';}
const parseArea=value=>value.split('/').map(s=>s.trim()).filter(Boolean);
function syncFields(){for(const field of document.querySelectorAll('[data-field]')){const value=get(config,field.dataset.field);if(field.type==='checkbox')field.checked=value===true;else field.value=field.dataset.scale&&value!==undefined?value/Number(field.dataset.scale):field.dataset.area?(Array.isArray(value)?value.join(' / '):''):field.dataset.list?(Array.isArray(value)?value.join('\n'):''):(value??'');} $('rawConfig').value=JSON.stringify(config,null,2);renderEndpoints();renderResources();permissionScopes();changed();}
function wireFields(){for(const field of document.querySelectorAll('[data-field]'))field.oninput=()=>{let value=field.type==='checkbox'?field.checked:field.value;if(field.dataset.number){if(!field.checkValidity())return;value=Math.round(Number(field.value)*Number(field.dataset.scale||1));}if(field.dataset.area)value=parseArea(field.value);if(field.dataset.list)value=field.value.split(/\n/).map(s=>s.trim()).filter(Boolean).map(s=>field.dataset.list==='number'?Number(s):s);put(config,field.dataset.field,value);if(field.dataset.grant){const grants=new Set(get(config,'permissions.full_access')||[]);if(value)grants.add(field.dataset.grant);else grants.delete(field.dataset.grant);put(config,'permissions.full_access',[...grants]);}permissionScopes();changed();};}
function renderResources(){
 const resources=config.metadata?.resources||[];
 // Rebuilt only after an explicit settings load/add/remove, never by polling.
 $('resources').innerHTML=resources.map((r,i)=>`<section class="panel" data-resource="${i}"><h4>${esc(prettyCapability(r.capability))}</h4><label>Resource name<input data-resource-field="name" value="${esc(r.name||'')}"></label><label>Resource area<input data-resource-field="area" value="${esc((r.area||[]).join(' / '))}" placeholder="Use node area"></label><label class="toggle"><input data-resource-field="available" type="checkbox" ${r.available!==false?'checked':''}>Offer this resource when its feature is enabled</label><details><summary>Resource details</summary><p>Stable resource ID: <code>${esc(config.node_id+'::'+r.id)}</code></p>${['camera.capture','audio.capture'].includes(r.capability)?`<label>Hardware index (optional)<input data-resource-field="device" type="number" min="0" ${r.capability==='camera.capture'?'max="16"':''} step="1" value="${esc(r.parameters?.device??'')}" placeholder="System default"></label><small>Leave blank for the default. An index selects an existing adapter; saving does not test or activate it.</small>`:''}</details><button type="button" data-remove-resource class="secondary">Remove resource</button></section>`).join('')||'<p>No named resources yet. Enabled features still use their defaults.</p>';
 for(const section of $('resources').querySelectorAll('[data-resource]')){
  const resource=resources[Number(section.dataset.resource)];
  for(const field of section.querySelectorAll('[data-resource-field]'))field.oninput=()=>{
   const key=field.dataset.resourceField;
   if(key==='device'){
    resource.parameters??={};
    if(field.value==='')delete resource.parameters.device;
    else if(field.checkValidity())resource.parameters.device=Number(field.value);
    else{field.reportValidity();return;}
   }else resource[key]=key==='area'?parseArea(field.value):key==='available'?field.checked:field.value;
   changed();
  };
  section.querySelector('[data-remove-resource]').onclick=()=>{resources.splice(resources.indexOf(resource),1);renderResources();changed();};
 }
 $('addResource').onclick=()=>{
  config.metadata??={};config.metadata.resources??=[];
  if(config.metadata.resources.length>=128){notice('This node supports up to 128 named resources.',true);return;}
  const capability=$('resourceType').value;
  config.metadata.resources.push({id:crypto.randomUUID(),name:prettyCapability(capability),capability,area:[],available:true});
  renderResources();changed();$('resources').lastElementChild.querySelector('input').focus();
 };
}
function visibility(){document.querySelectorAll('.device-only').forEach(e=>e.hidden=!isDevice());document.querySelectorAll('.eef-only').forEach(e=>e.hidden=isDevice());$('autoApply').closest('label').hidden=!isDevice();$('startup').disabled=!!remote;document.querySelectorAll('[data-page=network] .device-only input,[data-page=network] .device-only textarea,[data-page=network] .device-only button').forEach(e=>e.disabled=!!remote);for(const id of ['checkUpdate','applyUpdate'])$(id).disabled=!!remote;live('updateStatus').textContent=remote?'Check and install updates in that PC’s node app.':'';if($('remoteAllowed'))$('remoteAllowed').disabled=!!remote;live('brandRole').textContent=isDevice()?'Node app':'Your EEF network';live('appRole').textContent=remote?'MANAGING '+remote.name:(isDevice()?'THIS NODE':'YOUR NETWORK');}
function page(){const name=location.hash.slice(1)||'home',valid=['home','devices','jobs','models','permissions','network','settings','advanced'];const target=valid.includes(name)?name:'home';document.querySelectorAll('[data-page]').forEach(e=>e.hidden=e.dataset.page!==target);document.querySelectorAll('nav a').forEach(a=>a.setAttribute('aria-current',a.hash==='#'+target?'page':'false'));live('pageTitle').textContent=target==='devices'?'Nodes':target[0].toUpperCase()+target.slice(1);if(target==='models')loadModels().catch(e=>notice(e.message,true));if(target==='jobs')loadJobs();if(target==='advanced')$('rawConfig').value=JSON.stringify(config,null,2);}
const featureGrants={
 'filesystem.read':'filesystem.read','filesystem.write':'filesystem.write',
 'process.enabled':'process.exec','applications.enabled':'application.control',
 'http.enabled':'http.request','wake_on_lan.enabled':'network.wol'
};
const permissionGroups=[
 ['Files & apps',[
 ['filesystem.read','Read files','Read any file this Windows account can access.'],
 ['filesystem.write','Modify files','Create or change files anywhere this Windows account can write.',true],
 ['process.enabled','Run programs','Run programs and commands with this Windows account’s access.',true],
 ['applications.enabled','Control apps','List, launch, and close apps accessible to this account.',true]]],
 ['Network',[['http.enabled','Access websites and APIs','Send HTTP requests to internet and local-network services.',true],
 ['wake_on_lan.enabled','Wake other computers','Send wake signals without a computer allowlist. The target still needs Wake-on-LAN support.']]],
 ['Audio',[['media.microphone','Microphone','Record audio when requested.',true],['media.audio_output','Speakers','Play audio on this PC.'],['media.tts','Voice output','Read text aloud with the system voice.'],['stt.enabled','Speech recognition','Send audio to the speech service you choose.',true]]],
 ['Camera & screen',[['media.camera','Camera','Capture images from this PC’s camera.',true],['media.screen_capture','Screen','Capture what is visible on your display.',true]]],
 ['Updates',[['remote_updates','Allow remote updates','Allow EEF to install updates from the update feed you selected. Local updates remain available when this is off.',true]]],
 ['Input control',[['media.input_control','Keyboard & mouse','Click, type, and interact with this computer.',true]]]
];
function permissionScopes(){
 for(const item of document.querySelectorAll('[data-scope]')){
  const key=item.dataset.scope, enabled=get(config,'permissions.'+key);
  item.hidden=!enabled||(get(config,'permissions.full_access')||[]).includes(featureGrants[key]);
 }
}
function buildPermissions(){
 live('permissionGroups').innerHTML='<p class="risk">Each switch allows the feature’s supported actions with your Windows account’s access. Running programs, controlling apps, or keyboard and mouse access can also affect files, independently of the file switches. This does not grant administrator rights. Existing limited permissions stay limited until you explicitly switch them off and on.</p>'+
 permissionGroups.map(([name,permissions])=>`<section class="panel"><h3>${esc(name)}</h3><div class="permission-grid">${permissions.map(([key,label,help,risk])=>`<div class="permission"><label class="toggle"><input type="checkbox" data-field="permissions.${key}" ${featureGrants[key]?'data-grant="'+featureGrants[key]+'"':''}>${esc(label)}</label><small class="${risk?'risk':''}">${esc(help)}</small>${featureGrants[key]?'<small class="risk" data-scope="'+key+'" hidden>Limited by previous settings. Switch off and on to grant full feature access.</small>':''}</div>`).join('')}</div>${name==='Audio'?'<details><summary>Speech service settings</summary><label>Speech service address<input data-field="permissions.stt.endpoint" placeholder="Address supplied by your speech service"></label><label>Speech model<input data-field="permissions.stt.model"></label><small>Speech recognition needs a configured service and model. Other provider settings remain in Advanced.</small></details>':''}</section>`).join('');
}
function prettyCapability(id){return({'llm.infer':'Text AI','vlm.analyze':'Image understanding','system.info':'Node information','system.ping':'Connection checks','node.update':'Updates','node.configure':'Node settings','filesystem':'File access','process.exec':'Programs','application.control':'App control','launch_application':'App launching','http.request':'Web access','stt.transcribe':'Speech recognition','network.wol':'Wake computers','audio.capture':'Microphone','microphone.capture':'Microphone','audio.play':'Speakers','audio.output':'Speakers','tts.speak':'Voice output','camera.capture':'Camera','screen.capture':'Screen','input.control':'Keyboard & mouse'})[id]||'Additional node service';}
function stateText(s){const c=s.connection||{},state=c.state;return({paused:['Connection stopped','This node is not connecting to EEF. Resume when you are ready. Models and settings are kept.'],checking:['Checking EEF','Checking that the EEF service is reachable before connecting.'],connected:['Connected','This PC is connected to EEF and can provide its enabled services.'],connecting:['Connecting','Signing this node in to EEF.'],retrying:['Reconnecting','EEF is not reachable. We are retrying in the background. Check that EEF is running and the network is available.'],disconnected:['Disconnected','The connection to EEF was lost. Retry or choose another EEF.'],waiting:['Waiting for EEF','Launch EEF on this PC to connect automatically, or add a connection code from another PC.'],starting:['Starting','Preparing this node’s services and selected models.'],error:['Needs attention','Node services could not start. Review the details below, then change settings or retry.']})[state]||['Offline','The app is not reachable. Launch it from the Start menu and try again.'];}
function card(title,body,extra=''){return `<article class="card ${extra}"><h3>${esc(title)}</h3>${body}</article>`;}
function stat(n,label){return `<p class="stat">${esc(n)}</p><p>${esc(label)}</p>`;}
function renderStatus(){
 const s=latest,d=isDevice(),nodes=network.world?.devices||[],connected=nodes.filter(n=>n.connected);
 $('alphaBanner').hidden=!String(s.version||'').includes('-alpha.');
 let title,help;if(d){[title,help]=stateText(s);}else{title=connected.length?(s.models?.length?'Ready':'Connected — choose a model'):'Waiting for nodes';help=connected.length?'EEF uses the nodes and models shown below.':'Install and launch the node app on this PC. It will connect automatically.';}
 live('health').textContent=title;$('health').className='badge'+((d?s.connection?.state==='connected':connected.length)?'':' warn');live('homeTitle').textContent=title;live('homeHelp').textContent=help;live('deviceName').textContent=d?(s.name||config.name||'This PC'):'YOUR EEF NETWORK';live('version').textContent=`${d?'Node app':'EEF'} version ${s.version||'not reported'}`;
 live('homeActions').innerHTML=d?'<a class="button" href="#permissions">Choose permissions</a><a class="button secondary" href="#models">Choose models</a><a href="#network">Connection help</a>':'<a class="button" href="#devices">View nodes</a><a class="button secondary" href="#network">Add a node</a>';
 if(d){const hw=s.hardware||{},enabled=Object.values(s.permissions||{}).flatMap(v=>Object.values(v||{})).filter(v=>v===true).length;
  const inventory=hw.devices||[]; live('hardwareAvailability').innerHTML='<h3>Detected hardware</h3>'+(hw.inventory_available?'<p>'+esc(inventory.length?inventory.map(item=>item.Name).join(' · '):'No camera, audio endpoint, or monitor was reported.')+'</p><small>Detection does not grant permission or prove capture will succeed. Windows privacy settings also apply. Hardware selection currently uses the system default.</small>':'<p>Hardware inventory is not available. Permissions below are preferences, not proof that a camera or microphone is ready.</p>');
  live('homeCards').innerHTML=card('This PC',`<p>${esc(hw.cpu_name||'Detecting processor…')}</p><p>${hw.ram_mb?esc((hw.ram_mb/1024).toFixed(1))+' GB memory':'Memory not reported'}</p><p>${esc(hw.gpu_model||'GPU details not reported; CPU models are available.')}</p>`)+card('Node permissions',stat(enabled,'Applied permission switches')+'<a href="#permissions">Manage permissions</a>')+card('AI models',stat(s.models?.length||0,'Models available to EEF')+'<a href="#models">Install or choose a model</a>');
  live('pauseConnection').textContent=s.connection?.state==='paused'?'Resume connection':'Stop connection';live('connectionTitle').textContent=title;live('connectionHelp').textContent=`${s.coordinator_name||config.coordinator_name||'EEF connection'} — ${help}`;live('connectionDetails').textContent=JSON.stringify(s.connection||{},null,2);live('activity').textContent=s.last_activity?prettyCapability(s.last_activity)+' — most recent request':'No requests yet. Once connected, this PC can help EEF with the services you enable.';
 }else{live('homeCards').innerHTML=card('Connected nodes',stat(connected.length,'Online now'))+card('AI models',stat(s.models?.length||0,'Available across your nodes'))+card('Network',`<p>${nodes.length-connected.length} offline nodes</p><p>${connected.length?'Serving connected nodes':'No nodes connected yet'}</p>`);const tasks=s.world?.active_tasks||[];live('activity').textContent=tasks.length?tasks.slice(-5).map(t=>`${t.description||'Network task'} — ${t.status||'In progress'}`).join('\n'):'No tasks yet.';}
 $('chatPanel').hidden=!d||!!remote;$('restartBanner').hidden=!s.pending_restart;live('diagnostics').textContent=JSON.stringify({version:s.version,status:s,remote_device:remote?.id},null,2);
 if(!d||role==='eef')renderNodes(nodes);else live('devices').innerHTML=card(s.name||'This PC',`<p>${esc(title)}</p><p>Run EEF to see all nodes in your network.</p>`);
 if(s.proposal_pending&&!reviewing){$('proposalBanner').hidden=false;}else $('proposalBanner').hidden=true;
 renderProgress(s.download);
}
function renderNodes(nodes){
 live('devices').innerHTML=nodes.length?nodes.map(node=>card(node.name||'Node',
  `<p><span class="badge ${node.connected?'':'offline'}">${node.connected?'Connected':'Offline'}</span></p>
  <p>${node.connected?'Available to EEF':'Last seen '+(node.last_seen_ms?new Date(node.last_seen_ms).toLocaleString():'before this session')}</p>
  <p>${esc(node.metadata?.area?.join(' / ')||'Area not set')}</p>
  <p>${esc((node.capabilities||[]).map(prettyCapability).filter((v,i,a)=>a.indexOf(v)===i).join(' · '))}</p>
  <button data-manage="${esc(node.node_id)}" ${node.connected?'':'disabled'}>Configure node</button>
  <details><summary>Node details</summary><p>${esc(node.node_id)}</p>${(node.metadata?.resources||[]).map(r=>`<p>${esc(r.name||prettyCapability(r.capability))}: ${r.available?'Offered':'Unavailable'} · ${esc((r.area||[]).join(' / '))}<br><code>${esc(node.node_id+'::'+r.id)}</code></p>`).join('')}</details>`,node.connected?'':'offline')).join(''):
  '<article class="card"><h3>No nodes connected yet</h3><p>Install and launch the node app on this PC or another computer.</p><a class="button" href="#network">Add node</a></article>';
 for(const button of document.querySelectorAll('[data-manage]'))button.onclick=attempt(async()=>{
  if(dirty&&!confirm('Discard your unsaved changes?'))return;
  const node=nodes.find(node=>node.node_id===button.dataset.manage);
  if(!node||!node.connected)throw Error('This node is no longer connected.');
  remote={id:node.node_id,name:node.name};
  await loadConfig();visibility();$('remoteBanner').hidden=false;
  live('remoteName').textContent='Managing '+node.name;location.hash='permissions';await poll();
 });
}
function renderEndpoints(){const list=config.endpoints||[];live('endpoints').innerHTML=list.map((e,i)=>`<div class="endpoint"><label>EEF address<input data-endpoint="${i}" value="${esc(typeof e==='string'?e:e.address)}"></label><label>Priority<input data-priority="${i}" type="number" value="${esc(e.priority||0)}"></label><button data-remove-endpoint="${i}" class="secondary">Remove</button></div>`).join('');document.querySelectorAll('[data-endpoint],[data-priority]').forEach(f=>f.oninput=()=>{const index=Number(f.dataset.endpoint??f.dataset.priority);if(typeof config.endpoints[index]==='string')config.endpoints[index]={address:config.endpoints[index],priority:0};config.endpoints[index][f.dataset.endpoint!==undefined?'address':'priority']=f.dataset.endpoint!==undefined?f.value:Number(f.value);config.auto_local=false;config.local_pairing=false;changed();});document.querySelectorAll('[data-remove-endpoint]').forEach(b=>b.onclick=()=>{config.endpoints.splice(Number(b.dataset.removeEndpoint),1);renderEndpoints();changed();});}
async function loadConfig(){const c=await api('/api/config');config=c.config;saved=clone(config);syncFields();}
async function save(restartAfter=false){for(const field of $('resources').querySelectorAll('input'))if(!field.reportValidity())throw Error('Review the resource settings before saving.');if(!isDevice())for(const field of document.querySelectorAll('[data-number]'))if(!field.reportValidity())throw Error('Review the job storage settings before saving.');const result=await act('/api/config',{config},'PUT');if(result.approval_required){notice('Changes sent. Review and approve them in that PC’s node app.');saved=clone(config);changed();return;}saved=clone(config);changed();latest.pending_restart=true;$('restartBanner').hidden=false;if(reviewing&&!remote){await request('/api/proposal',{method:'DELETE'});reviewing=false;}notice('Changes saved. Restart to apply them.');if(restartAfter||$('autoApply').checked)await restartNow();}
async function restartNow(){if(dirty)throw Error('Save or discard your edits before restarting.');if(!isDevice()&&!confirm('Restart EEF? Connected nodes will reconnect; running tasks may be interrupted.'))return;await act('/api/restart');notice(isDevice()?'Restarting node services. The connection will return automatically.':'Restarting EEF. Nodes will reconnect automatically.');$('restartBanner').hidden=true;}
const gb=bytes=>Number(bytes||0)>=1e9?(Number(bytes)/1e9).toFixed(1)+' GB':Math.round(Number(bytes||0)/1e6)+' MB';
function renderProgress(job){
 if(!job||job.state==='idle'){$('modelProgress').hidden=true;return;}
 $('modelProgress').hidden=false;
 const percent=job.total?Math.min(100,Math.round(100*job.completed/job.total)):0;
 const text=job.state==='installed'?'Installed — choose Use model below':job.state==='error'?job.error:job.state==='cancelled'?job.message:job.cancel_requested?'Cancelling…':(job.phase||'Downloading')+(job.total?' · '+gb(job.completed)+' / '+gb(job.total)+' reported so far':'');
 live('modelProgress').innerHTML=`<strong>${esc(job.name||'Model')}: ${esc(text)}</strong>${job.state==='downloading'?`<progress max="100" ${job.total?'value="'+percent+'"':''} aria-label="Model download progress"></progress><button id="cancelDownload" class="secondary" ${job.cancel_requested?'disabled':''}>Cancel download</button>`:''}`;
 if($('cancelDownload')){const id=job.id;$('cancelDownload').onclick=attempt(()=>act('/api/models/cancel',{id}));}
}
async function loadModels(){if(!isDevice()){live('modelStorage').textContent='';live('modelHelp').textContent='Models run on your nodes. Configure a node to install or choose its models.';live('models').innerHTML=(network.models||[]).map(m=>card(m.model_id||m.name||'Model',`<p>${esc(m.modality==='vlm'?'Text and images':'Text')}</p><p>Node: ${esc((network.world?.devices||[]).find(n=>n.node_id===m.node_id)?.name||'Connected node')}</p><p>Ready on the connected node</p>`)).join('')||card('No AI models available','<p>Open Nodes, choose Configure node, then Models.</p><a href="#devices">Choose a node</a>');return;}
 modelData=await api('/api/models');live('modelHelp').textContent=modelData.backend==='ollama'?'Using your configured Ollama service. Choose which installed models EEF may use.':'Using the included model runtime. Download a model below or choose an existing file.';live('modelStorage').textContent=modelData.storage?(modelData.storage.free_bytes!=null?gb(modelData.storage.free_bytes)+' free. ':'')+modelData.storage.message:'';renderProgress(modelData.download);renderModelCards();renderCatalog();}
function renderModelCards(){const ollama=modelData.backend==='ollama';const selected=ollama?get(config,'models.ollama.selected')||[]:get(config,'models.llamacpp.slots')||[];const items=ollama?(modelData.ollama_models||[]):[...(modelData.installed||[]),...selected.filter(s=>!(modelData.installed||[]).some(m=>m.id===s.model_id)).map(s=>({id:s.model_id,name:s.model_id,path:s.model_path,source:'Your local model file'}))];
 live('models').innerHTML=items.map((m,i)=>{const id=ollama?m.name:m.id,chosen=selected.some(s=>s.model_id===id);return card(m.name||id,`<p>${esc(ollama?'Existing model service':m.source||'Local file')} · ${gb(m.size||m.bytes)}</p>${ollama&&!chosen?`<label>Model type<select data-model-type="${esc(id)}"><option value="auto">Detect from model service</option><option value="text">Text only (owner choice)</option><option value="vlm">Text and images (owner choice)</option></select></label>`:''}<p>${m.available===false?'Model file is missing':chosen?'Selected'+(dirty?' — unsaved':latest.pending_restart?' — restart required':''):'Installed, not selected'}</p><button data-use-model="${esc(id)}" ${chosen||m.available===false?'disabled':''}>${chosen?'Selected':'Use model'}</button>${chosen?` <button class="secondary" data-disable-model="${esc(id)}">Stop using</button>`:''}`);}).join('')||card('No installed models','<p>Choose a model below to get started. Downloads happen only when you request them.</p>');
 for(const b of document.querySelectorAll('[data-use-model]'))b.onclick=attempt(async()=>{const m=items.find(m=>(ollama?m.name:m.id)===b.dataset.useModel);if(!m)throw Error("This model is no longer available. Refresh models.");if(ollama){const choice=[...document.querySelectorAll('[data-model-type]')].find(f=>f.dataset.modelType===m.name)?.value||'auto';const selectionConfig=config;const details=await act('/api/models/inspect',{id:m.name});if(config!==selectionConfig)throw Error('Node settings changed while inspecting this model. Select it again.');if(details.capabilities_known&&!details.modality)throw Error('This model does not report text or image generation support. Embedding-only models are not supported here.');if(choice==='vlm'&&details.capabilities_known&&details.modality!=='vlm')throw Error('This model does not report image support.');const modality=choice==='auto'?details.modality:choice;if(!modality)throw Error('The service did not report capabilities. Choose the model type on this card before selecting it.');const list=get(config,'models.ollama.selected')||[];if(list.some(s=>s.model_id===m.name))return;put(config,'models.ollama.selected',[...list,{model_id:m.name,modality}]);}else selectLocal(m.id,m.path);changed();renderModelCards();notice('Model selected. Apply and restart to make it available to EEF.');});
 for(const b of document.querySelectorAll('[data-disable-model]'))b.onclick=()=>{const path=ollama?'models.ollama.selected':'models.llamacpp.slots';put(config,path,(get(config,path)||[]).filter(m=>m.model_id!==b.dataset.disableModel));changed();renderModelCards();};
}
function selectLocal(name,path){if(!name.trim()||!path.trim())throw Error('Choose a model file and give it a name.');const slots=get(config,'models.llamacpp.slots')||[];if(slots.some(s=>s.model_id===name))throw Error('That model is already selected.');let port=8082;while(slots.some(s=>s.port===port))port++;put(config,'models.provider','llamacpp');put(config,'models.llamacpp.slots',[...slots,{model_id:name,model_path:path,port,context:2048,gpu_layers:0}]);syncFields();}
function renderCatalog(){const search=$('modelSearch').value.toLowerCase();live('catalog').innerHTML=(modelData.catalog||[]).filter(m=>`${m.name} ${m.description}`.toLowerCase().includes(search)).map(m=>{const ram=Number(latest.hardware?.ram_mb||0)/1024,low=ram>0&&ram<m.recommended_ram_gb;return card(m.name,`<p>${esc(m.description)}</p><p>${gb(m.bytes)} download · about ${esc(m.recommended_ram_gb||'?')} GB RAM recommended</p><p>${esc(m.source)} · ${esc(m.license||'See model source')}</p>${low?'<p class="risk">This PC has less memory than recommended. Choose a smaller model if it does not start.</p>':''}<button data-install="${esc(m.id)}">Install model</button>`);}).join('');document.querySelectorAll('[data-install]').forEach(b=>b.onclick=attempt(async()=>{await act('/api/models/install',{id:b.dataset.install});notice('Download started. This does not select the model automatically.');await loadModels();}));}
async function poll(){if(polling)return;polling=true;try{if(role==='eef')network=await request('/api/status');latest=remote||role==='device'?await api('/api/status'):network;renderStatus();}catch(e){live('health').textContent='Offline';$('health').className='badge offline';live('homeTitle').textContent='App is not reachable';live('homeHelp').textContent='If the app is restarting, this page will reconnect automatically. Otherwise, launch it from the Start menu.';}finally{polling=false;}}
let jobsLoading=false, selectedJob=null;
const jobLabel=s=>({pending:'Queued',running:'Running',pausing:'Pausing after current steps',paused:'Paused',stopping:'Stopping after current steps',cancelled:'Stopped',interrupted:'Interrupted — review needed',completed:'Completed',failed:'Failed',skipped:'Skipped'})[s]||s;
const jobFinished=j=>['completed','failed','cancelled'].includes(j.status);
const jobBadge=j=>j.status==='completed'?'badge':['failed','interrupted'].includes(j.status)?'badge offline':'badge warn';
function jobButtons(j){
 const button=(action,label)=>`<button class="secondary" data-job-id="${esc(j.id)}" data-job-action="${action}">${label}</button>`;
 return '<div class="actions">'+button('view','View steps')+
 (['pending','running'].includes(j.status)?button('pause','Pause'):'')+
 (j.can_resume?button('resume','Resume'):'')+
 (!jobFinished(j)?button('stop','Stop'):button('remove','Remove history'))+'</div>';
}
async function loadJobs(){
 if(jobsLoading)return;
 if(remote){live('jobState').textContent='Leave remote configuration to view jobs in this app.';live('jobStorage').textContent='';live('jobList').innerHTML='';$('jobDetailsPanel').hidden=true;return;}
 jobsLoading=true;
 try{
  const data=await request('/api/jobs');if(remote)return;
  const jobs=data.jobs||[];
  const storage=data.storage;
  live('jobStorage').textContent=storage?`${storage.count} of ${storage.max_count} network history slots used. ${(storage.payload_bytes/1048576).toFixed(2)} MiB saved + ${(storage.reserved_bytes/1048576).toFixed(2)} MiB reserved for recovery, within a ${(storage.max_bytes/1048576).toFixed(2)} MiB job budget. This is not the disk’s total usage.${storage.over_limit?' Above the configured budget: remove finished history or increase the limits in EEF Settings.':''}`:'';
  live('jobState').textContent=(role==='device'?'Jobs submitted through this node. ':'Network job history. ')+(jobs.length?`Showing ${jobs.length} of ${data.total??jobs.length}.`:'No saved jobs yet. Planned actions submitted from Home will appear here.');
  live('jobList').innerHTML=jobs.map(j=>card(j.description||'Network job',`<p class="${jobBadge(j)}">${esc(jobLabel(j.status))}</p><p>${j.completed_tasks} of ${j.task_count} steps completed</p><p class="muted">${esc(j.request_context?.origin_area?.join(' / ')||'No area set')} · ${esc(new Date(j.updated_at_ms).toLocaleString())}</p>${j.status==='interrupted'&&!j.can_resume?'<p class="risk">An action may already have happened. Review its steps before creating new work; this job cannot safely resume.</p>':''}${jobButtons(j)}`)).join('');
  if(selectedJob){
   const id=selectedJob, detail=await request('/api/jobs/'+encodeURIComponent(id));if(remote||selectedJob!==id)return;
   $('jobDetailsPanel').hidden=false;
   live('jobDetails').innerHTML=`<p>${esc(detail.description)} — ${esc(jobLabel(detail.status))}</p><p class="muted">Job ID: <code>${esc(detail.id)}</code></p>`+
    (detail.tasks||[]).map(t=>`<section class="panel"><h3>${esc(prettyCapability(t.capability))} · ${esc(t.action)}</h3><p>${esc(jobLabel(t.status))} · ${t.attempts} attempt(s)</p>${t.assignment?`<p>Assigned node: <code>${esc(t.assignment.node_id)}</code>${t.assignment.resource_id?'<br>Resource: '+esc(t.assignment.resource_id):''}${t.assignment.model_id?'<br>Model: '+esc(t.assignment.model_id):''}</p>`:'<p>Not assigned to a node yet.</p>'}${t.error?'<p class="risk">'+esc(t.error)+'</p>':''}</section>`).join('')+
    '<details><summary>Lifecycle history</summary><ol>'+(detail.history||[]).map(e=>`<li>${esc(new Date(e.at_ms).toLocaleString())} — ${esc(e.message)}</li>`).join('')+'</ol></details>';
  }
 }catch(e){live('jobState').textContent='Job history could not refresh: '+e.message;}
 finally{jobsLoading=false;}
}
async function init(){const ui=await request('/api/ui');role=ui.role;buildPermissions();wireFields();live('nav').innerHTML=['Home','Nodes','Jobs','Models','Permissions','Network','Settings','Advanced'].map(s=>`<a href="#${s==='Nodes'?'devices':s.toLowerCase()}"><span class="nav-icon" aria-hidden="true">${({Home:"▤",Nodes:"⬡",Permissions:"☑",Settings:"⚙"})[s]||"◇"}</span>${s}</a>`).join('');
 $('jobState').insertAdjacentHTML('afterend','<p id="jobStorage" class="muted"></p>');
 document.querySelector('[data-page=settings]').insertAdjacentHTML('afterbegin','<section class="panel eef-only"><h2>Saved job storage</h2><label>Maximum saved jobs<input data-field="jobs.max_count" data-number="true" type="number" min="1" max="1000000" step="1" required></label><label>Job storage budget (MiB)<input data-field="jobs.max_bytes" data-number="true" data-scale="1048576" type="number" min="0.0009765625" max="1048576" step="any" required></label><p>Includes recovery headroom, but not SQLite overhead or other app data. Existing history is kept if you lower the limits. New work needs room and is not queued automatically. Remove finished history on Jobs to free its budget. Save and restart to apply.</p></section>');
 $('refreshJobs').onclick=loadJobs;
 $('jobList').onclick=attempt(async event=>{
  const button=event.target.closest('[data-job-action]');if(!button||remote)return;
  const {jobId:id,jobAction:action}=button.dataset;
  if(action==='view'){selectedJob=id;await loadJobs();return;}
  const confirmation={resume:'Resume unfinished safe steps? Completed steps will not run again.',stop:'Stop later steps? Actions already sent may still finish and are not undone.',remove:'Remove this finished job’s saved history? This cannot be undone.'}[action];
  if(confirmation&&!confirm(confirmation))return;
  button.disabled=true;
  try{
   if(action==='remove'){await request('/api/jobs/'+encodeURIComponent(id),{method:'DELETE'});if(selectedJob===id){selectedJob=null;$('jobDetailsPanel').hidden=true;}}
   else await send('/api/jobs/'+encodeURIComponent(id)+'/'+action);
   notice(action==='remove'?'Finished job history removed. It cannot be recovered in this app.':'Job control saved. Already-running actions may still finish.');
   await loadJobs();
  }finally{button.disabled=false;}
 });
 setInterval(()=>{if(location.hash==='#jobs')loadJobs();},6000);
 $('content').insertAdjacentHTML('afterbegin','<div id="remoteBanner" class="banner" hidden><strong id="remoteName"></strong><button id="backToEEF" class="secondary">Back to EEF</button></div><div id="proposalBanner" class="banner" hidden><strong>EEF has requested changes to this node.</strong><button id="reviewProposal">Review changes</button><button id="rejectProposal" class="secondary">Reject</button></div>');
 $('backToEEF').onclick=attempt(async()=>{if(dirty&&!confirm('Discard unsaved edits?'))return;remote=null;$('remoteBanner').hidden=true;await loadConfig();visibility();await poll();location.hash='devices';});
 $('reviewProposal').onclick=attempt(async()=>{if(dirty&&!confirm('Replace unsaved edits with the proposed settings?'))return;config=(await request('/api/proposal')).config;reviewing=true;syncFields();location.hash='permissions';notice('Review the proposed permissions, models, and settings. Save only if you approve.');});$('rejectProposal').onclick=attempt(async()=>{await request('/api/proposal',{method:'DELETE'});reviewing=false;await poll();});
 const settings=document.querySelector('[data-page=settings]');settings.insertAdjacentHTML('afterbegin','<section class="panel device-only"><h2>Management from EEF</h2><label class="toggle"><input type="checkbox" data-field="management.allow_remote" id="remoteAllowed"> Allow EEF to apply settings and restart this node</label><p class="risk">Enable only for a trusted EEF network. Otherwise, changes requested from EEF need your approval here. This permission can only be granted in the local node app.</p></section>');wireFields();
 await loadConfig();visibility();for(const a of document.querySelectorAll('.peer'))a.href=ui.peer_url||'#devices';
 $('autoApply').checked=localStorage.getItem('eef.autoApply')==='true';$('autoApply').onchange=()=>localStorage.setItem('eef.autoApply',String($('autoApply').checked));
 const startup=await request('/api/startup').catch(()=>({supported:false}));$('startup').checked=startup.enabled;$('startup').disabled=!startup.supported;live('startupLabel').textContent=`Start ${role==='device'?'the node app':'EEF'} automatically when I sign in`;
 $('startup').onchange=attempt(async()=>{if(remote)throw Error('Change startup in the local node app.');try{await send('/api/startup',{enabled:$('startup').checked},'PUT');notice('Startup preference saved.');}catch(e){$('startup').checked=!$('startup').checked;throw e;}});
 $('save').onclick=attempt(()=>save());$('saveRestart').onclick=attempt(()=>save(true));$('discard').onclick=()=>{config=clone(saved);syncFields();};$('refresh').onclick=attempt(poll);$('refreshModels').onclick=attempt(loadModels);$('modelSearch').oninput=renderCatalog;
 document.querySelectorAll('[data-action=restart]').forEach(b=>b.onclick=attempt(restartNow));$('later').onclick=()=>{$('restartBanner').hidden=true;notice('Saved settings will apply after the next restart.');};
 $('pauseConnection').onclick=attempt(async()=>{if(dirty)throw Error('Save or discard edits before changing the connection.');await act('/api/network/pause',{paused:latest.connection?.state!=='paused'});await loadConfig();await poll();});
 $('useLocal').onclick=attempt(async()=>{if(dirty&&!confirm('Discard unsaved changes and connect to local EEF?'))return;await act('/api/network/local');await loadConfig();notice('Looking for EEF on this PC. Launch EEF if it is not running.');});
 $('createInvite').onclick=attempt(async()=>{const v=await send('/api/network/invite');$('inviteLabel').hidden=false;$('inviteOutput').value=v.code;$('inviteOutput').select();notice('Keep this connection code private. Paste it in the other PC’s node app.');});
 $('join').onclick=attempt(async()=>{let v;try{v=JSON.parse(new TextDecoder().decode(Uint8Array.from(atob($('inviteInput').value.trim()),c=>c.charCodeAt(0))));}catch{throw Error('This connection code is not valid. Copy it again from EEF.');}if(typeof v.address!=='string'||typeof v.psk!=='string'||v.psk.length<12)throw Error('This connection code is incomplete.');config.endpoints=[{address:v.address,priority:100}];config.psk=v.psk;config.coordinator_name=v.name||'EEF';config.auto_local=false;config.local_pairing=false;syncFields();await save(true);$('inviteInput').value='';});
 $('addEndpoint').onclick=()=>{config.endpoints??=[];config.endpoints.push({address:'',priority:0});renderEndpoints();changed();};
 $('installCustom').onclick=attempt(async()=>{await act('/api/models/install',{id:$('customModel').value.trim()});await loadModels();});$('pickModel').onclick=attempt(async()=>{$('modelPath').value=(await act('/api/pick',{kind:'model'})).path;});$('addLocalModel').onclick=attempt(async()=>{selectLocal($('localModelName').value,$('modelPath').value);changed();notice('Model selected. Apply and restart when ready.');});
 $('applyRaw').onclick=attempt(async()=>{const value=JSON.parse($('rawConfig').value);if(!value||Array.isArray(value)||typeof value!=='object')throw Error('Configuration must be an object.');config=value;syncFields();notice('Edits loaded. Save to validate and apply them.');});
 $('restore').onclick=attempt(async()=>{if(confirm('Restore the previous saved configuration? Unsaved edits will be discarded.')){await act('/api/config/restore');await loadConfig();latest.pending_restart=true;$('restartBanner').hidden=false;}});
 $('reset').onclick=attempt(async()=>{if(confirm('Reset this node’s settings? Permissions will be disabled. Your node ID and downloaded models stay intact.')){await act('/api/config/reset');await loadConfig();latest.pending_restart=true;$('restartBanner').hidden=false;}});
 $('checkUpdate').onclick=attempt(async()=>{live('updateStatus').textContent='Checking for updates…';const u=await act('/api/update/check');live('updateStatus').textContent=u.update_available?'Version '+u.latest_version+' is available.':'You are up to date.';});$('applyUpdate').onclick=attempt(async()=>{if(dirty)throw Error('Save or discard your edits before installing an update.');const check=await act('/api/update/check');if(!check.update_available){live('updateStatus').textContent='You are up to date.';return;}if(confirm('Download and install version '+check.latest_version+'?')){$('applyUpdate').disabled=true;try{live('updateStatus').textContent='Downloading and verifying the update…';await act('/api/update/apply');latest.pending_restart=true;$('restartBanner').hidden=false;live('updateStatus').textContent='Update installed. Restart now to use it.';notice('Update installed. Use Restart now to apply it.');}finally{$('applyUpdate').disabled=false;}}});
 let chatController=null;
 $('chatForm').onsubmit=attempt(async()=>{
  if(chatController)return;
  if(latest.connection?.state!=='connected')throw Error('Connect this node to EEF before sending a message.');
  const controller=new AbortController();chatController=controller;
  $('sendMessage').disabled=true;$('stopWaiting').hidden=false;
  live('reply').textContent='Waiting for your network…';
  try{
   const value=await request('/api/chat',{method:'POST',body:JSON.stringify({message:$('message').value}),signal:controller.signal});
   live('reply').textContent=value.reply||'Request completed.';
  }catch(error){
   if(controller.signal.aborted)live('reply').textContent='Stopped waiting. Work may still run on the network. Nothing was automatically sent again.';
   else{live('reply').textContent=error.message;throw error;}
  }finally{chatController=null;$('sendMessage').disabled=false;$('stopWaiting').hidden=true;}
 });
 $('stopWaiting').onclick=()=>chatController?.abort();
 $('exportDiagnostics').onclick=()=>{const a=document.createElement('a');a.href=URL.createObjectURL(new Blob([JSON.stringify({version:latest.version,connection:latest.connection,hardware:latest.hardware,capabilities:latest.capabilities},null,2)],{type:'application/json'}));a.download='eef-diagnostics.json';a.click();URL.revokeObjectURL(a.href);};
 addEventListener('beforeunload',e=>{if(dirty){e.preventDefault();e.returnValue='';}});addEventListener('hashchange',page);page();await poll();setInterval(poll,3000);setInterval(()=>{if(location.hash==='#models')loadModels().catch(()=>{});},6000);
}
init().catch(e=>notice(e.message,true));
