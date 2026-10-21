// Selected bundle resolution with real Python metadata inspection and refusing
// executable sentinels. No real media, models, inference or installed app edits.
import assert from 'node:assert/strict';
import {mkdtemp,mkdir,readFile,writeFile,link,unlink,stat} from 'node:fs/promises';
import {join,resolve,relative,sep,dirname} from 'node:path';
import {spawn} from 'node:child_process';
import {createServer} from 'node:net';
assert.equal(process.platform,'win32','Native bundle-layout acceptance is Windows-only');
const root=resolve(import.meta.dirname,'..'),bin=resolve(process.env.EEF_TEST_BINARY_DIR||join(root,'target/debug')),python=process.env.EEF_TEST_PYTHON;
assert(python,'Supply an existing trusted interpreter; no downloads');await stat(python);
const scratch=await mkdtemp(join(root,'.validation/bundled-components-')),children=[],links=[];
const version=(await readFile(join(root,'Cargo.toml'),'utf8')).match(/^version\s*=\s*"([^"]+)"/m)[1];
const baseEnv={...process.env,EEF_NODE_PSK:'',APPDATA:join(scratch,'appdata'),EEF_DISCOVERY_DIR:join(scratch,'discovery'),PATH:join(root,'.tooling/llvm-mingw-20260616-ucrt-x86_64/bin')+';'+process.env.PATH};
for(const key of Object.keys(baseEnv))if(['EEF_PYTHON','EEF_NODE_OLLAMA'].includes(key.toUpperCase()))delete baseEnv[key];
function owned(path){assert(resolve(path).startsWith(resolve(scratch)+sep),'Mutation outside new fixture');return path;}
async function alias(source,target){await mkdir(dirname(owned(target)),{recursive:true});await link(source,target);links.push(target);}
async function exists(path){try{return (await stat(path)).isFile();}catch(error){if(error.code==='ENOENT')return false;throw error;}}
function start(exe,args,extra={}){const c=spawn(exe,args,{cwd:root,windowsHide:true,env:{...baseEnv,...extra},stdio:['ignore','pipe','pipe']});let out='',err='';c.stdout.on('data',b=>out=(out+b).slice(-1048576));c.stderr.on('data',b=>err=(err+b).slice(-1048576));c.logs=()=>({out,err});c.result=new Promise((r,j)=>{c.on('error',j);c.on('close',code=>r({code,out,err}));});c.result.catch(()=>{});children.push(c);return c;}
async function until(fn,label){const end=Date.now()+30000;while(Date.now()<end){try{if(await fn())return;}catch{}await new Promise(r=>setTimeout(r,100));}throw Error('Timeout: '+label);}
async function json(url){const r=await fetch(url,{signal:AbortSignal.timeout(10000)});assert(r.ok,'Owner read failed; private evidence '+scratch);return r.json();}
async function setupText(f){const c=start(join(bin,'eefn.exe'),['--config',f.config,'setup']);const timer=setTimeout(()=>c.kill(),30000);try{const result=await c.result;assert.equal(result.code,0,'Setup check failed; private evidence '+scratch);return result.out;}finally{clearTimeout(timer);}}
async function port(){const s=createServer();await new Promise(r=>s.listen(0,'127.0.0.1',r));const p=s.address().port;await new Promise(r=>s.close(r));return p;}
async function plugin(directory,marker,label){await mkdir(owned(directory),{recursive:true});await writeFile(owned(join(directory,'audio_output.py')),`from pathlib import Path\nPath(${JSON.stringify(marker)}).write_text(${JSON.stringify(label)}, encoding='utf-8')\nCAPABILITY='audio.play'\nACTIONS=['play','stop']\nPARAMS_SCHEMA={}\ndef handle(action, params):\n    raise RuntimeError('Synthetic metadata only; media invocation forbidden')\n`);}
async function fixture(name,{versioned=true,missingPlugin=false,missingPython=false,relativePython=false,llama=false,custom=false}={}){
 const install=owned(join(scratch,name)),bundle=versioned?join(install,'versions',version):install;await mkdir(bundle,{recursive:true});
 const exe=join(bundle,'eefn.exe');await alias(join(bin,'eefn.exe'),exe);
 await writeFile(owned(join(bundle,'bundle.json')),JSON.stringify({name:'eefn',version}));
 const oldMarker=join(install,'old-plugin.marker'),currentMarker=join(install,'current-plugin.marker'),pathMarker=join(install,'path-python.marker');
 if(versioned)await plugin(join(install,'python/plugins'),oldMarker,'old root must not run');
 if(!missingPlugin)await plugin(join(bundle,'python/plugins'),currentMarker,'current bundle');
 const aliasDir=join(install,'path-bin');await alias(join(bin,'examples/component-fixture.exe'),join(aliasDir,'python.exe'));
 const config=join(install,'node.json'),api=await port(),url=`http://127.0.0.1:${api}`;
 const value=JSON.parse(await readFile(join(root,'config/node.example.json'),'utf8'));
 Object.assign(value,{node_id:'bundle-'+name,name:'Bundle fixture',auto_local:false,local_pairing:false,connection_enabled:false,endpoints:[],psk:'',dashboard:{enabled:true,ui_enabled:false,host:'127.0.0.1',port:api},update:{policy:'off'},models:{provider:'ollama',ollama:{base_url:'http://127.0.0.1:1',selected:[]},llamacpp:{binary:'tools/llama-server.exe',slots:[]}}});
 value.permissions.media.audio_output=!llama;
 if(relativePython)value.python=relative(install,python);
 if(llama){
  await alias(join(bin,'examples/component-fixture.exe'),join(bundle,'tools/llama-server.exe'));
  if(versioned)await alias(join(bin,'examples/component-fixture.exe'),join(install,'tools/llama-server.exe'));
  const model=join(install,'synthetic.gguf');await writeFile(owned(model),'GGUF synthetic placeholder; never valid inference weights');
  value.models.provider='llamacpp';value.models.llamacpp.slots=[{model_id:'fixture',model_path:model,port:await port(),context:1024,gpu_layers:0}];
  value.model_lifecycle={...value.model_lifecycle,startup:'eager'};
  if(custom)value.models.llamacpp.binary=join(install,'tools/llama-server.exe');
 }
 await writeFile(owned(config),JSON.stringify(value));
 const env={PATH:aliasDir+';'+join(root,'.tooling/llvm-mingw-20260616-ucrt-x86_64/bin')+';'+process.env.PATH,EEF_TEST_PYTHON_MARKER:pathMarker};
 const child=start(exe,['--config',config,'--no-ui',...(!missingPython&&!relativePython?['--python',python]:[])],env);
 await until(async()=>(await json(url+'/api/diagnostics')).runtime_id,'node runtime '+name);
 return {install,bundle,config,url,oldMarker,currentMarker,pathMarker,process:child,status:()=>json(url+'/api/status'),stop:async()=>{if(child.exitCode===null)child.kill();await child.result;}};
}
try{
 const current=await fixture('current');
 assert(await exists(current.currentMarker),'Selected version must inspect its own bundled plugin, not the old installation root');
 assert(!(await exists(current.oldMarker)),'Old root plugin must not execute');assert.equal((await current.status()).python_bridge.enabled,true);await current.stop();
 const missing=await fixture('missing-plugin',{missingPlugin:true});
 assert((await missing.status()).startup_issues.includes('builtin_plugin_unavailable'));assert(!(await exists(missing.oldMarker)),'Missing selected plugin must not borrow old root or CWD');
 assert((await setupText(missing)).includes('Repair this node package'));await missing.stop();
 const flat=await fixture('flat',{versioned:false});assert(await exists(flat.currentMarker));await flat.stop();
 const configured=await fixture('configured-python',{relativePython:true});assert(await exists(configured.currentMarker),'Configured Python is relative to its config directory');await configured.stop();
 for(const versioned of [false,true]){
  const absent=await fixture('missing-python-'+versioned,{versioned,missingPython:true});
  assert((await absent.status()).startup_issues.includes('python_runtime_unavailable'));
  assert(!(await exists(absent.pathMarker)),'Managed bundle must not launch an implicit PATH Python');
  assert((await setupText(absent)).includes('Repair this node package'));await absent.stop();
 }
 const model=await fixture('bundled-llama',{llama:true});
 await until(()=>exists(join(model.bundle,'tools/fixture-started.marker')),'selected bundled model executable');
 assert(!(await exists(join(model.install,'tools/fixture-started.marker'))),'Old root model executable must not run');await model.stop();
 const explicit=await fixture('explicit-llama',{llama:true,custom:true});
 await until(()=>exists(join(explicit.install,'tools/fixture-started.marker')),'explicit owner model executable');
 assert(!(await exists(join(explicit.bundle,'tools/fixture-started.marker'))),'Absolute owner override remains explicit');await explicit.stop();
 await writeFile(owned(join(scratch,'results.json')),JSON.stringify({passed:true,physical_two_pc:false,synthetic_plugins_and_executable_sentinels:true,real_python_metadata_inspection:true,real_model_inference:false,media_invocation:false,
  selected_bundle_plugins:true,no_old_root_or_cwd_plugin_fallback:true,flat_bundle_preserved:true,configured_python_is_config_relative:true,no_implicit_path_python_for_managed_bundles:true,
  selected_bundle_model_binary:true,absolute_owner_binary_preserved:true,read_only_package_repair_guidance:true},null,2));
 console.log('Bundled component resolution validation passed: '+scratch);
}catch(error){await writeFile(owned(join(scratch,'PRIVATE-failure.json')),JSON.stringify({message:String(error)},null,2));await writeFile(owned(join(scratch,'PRIVATE-logs.json')),JSON.stringify(children.map(c=>({exe:c.spawnfile,code:c.exitCode,...c.logs()})),null,2));throw error;}
finally{for(const c of children)if(c.exitCode===null)c.kill();await Promise.allSettled(children.map(c=>c.result));for(const path of links)await unlink(owned(path));}
