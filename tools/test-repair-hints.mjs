// Current-bundle discovery and real publisher helpers. TCP gates never serve a
// TLS handshake/artifact. No installed node, model, OS trust or release is edited.
import assert from 'node:assert/strict';
import {mkdtemp,mkdir,link,unlink,readFile,writeFile,access} from 'node:fs/promises';
import {resolve,join,dirname,sep} from 'node:path';
import {spawn} from 'node:child_process';
import {createServer} from 'node:net';
import {createHash,randomUUID} from 'node:crypto';
assert.equal(process.platform,'win32');
const root=resolve(import.meta.dirname,'..'),bin=resolve(process.env.EEF_TEST_BINARY_DIR||join(root,'target/debug'));
const scratch=await mkdtemp(join(root,'.validation/repair-hints-')),children=[],links=[],sockets=new Set();
const env={...process.env,EEF_NODE_PSK:'',APPDATA:join(scratch,'appdata'),EEF_DISCOVERY_DIR:join(scratch,'discovery'),PATH:join(root,'.tooling/llvm-mingw-20260616-ucrt-x86_64/bin')+';'+process.env.PATH};
for(const key of Object.keys(env))if(['HTTP_PROXY','HTTPS_PROXY','ALL_PROXY','NO_PROXY'].includes(key.toUpperCase()))delete env[key];
env.NO_PROXY='127.0.0.1,localhost';
const hash=bytes=>createHash('sha256').update(bytes).digest('hex');let connections=0;
const gate=createServer(socket=>{connections++;sockets.add(socket);socket.on('close',()=>sockets.delete(socket));socket.on('error',()=>{});socket.resume();});
function owned(path){assert(resolve(path).startsWith(resolve(scratch)+sep));return path;}
async function alias(source,target){await mkdir(dirname(owned(target)),{recursive:true});await link(source,target);links.push(target);}
function launch(exe,args){const c=spawn(exe,args,{cwd:root,env,windowsHide:true,stdio:['ignore','pipe','pipe']});let out='',err='';
 c.stdout.on('data',b=>out=(out+b).slice(-1048576));c.stderr.on('data',b=>err=(err+b).slice(-1048576));c.logs=()=>({out,err});
 c.result=new Promise((r,j)=>{c.on('error',j);c.on('close',code=>r({code,out,err}));});children.push(c);return c;}
