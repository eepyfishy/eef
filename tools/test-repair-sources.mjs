// Native source-command/metadata/early-refusal checks, not a successful public
// HTTPS descriptor/artifact transfer. Unit tests separately inject descriptor
// bytes through a private transport seam to exercise exact source approval.
import assert from 'node:assert/strict';
import {mkdtemp,mkdir,link,unlink,readFile,writeFile,access} from 'node:fs/promises';
import {join,resolve,dirname,sep} from 'node:path';
import {spawn} from 'node:child_process';
import {createServer} from 'node:net';
import {randomUUID,createHash} from 'node:crypto';
assert.equal(process.platform,'win32');
const root=resolve(import.meta.dirname,'..'),bin=resolve(process.env.EEF_TEST_BINARY_DIR||join(root,'target/debug'));
const scratch=await mkdtemp(join(root,'.validation/repair-sources-')),children=[],links=[],sockets=new Set();
const env={...process.env,EEF_NODE_PSK:'',APPDATA:join(scratch,'appdata'),EEF_DISCOVERY_DIR:join(scratch,'discovery'),PATH:join(root,'.tooling/llvm-mingw-20260616-ucrt-x86_64/bin')+';'+process.env.PATH};
for(const key of Object.keys(env))if(['HTTP_PROXY','HTTPS_PROXY','ALL_PROXY','NO_PROXY'].includes(key.toUpperCase()))delete env[key];
env.NO_PROXY='127.0.0.1,localhost';
const hash=bytes=>createHash('sha256').update(bytes).digest('hex');
let connections=0;
const gate=createServer(socket=>{connections++;sockets.add(socket);socket.on('close',()=>sockets.delete(socket));socket.on('error',()=>{});socket.resume();});
function owned(path){assert(resolve(path).startsWith(resolve(scratch)+sep));return path;}
function launch(exe,args){const c=spawn(exe,args,{cwd:root,env,windowsHide:true,stdio:['ignore','pipe','pipe']});let out='',err='';
 c.stdout.on('data',b=>out=(out+b).slice(-1048576));c.stderr.on('data',b=>err=(err+b).slice(-1048576));c.logs=()=>({out,err});
 c.result=new Promise((r,j)=>{c.on('error',j);c.on('close',code=>r({code,out,err}));});children.push(c);return c;}
