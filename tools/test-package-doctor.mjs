// Isolated owner commands; real preexisting stock runtimes, synthetic fault helper.
// No model, installed-node, permission, OS-trust or automatic-update changes.
import assert from 'node:assert/strict';
import {mkdtemp,mkdir,link,unlink,readdir,readFile,writeFile,rename,symlink,rmdir} from 'node:fs/promises';
import {resolve,join,dirname,sep} from 'node:path';
import {spawn} from 'node:child_process';
import {randomUUID} from 'node:crypto';
assert.equal(process.platform,'win32');
const root=resolve(import.meta.dirname,'..'),bin=resolve(process.env.EEF_TEST_BINARY_DIR||join(root,'target/debug'));
const bundle=resolve(process.env.EEF_TEST_DOCTOR_BUNDLE||'' );
assert(process.env.EEF_TEST_DOCTOR_BUNDLE,'Supply an existing trusted Windows node bundle with Python and tools; no downloads');
const scratch=await mkdtemp(join(root,'.validation/package-doctor-')),children=[],links=[];
const env={...process.env,EEF_NODE_PSK:'',APPDATA:join(scratch,'appdata'),EEF_DISCOVERY_DIR:join(scratch,'discovery'),
 EEF_DOCTOR_PRIVATE_ENV_SENTINEL:'PRIVATE_PROBE_CREDENTIAL',
 PATH:join(root,'.tooling/llvm-mingw-20260616-ucrt-x86_64/bin')+';'+join(bundle,'python')+';'+process.env.PATH,
 PYTHONHOME:join(bundle,'python'),PYTHONPATH:join(bundle,'python')};
function owned(path){assert(resolve(path).startsWith(scratch+sep));return path;}
async function alias(source,target){await mkdir(dirname(owned(target)),{recursive:true});await link(source,target);links.push(target);}
async function remove(path){try{await unlink(owned(path));}catch(e){if(e.code!=='ENOENT')throw e;}}
async function flat(source,target,match){for(const entry of await readdir(source,{withFileTypes:true}))if(entry.isFile()&&match(entry.name))await alias(join(source,entry.name),join(target,entry.name));}
function launch(exe,args){const child=spawn(exe,args,{cwd:scratch,env,windowsHide:true,stdio:['ignore','pipe','pipe']});let out='',err='';
 child.stdout.on('data',b=>out=(out+b).slice(-1048576));child.stderr.on('data',b=>err=(err+b).slice(-1048576));
 child.logs=()=>({out,err});child.result=new Promise((r,j)=>{child.on('error',j);child.on('close',code=>r({code,out,err}));});children.push(child);return child;}
async function run(exe,args,success=true){const child=launch(exe,args),timer=setTimeout(()=>child.kill(),45000);
 try{const r=await child.result;assert.equal(r.code,success?0:1,r.out+r.err);assert(!r.out.includes('PRIVATE_PROBE_TRACE')&&!r.err.includes('PRIVATE_PROBE_TRACE'));return r;}finally{clearTimeout(timer);}}
async function doctor(exe,config,args=[],success=true){return JSON.parse((await run(exe,['--config',config,'--json','doctor',...args],success)).out);}
async function retired(path){const pids=JSON.parse(await readFile(path,'utf8'));assert(Number.isInteger(pids.root)&&Number.isInteger(pids.child));
 const deadline=Date.now()+4000;while(true){let live=0;for(const pid of [pids.root,pids.child]){try{process.kill(pid,0);live++;}catch(e){if(e.code!=='ESRCH')throw e;}}
  if(!live)return;assert(Date.now()<deadline,'Owned fake root/descendant still alive');await new Promise(r=>setTimeout(r,75));}}