async function run(exe,args,code=0){const c=launch(exe,args),timer=setTimeout(()=>c.kill(),25000);try{const r=await c.result;assert.equal(r.code,code,r.out+r.err);return r;}finally{clearTimeout(timer);}}
async function cli(exe,args,success=true,json=true){const r=await run(exe,[...(json?['--json']:[]),'repair',...args],success?0:1);return json?JSON.parse(r.out):r.out;}
async function ps(script,args,code=0){return run('powershell.exe',['-NoProfile','-NonInteractive','-ExecutionPolicy','Bypass','-File',join(root,'tools',script),...args],code);}
async function until(fn,label){const end=Date.now()+12000;while(Date.now()<end){if(await fn())return;await new Promise(r=>setTimeout(r,50));}throw Error('Timeout: '+label);}
async function absent(path){try{await access(path);assert.fail('Unexpected file: '+path);}catch(error){if(error.code!=='ENOENT')throw error;}}
async function stop(c){if(c.exitCode===null)c.kill();await c.result;for(const socket of sockets)socket.destroy();}
try{
 await new Promise(r=>gate.listen(0,'127.0.0.1',r));const base=`https://127.0.0.1:${gate.address().port}`;
 const template=base+'/v{version}/{program}-repair.json',artifactTemplate=base+'/v{version}/{program}-installer.exe';
 // These deliberately invalid invocations terminate before any download,
 // build, package creation or legacy builder cleanup. No full bundle run here.
 await ps('bundle-windows.ps1',['-AllowPrerelease','-RepairSourceTemplate',template],1);
 await ps('bundle-windows.ps1',['-AllowPrerelease','-RepairSourceTemplate','http://invalid/{version}/{program}.json','-RepairArtifactTemplate',artifactTemplate],1);
 const builder=await readFile(join(root,'tools/bundle-windows.ps1'),'utf8');
 const hintAt=builder.indexOf('& $repairHintScript -Product $product -Version $version -ManifestTemplate $RepairSourceTemplate -Output'),inventoryAt=builder.indexOf("'files.sha256.json'");
 const descriptorAt=builder.indexOf('& $repairDescriptorScript -Product'),scanAt=builder.lastIndexOf('& $scanScript -Path'),signAt=builder.lastIndexOf('& $signScript -SignToolPath');
 assert(hintAt>=0&&inventoryAt>hintAt);assert(scanAt>=0&&descriptorAt>scanAt);assert(signAt>=0&&descriptorAt>signAt);
 for(const role of ['eef','eefn']){
  const install=join(scratch,role),exe=join(install,role+'.exe');await alias(join(bin,role+'.exe'),exe);
  const version=(await run(exe,['--version'])).out.trim().split(/\s+/).at(-1),hintPath=join(install,role+'.repair-source.json');
  const owner=Buffer.from('owner settings sentinel: not JSON'),config=join(install,'config.json');await writeFile(config,owner);
  await writeFile(join(install,'bundle.json'),JSON.stringify({name:role,version}));
  const noSource=await cli(exe,['--check','--allow-prerelease'],false);assert.equal(noSource.error_code,'repair_source_unconfigured');
  const writerArgs=['-Product',role.toUpperCase(),'-Version',version,'-ManifestTemplate',template];
  const source=(await ps('new-repair-source-hint.ps1',[...writerArgs,'-ResolveOnly'])).out.trim();
  assert.equal(source,base+`/v${version}/${role}-repair.json`);
  await ps('new-repair-source-hint.ps1',[...writerArgs,'-Output',hintPath]);const hintBytes=await readFile(hintPath),hint=JSON.parse(hintBytes);
  assert.equal(hint.program,role);assert.equal(hint.version,version);assert.equal(hint.manifest_url,source);
  await ps('new-repair-source-hint.ps1',[...writerArgs,'-Output',hintPath],1);assert.deepEqual(await readFile(hintPath),hintBytes);
  for(const bad of [template+'?PRIVATE_QUERY',template+'#PRIVATE_FRAGMENT','https://user:PRIVATE_PASSWORD@publisher.invalid/{version}/{program}.json',base+'/fixed.json']){
   const output=join(install,'invalid.json');await ps('new-repair-source-hint.ps1',['-Product',role,'-Version',version,'-ManifestTemplate',bad,'-Output',output],1);await absent(output);
  }
  const initial=await cli(exe,['--status']);assert.equal(initial.repair_source_hint.state,'configured');assert.equal(initial.repair_source_hint.publisher_source,source);
  assert.equal(initial.repair_source_hint.authority_granted,false);assert.equal(initial.repair_source_hint.publisher_signature_verified,false);
  assert((await cli(exe,['--status'],true,false)).includes(source));assert.deepEqual(await readFile(hintPath),hintBytes);
  const receipts=join(install,'.package-repairs.json'),plans=join(install,'.package-repair-plans.json'),before=connections;
  const checking=launch(exe,['--json','repair','--check','--allow-prerelease']);await until(()=>connections>before,'implicit source metadata fetch '+role);await stop(checking);
  await absent(receipts);await absent(plans);
  const artifact=join(install,'synthetic-package.exe');await writeFile(artifact,'synthetic package bytes; never executable, served or installed');
  const descriptorPath=join(install,role+'-repair.json'),artifactUrl=(await ps('new-repair-source-hint.ps1',['-Product',role,'-Version',version,'-ManifestTemplate',artifactTemplate,'-ResolveOnly'])).out.trim();
  await ps('new-repair-source.ps1',['-Product',role,'-Version',version,'-Executable',exe,'-Artifact',artifact,'-ArtifactUrl',artifactUrl,'-Output',descriptorPath]);
  const descriptorBytes=await readFile(descriptorPath),descriptor=JSON.parse(descriptorBytes);assert.equal(descriptor.executable_sha256,hash(await readFile(exe)));
  // Synthetic proposal store exercises implicit approval fences, not successful
  // public descriptor delivery. The two actual helpers produce matching fields.
  const id=randomUUID(),now=Date.now(),plan={id,program:role,version,executable_sha256:descriptor.executable_sha256,running_slot:'$root',source_url_sha256:hash(source),
   manifest_sha256:hash(descriptorBytes),artifact_sha256:descriptor.artifact.sha256,artifact_bytes:descriptor.artifact.size_bytes,allow_prerelease:true,created_at_ms:now,expires_at_ms:now+86400000};
  await writeFile(plans,JSON.stringify({schema_version:1,plans:[plan]}));const count=connections;
  const approving=launch(exe,['--json','repair','--approve',id,'--allow-prerelease']);await until(()=>connections>count,'implicit approval metadata fetch '+role);await stop(approving);await absent(receipts);
  await writeFile(hintPath,JSON.stringify({...hint,manifest_url:source+'/changed'}));const next=connections;
  const changed=await cli(exe,['--approve',id,'--allow-prerelease'],false);assert.equal(changed.error_code,'repair_source_url_changed');assert.equal(connections,next);
  await writeFile(hintPath,JSON.stringify({...hint,manifest_url:source+'?PRIVATE_HINT_TOKEN'}));
  const invalid=await cli(exe,['--check','--allow-prerelease'],false);assert.equal(invalid.error_code,'repair_source_hint_invalid');assert(!JSON.stringify(invalid).includes('PRIVATE_HINT_TOKEN'));
  const optional=await cli(exe,['--status']);assert.equal(optional.repair_source_hint.state,'unavailable');assert.equal(optional.success,true);
  const explicit=launch(exe,['--json','repair','--check','--manifest',source+'?PRIVATE_EXPLICIT_TOKEN','--allow-prerelease']);
  await until(()=>connections>next,'explicit source override despite invalid hint '+role);await stop(explicit);await absent(receipts);
  await writeFile(hintPath,hintBytes);
  const bundle=join(install,'versions',version),versioned=join(bundle,role+'.exe');await alias(join(bin,role+'.exe'),versioned);
  const missing=await cli(versioned,['--check','--allow-prerelease'],false);assert.equal(missing.error_code,'repair_source_unconfigured','a versioned executable must never borrow its root hint');
  const currentTemplate=base+'/current/{version}/{program}-repair.json',currentHint=join(bundle,role+'.repair-source.json');
  await ps('new-repair-source-hint.ps1',['-Product',role,'-Version',version,'-ManifestTemplate',currentTemplate,'-Output',currentHint]);
  const currentUrl=JSON.parse(await readFile(currentHint,'utf8')).manifest_url;
  assert.equal((await cli(versioned,['--status'])).repair_source_hint.publisher_source,currentUrl);
  const pendingDir=join(install,'versions','99.0.0');await alias(join(bin,role+'.exe'),join(pendingDir,role+'.exe'));
  await ps('new-repair-source-hint.ps1',['-Product',role,'-Version','99.0.0','-ManifestTemplate',template,'-Output',join(pendingDir,role+'.repair-source.json')]);
  await writeFile(join(install,'current.txt'),'99.0.0');
  assert.equal((await cli(versioned,['--status'])).repair_source_hint.publisher_source,currentUrl,'a pending selection must not supply the current process source');
  const prePending=connections,pending=await cli(versioned,['--check','--allow-prerelease'],false);assert.equal(pending.error_code,'repair_source_build_unavailable');assert.equal(connections,prePending);
  assert.deepEqual(await readFile(config),owner);
 }
 await writeFile(join(scratch,'results.json'),JSON.stringify({passed:true,roles:['eef','eefn'],implicit_current_bundle_discovery:true,old_root_and_pending_hint_never_used:true,
  public_source_visible_before_approval:true,private_explicit_source_still_supported:true,packaged_secret_queries_rejected:true,
  changed_hint_refuses_descriptor_fetch:true,invalid_hint_does_not_disable_offline_status:true,paired_publisher_helpers:true,hint_no_overwrite:true,
  builder_negative_preflight:true,builder_order_checked_statically:true,full_packaging_run:false,public_https_descriptor_success:false,artifact_served:false,
  real_package_repair:false,synthetic_proposal_store:true,owner_config_unchanged:true,no_os_trust_or_update_policy_change:true,physical_two_pc:false},null,2));
 console.log('Repair hint native checks passed: '+scratch);
}catch(error){await writeFile(join(scratch,'PRIVATE-failure.json'),JSON.stringify({message:String(error)},null,2));await writeFile(join(scratch,'PRIVATE-logs.json'),JSON.stringify(children.map(c=>({exe:c.spawnfile,code:c.exitCode,...c.logs()})),null,2));throw error;}
finally{for(const c of children)if(c.exitCode===null)c.kill();await Promise.allSettled(children.map(c=>c.result));for(const socket of sockets)socket.destroy();await new Promise(r=>gate.close(r));for(const path of links)await unlink(owned(path));}
