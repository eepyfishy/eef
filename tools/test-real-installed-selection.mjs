// Opt-in integrity/selection test against an existing development cache only.
// Adds a uniquely named test config beside that cache so no weights are copied.
// Does not alter pre-existing configs/index, launch a runtime or infer/execute.
import assert from 'node:assert/strict';
import {readFile,writeFile,realpath,mkdtemp,stat} from 'node:fs/promises';
import {resolve,join,dirname,sep} from 'node:path';
import {spawn} from 'node:child_process';
import {randomUUID,createHash} from 'node:crypto';
const root=resolve(import.meta.dirname,'..'),bin=resolve(process.env.EEF_TEST_BINARY_DIR||join(root,'target/debug'));
assert(process.argv[2]&&process.argv[3],'Pass an existing ignored development models directory and catalog ID explicitly');
const validation=await realpath(join(root,'.validation')),directory=await realpath(process.argv[2]),id=process.argv[3];
assert(directory.startsWith(validation+sep)&&directory.endsWith(sep+'models'),'Use only an existing controlled development cache');
const indexPath=join(directory,'installed.json'),indexBefore=await readFile(indexPath);
const index=JSON.parse(indexBefore),catalog=JSON.parse(await readFile(join(root,'config/model-catalog.json'),'utf8'));
assert.equal(index.filter(m=>m.id===id).length,1);assert.equal(catalog.filter(m=>m.id===id).length,1);
const entry=catalog.find(m=>m.id===id),artifact=join(directory,entry.sha256.toLowerCase()+'.gguf');
assert.equal((await stat(artifact)).size,entry.bytes);
const scratch=await mkdtemp(join(validation,'real-installed-selection-'));
const configPath=join(dirname(directory),'PRIVATE-installed-selection-'+randomUUID()+'.json');
const config=JSON.parse(await readFile(join(root,'config/node.example.json'),'utf8'));
Object.assign(config,{node_id:'real-installed-selection-'+randomUUID(),name:'Real installed selection fixture',
 auto_local:false,local_pairing:false,endpoints:[],connection_enabled:false,update:{policy:'off'},
 dashboard:{...config.dashboard,enabled:false,ui_enabled:false}});
await writeFile(configPath,JSON.stringify(config),{flag:'wx'});
const env={...process.env,EEF_NODE_PSK:'',APPDATA:join(scratch,'appdata'),EEF_DISCOVERY_DIR:join(scratch,'discovery'),
 PATH:join(root,'.tooling/llvm-mingw-20260616-ucrt-x86_64/bin')+';'+process.env.PATH};
let child;
try{
 const started=performance.now();child=spawn(join(bin,'eefn.exe'),['--config',configPath,'--json','models','select-installed-gguf','--model',id,'--interpreter'],
  {cwd:root,windowsHide:true,env,stdio:['ignore','pipe','pipe']});
 let out='',err='';child.stdout.on('data',b=>out=(out+b).slice(-1048576));child.stderr.on('data',b=>err=(err+b).slice(-1048576));
 const timer=setTimeout(()=>child.kill(),85000);
 let code;try{code=await new Promise((r,j)=>{child.on('error',j);child.on('close',r);});}finally{clearTimeout(timer);}
 assert.equal(code,0,out+err);const selected=JSON.parse(out);
 assert.equal(selected.running,false);assert.equal(selected.changed,true);
 assert.deepEqual(selected.installed_artifact,{model_id:id,sha256:entry.sha256.toLowerCase(),bytes:entry.bytes,verified_at_selection:true});
 assert.equal(selected.saved_selections.length,1);assert.equal(selected.saved_selections[0].model_id,id);
 assert(selected.saved_selections[0].model_metadata.roles.includes('request_interpreter'));
 assert.equal(selected.registered_models,null);
 const saved=JSON.parse(await readFile(configPath,'utf8'));
 assert.equal(saved.models.provider,config.models.provider);assert.deepEqual(saved.permissions,config.permissions);
 assert.deepEqual(await readFile(indexPath),indexBefore,'selection must not rewrite the existing installed index');
 assert.equal((await stat(artifact)).size,entry.bytes);
 const elapsed=Math.round(performance.now()-started);
 await writeFile(join(scratch,'results.json'),JSON.stringify({passed:true,model:id,bytes:entry.bytes,sha256:entry.sha256,
  real_cached_artifact:true,selection_elapsed_ms:elapsed,offline_no_runtime:true,explicit_interpreter_assignment:true,
  preexisting_index_unchanged:true,model_downloads:false,model_copies:false,real_model_loading:false,model_quality_approved:false,
  physical_two_pc:false,isolated_config_sha256:createHash('sha256').update(await readFile(configPath)).digest('hex')},null,2));
 console.log('Real installed selection passed in '+elapsed+' ms: '+scratch);
}catch(error){await writeFile(join(scratch,'PRIVATE-failure.json'),JSON.stringify({error:String(error),configPath},null,2));throw error;}
finally{if(child&&child.exitCode===null)child.kill();assert.deepEqual(await readFile(indexPath),indexBefore);}