try{
 for(const role of ['eef','eefn']){
  const dir=join(scratch,role),exe=join(dir,role+'.exe'),config=join(dir,'config.json'),python=join(dir,'python/python.exe');
  await alias(join(bin,role+'.exe'),exe);const sentinel=Buffer.from('{ owner settings sentinel: invalid JSON and YAML');await writeFile(config,sentinel);
  const version=(await run(exe,['--version'])).out.trim().split(/\s+/).at(-1);
  await writeFile(join(dir,'bundle.json'),JSON.stringify({name:role,version,architecture:'x86_64-windows',python:'3.11.9-embedded',...(role==='eefn'?{llamacpp:'b10621'}:{})}));
  await flat(join(bundle,'python'),join(dir,'python'),name=>name==='python.exe'||/\.(dll|pyd|zip|_pth)$/i.test(name));
  if(role==='eefn')await flat(join(bundle,'tools'),join(dir,'tools'),name=>name==='llama-server.exe'||/\.dll$/i.test(name));
  const before=await readdir(dir),metadata=await doctor(exe,config);assert.equal(metadata.probes_requested,false);assert.equal(metadata.probe_identity_unchanged,null);
  assert.equal(metadata.all_requested_probes_passed,null);assert.deepEqual(await readdir(dir),before);
  const human=(await run(exe,['--config',config,'doctor'])).out;assert(human.includes('No runtime started'));assert(human.includes('not system readiness'));
  const actual=await doctor(exe,config,['--probe']);assert.equal(actual.all_requested_probes_passed,true);assert.equal(actual.probe_identity_unchanged,true);
  assert.deepEqual(actual.probes[0].observation.version,[3,11,9]);assert.equal(actual.probes.length,role==='eef'?1:2);
  if(role==='eefn')assert.equal(actual.probes[1].observation.build,10621);
  for(const field of ['running_service_inspected','model_inference_checked','ollama_checked','permissions_changed','models_changed','repair_requested','restart_requested','automatically_retried'])assert.equal(actual[field],false);
  assert.equal(actual.system_ready,null);
  // -I plus the package's ._pth must not borrow our deliberately supplied donor
  // PYTHONHOME/PYTHONPATH when this package's native extension is removed.
  const ctypes=join(dir,'python/_ctypes.pyd');await remove(ctypes);
  const missingImport=await doctor(exe,config,['--probe'],false);assert.equal(missingImport.probes[0].observation.failure_code,'python_module_import_failed');
  assert.equal(missingImport.probes[0].observation.modules.find(m=>m.name==='ctypes').imported,false);await alias(join(bundle,'python/_ctypes.pyd'),ctypes);
  const runtimeDll=join(dir,'python/python311.dll');await remove(runtimeDll);
  const loader=await doctor(exe,config,['--probe','--timeout-seconds','2'],false);assert.notEqual(loader.probes[0].state,'passed');await alias(join(bundle,'python/python311.dll'),runtimeDll);
  await remove(python);const absent=await doctor(exe,config,['--probe'],false);assert.equal(absent.probes[0].failure_code,'bundled_runtime_missing');
  await alias(join(bundle,'python/python.exe'),python);
  if(role==='eefn'){
   const optional=await doctor(exe,config,['--probe','--media'],false);assert.equal(optional.probes.at(-1).id,'python_media');
   assert.equal(optional.probes.at(-1).observation.failure_code,'python_module_import_failed');
  }
  if(role==='eef'){
   // A directory junction is unsafe even though its target is another owned
   // fixture directory. Removing the junction must never remove the target.
   const pyDir=join(dir,'python'),saved=join(dir,'saved-python');await rename(owned(pyDir),owned(saved));
   await symlink(owned(saved),owned(pyDir),'junction');
   const unsafe=await doctor(exe,config,['--probe'],false);assert.equal(unsafe.probes[0].failure_code,'bundled_runtime_unsafe_path');
   await rmdir(owned(pyDir));assert((await readdir(saved)).length>0);await rename(owned(saved),owned(pyDir));
   await remove(python);await alias(join(bin,'examples/doctor-fixture.exe'),python);
   // The Rust fault helper itself needs libunwind. Supply its known development
   // dependency beside that synthetic executable, not via inherited PATH.
   await alias(join(root,'.tooling/llvm-mingw-20260616-ucrt-x86_64/bin/libunwind.dll'),join(dir,'python/libunwind.dll'));
   const mode=join(dir,'python/doctor-fixture-mode'),pids=join(dir,'python/doctor-fixture-pids.json');
   for(const kind of ['malformed','large']){await writeFile(mode,kind);const bad=await doctor(exe,config,['--probe'],false);assert.equal(bad.probes[0].failure_code,kind==='malformed'?'probe_reply_invalid':'probe_output_bound');}
   await writeFile(mode,'hang');const earlyStart=performance.now();const early=await doctor(exe,config,['--probe','--timeout-seconds','1'],false);
   assert.equal(early.probes[0].failure_code,'probe_deadline');assert(performance.now()-earlyStart<8000);
   let earlyStarted=false;try{await retired(pids);earlyStarted=true;}catch(e){if(e.code!=='ENOENT')throw e;}await remove(pids);
   // Separate post-start deadline: do not pretend the one-second startup-inclusive
   // case proves a descendant existed. This larger case requires both markers.
   const afterStart=await doctor(exe,config,['--probe','--timeout-seconds','8'],false);assert.equal(afterStart.probes[0].failure_code,'probe_deadline');await retired(pids);await remove(pids);
   await writeFile(mode,'inherited');const inherited=await doctor(exe,config,['--probe']);assert.equal(inherited.probes[0].state,'passed');await retired(pids);
   await writeFile(join(dir,'synthetic-probe-observations.json'),JSON.stringify({early_deadline_parent_or_child_started:earlyStarted,post_start_deadline_both_retired:true,inherited_pipe_descendant_retired:true}));
  }
  // Same-version repair selection: inspect that generation without launching
  // its app, even when old-root Python is absent. Never borrow the old root.
  const slot=version+'.repair-'+randomUUID(),selected=join(dir,'versions',slot);await alias(join(bin,role+'.exe'),join(selected,role+'.exe'));
  await writeFile(join(selected,'bundle.json'),await readFile(join(dir,'bundle.json')));
  await flat(join(bundle,'python'),join(selected,'python'),name=>name==='python.exe'||/\.(dll|pyd|zip|_pth)$/i.test(name));
  if(role==='eefn')await flat(join(bundle,'tools'),join(selected,'tools'),name=>name==='llama-server.exe'||/\.dll$/i.test(name));
  await writeFile(join(dir,'versions',version+'.bundle.json'),JSON.stringify({schema_version:1,version,slot,previous_slot:'$root'}));
  await writeFile(join(dir,'current.txt'),version);await remove(python);
  const selectedProbe=await doctor(exe,config,['--probe']);assert.equal(selectedProbe.scope,'selected_package');assert.equal(selectedProbe.selected_target_resolved,true);
  assert.equal(selectedProbe.all_requested_probes_passed,true);assert.equal(selectedProbe.target_application_launch_checked,false);
  await writeFile(join(dir,'current.txt'),'99.0.0');const pending=await doctor(exe,config);assert.equal(pending.scope,'invoked_metadata_only');assert.equal(pending.selection_pending,true);
  const blocked=await doctor(exe,config,['--probe'],false);assert.equal(blocked.probes[0].failure_code,'selected_bundle_unavailable');
  assert.equal(pending.running_service_inspected,false);assert.equal(pending.system_ready,null);assert.deepEqual(await readFile(config),sentinel);
 }
 await writeFile(join(scratch,'results.json'),JSON.stringify({passed:true,roles:['eef','eefn'],metadata_no_runtime_or_config_writes:true,
  real_stock_python_core_imports:true,real_stock_llama_version:true,model_inference:false,optional_media_missing_observed:true,
  missing_extension_no_pythonhome_fallback:true,missing_executable_no_path_fallback:true,missing_dll_not_passed:true,unsafe_directory_junction_refused:true,
  private_bad_output_not_exposed:true,output_bound_and_absolute_deadline:true,post_start_root_and_descendant_retirement:true,inherited_pipe_cleanup_synthetic_protocol:true,
  selected_generation_runtimes_not_old_root_or_app_launch:true,broken_selection_blocks_old_runtime_fallback:true,
  offline_pending_selection_inspection:true,owner_config_unchanged:true,physical_two_pc:false,publisher_or_defender_clearance:false},null,2));
 console.log('Bundled runtime doctor native checks passed: '+scratch);
}catch(error){await writeFile(join(scratch,'PRIVATE-failure.txt'),String(error));await writeFile(join(scratch,'PRIVATE-command-logs.json'),JSON.stringify(children.map(c=>({exe:c.spawnfile,code:c.exitCode,...c.logs()})),null,2));throw error;}
finally{for(const child of children)if(child.exitCode===null&&child.signalCode===null)child.kill();await Promise.allSettled(children.map(c=>c.result));for(const path of [...new Set(links)].reverse())await remove(path);}