async function run(exe,args,code=0){const c=launch(exe,args),timer=setTimeout(()=>c.kill(),25000);try{const r=await c.result;assert.equal(r.code,code,r.out+r.err);return r;}finally{clearTimeout(timer);}}
async function cli(exe,args,success=true,json=true){const r=await run(exe,[...(json?['--json']:[]),'repair',...args],success?0:1);return json?JSON.parse(r.out):r.out;}
async function until(fn,label){const end=Date.now()+12000;while(Date.now()<end){if(await fn())return;await new Promise(r=>setTimeout(r,50));}throw Error('Timeout: '+label);}
async function absent(path){try{await access(path);assert.fail('Unexpected file: '+path);}catch(e){if(e.code!=='ENOENT')throw e;}}
async function stop(c){if(c.exitCode===null)c.kill();await c.result;for(const s of sockets)s.destroy();}
try{
 await new Promise(r=>gate.listen(0,'127.0.0.1',r));
 const source=`https://127.0.0.1:${gate.address().port}/descriptor?PRIVATE-SOURCE-TOKEN`,artifact=join(scratch,'synthetic-package.exe');
 await writeFile(artifact,'synthetic artifact only; never executable or served');
 for(const role of ['eef','eefn']){
  const install=join(scratch,role),exe=join(install,role+'.exe');await mkdir(install);await link(join(bin,role+'.exe'),exe);links.push(exe);
  const config=join(install,'config.json'),owner=Buffer.from('owner sentinel: not JSON; offline commands must not touch it');await writeFile(config,owner);
  const version=(await run(exe,['--version'])).out.trim().split(/\s+/).at(-1),planPath=join(install,'.package-repair-plans.json'),receiptPath=join(install,'.package-repairs.json');
  const help=(await run(exe,['repair','--help'])).out;for(const arg of ['--check','--manifest','--approve'])assert(help.includes(arg));
  const denied=await cli(exe,['--check','--manifest','http://private-token@127.0.0.1/secret','--allow-prerelease'],false);
  assert.equal(denied.error_code,'repair_source_requires_https');assert(!JSON.stringify(denied).includes('private-token'));await absent(planPath);await absent(receiptPath);
  const before=connections,check=launch(exe,['--json','repair','--check','--manifest',source,'--allow-prerelease']);
  await until(()=>connections>before,'public HTTPS descriptor fetch '+role);assert.equal(check.exitCode,null);await stop(check);
  await absent(planPath);await absent(receiptPath);
  // Exercise the real post-packaging metadata writer without executing this
  // synthetic package. It refuses overwrite and unsafe publisher URLs.
  const descriptorPath=join(install,'descriptor.json'),writer=join(root,'tools/new-repair-source.ps1');
  const writerArgs=['-NoProfile','-NonInteractive','-ExecutionPolicy','Bypass','-File',writer,'-Product',role.toUpperCase(),'-Version',version,'-Executable',exe,'-Artifact',artifact,'-ArtifactUrl',source.replace('/descriptor','/artifact'),'-Output',descriptorPath];
  await run('powershell.exe',writerArgs);const bytes=await readFile(descriptorPath),descriptor=JSON.parse(bytes);
  assert.equal(descriptor.program,role);assert.equal(descriptor.version,version);assert.equal(descriptor.executable_sha256,hash(await readFile(exe)));
  assert.equal(descriptor.artifact.sha256,hash(await readFile(artifact)));assert.equal(descriptor.artifact.size_bytes,(await readFile(artifact)).length);
  await run('powershell.exe',writerArgs,1);assert.deepEqual(await readFile(descriptorPath),bytes);
  const badArgs=[...writerArgs];badArgs[badArgs.indexOf('-ArtifactUrl')+1]='http://publisher.invalid/not-https';badArgs[badArgs.indexOf('-Output')+1]=join(install,'bad.json');
  await run('powershell.exe',badArgs,1);await absent(join(install,'bad.json'));
  // Synthetic proposal-store fixture tests the actual owner CLI admission
  // fences. This is not evidence that --check successfully fetched TLS metadata.
  const id=randomUUID(),now=Date.now(),plan={id,program:role,version,executable_sha256:descriptor.executable_sha256,running_slot:'$root',
   source_url_sha256:hash(source),manifest_sha256:hash(bytes),artifact_sha256:descriptor.artifact.sha256,artifact_bytes:descriptor.artifact.size_bytes,
   allow_prerelease:true,created_at_ms:now,expires_at_ms:now+86400000};
  const retained=Buffer.from(JSON.stringify({schema_version:1,plans:[plan]}));await writeFile(planPath,retained);
  const status=await cli(exe,['--status','--operation-id',id]);assert.equal(status.repair_source_plans.plans[0].id,id);assert.equal(status.repair_source_plans.approval_authorized,false);
  const human=await cli(exe,['--status','--operation-id',id],true,false);assert(human.includes(id));assert(!human.includes('PRIVATE-SOURCE-TOKEN'));
  const count=connections,mismatch=await cli(exe,['--manifest',source+'&changed','--approve',id,'--allow-prerelease'],false);
  assert.equal(mismatch.error_code,'repair_source_url_changed');assert.equal(connections,count);await absent(receiptPath);
  const eligible=launch(exe,['--json','repair','--manifest',source,'--approve',id,'--allow-prerelease']);
  await until(()=>connections>count,'exact proposal descriptor fetch '+role);await stop(eligible);await absent(receiptPath);assert.deepEqual(await readFile(planPath),retained);
  const nextCount=connections;plan.executable_sha256='b'.repeat(64);await writeFile(planPath,JSON.stringify({schema_version:1,plans:[plan]}));
  const changed=await cli(exe,['--manifest',source,'--approve',id,'--allow-prerelease'],false);assert.equal(changed.error_code,'repair_source_proposal_ineligible');assert.equal(connections,nextCount);
  const damaged='{"schema_version":99,"plans":[]}';await writeFile(planPath,damaged);
  const unavailable=await cli(exe,['--check','--manifest',source,'--allow-prerelease'],false);assert.equal(unavailable.error_code,'repair_source_store_unavailable');assert.equal(connections,nextCount);
  const partial=await cli(exe,['--status'],false);assert.equal(partial.repair_source_plans.available,false);assert.equal(partial.repair_receipts.available,true);
  assert.equal(await readFile(planPath,'utf8'),damaged);assert.deepEqual(await readFile(config),owner);
 }
 await writeFile(join(scratch,'results.json'),JSON.stringify({passed:true,roles:['eef','eefn'],public_source_fetch_entrypoint:true,
  public_https_descriptor_success:false,artifact_served:false,real_package_repair:false,synthetic_proposal_store:true,
  owner_cli_source_and_build_fences:true,no_artifact_admission_during_descriptor_fetch:true,proposal_inspection_read_only:true,
  publisher_descriptor_writer:true,writer_no_overwrite:true,owner_config_unchanged:true,no_tls_trust_changes:true,physical_two_pc:false},null,2));
 console.log('Repair source native checks passed: '+scratch);
}catch(error){await writeFile(join(scratch,'PRIVATE-failure.json'),JSON.stringify({message:String(error)},null,2));await writeFile(join(scratch,'PRIVATE-logs.json'),JSON.stringify(children.map(c=>({exe:c.spawnfile,code:c.exitCode,...c.logs()})),null,2));throw error;}
finally{for(const c of children)if(c.exitCode===null)c.kill();await Promise.allSettled(children.map(c=>c.result));for(const socket of sockets)socket.destroy();await new Promise(r=>gate.close(r));for(const path of links)await unlink(owned(path));}
