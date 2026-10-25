// Explicitly opted-in standard-user synthetic lifecycle; never executes payload.
import assert from 'node:assert/strict';
import {mkdtemp,mkdir,writeFile,readFile,stat,readdir} from 'node:fs/promises';
import {join,resolve,dirname} from 'node:path';
import {createHash,randomUUID} from 'node:crypto';
import {spawnSync} from 'node:child_process';
import {parseArgs} from 'node:util';
assert.equal(process.platform,'win32','Native MSI lifecycle requires Windows');
assert.equal(process.env.EEF_TEST_ALLOW_MSI_INSTALL,'1','Explicit EEF_TEST_ALLOW_MSI_INSTALL=1 is required');
const {values}=parseArgs({options:{rollback:{type:'boolean',default:false}}}),rollback=values.rollback;
const root=resolve(import.meta.dirname,'..');await mkdir(join(root,'.validation'),{recursive:true});
const scratch=await mkdtemp(join(root,'.validation/msi-upgrade-')),namespace=randomUUID(),sha=b=>createHash('sha256').update(b).digest('hex');
const sourcePaths=['tools/build-development-msi.ps1','tools/inspect-development-msi.ps1','tools/msi-version-policy.mjs','tools/msi-payload-plan.mjs','tools/msi-upgrade-fixture.ps1','tools/test-msi-upgrade.mjs'];
if(rollback)sourcePaths.push('tools/msi-upgrade-rollback-fixture.ps1');
const sources=[];for(const path of sourcePaths)sources.push({path,sha256:sha(await readFile(join(root,path)))});
await writeFile(join(scratch,'source-hashes.json'),JSON.stringify(sources,null,2),{flag:'wx'});
async function unchanged(){for(const row of sources)assert.equal(sha(await readFile(join(root,row.path))),row.sha256,'Lifecycle source changed; preserve state and do not continue');}
const reports=[],probes=[],operations=[],observations=[],owners=new Map(),current=new Map(),versions=['1.2.3-alpha.1','1.2.3-beta.1'];let phase='initial';
const ps=(file,args,timeout=240000)=>spawnSync('powershell.exe',['-NoProfile','-NonInteractive','-ExecutionPolicy','Bypass','-File',join(root,'tools',file),...args],{cwd:root,encoding:'utf8',windowsHide:true,timeout,maxBuffer:2*1024*1024});
const pathFor=role=>join(process.env.LOCALAPPDATA,'eef-Development-'+role+'-'+namespace.replaceAll('-',''));
async function text(path){const b=await readFile(path);return b[0]===255&&b[1]===254?b.subarray(2).toString('utf16le'):b.toString('utf8').replace(/^\uFEFF/,'');}
async function helper(action,report,label){
 await unchanged();console.log('Observing native MSI phase: '+label);
 const args=['-FixtureRoot',scratch,'-Action',action];if(report)args.push('-Package',report.package,'-Operation',label,'-AllowSyntheticInstall');
 const result=ps('msi-upgrade-fixture.ps1',args);await writeFile(join(scratch,label+'-helper.log'),(result.stdout||'')+'\n'+(result.stderr||''),{flag:'wx'});
 assert.equal(result.status,0,'Native helper failed; preserve exact fixture/process: '+scratch+' / '+label);assert.equal(result.signal,null);await unchanged();return JSON.parse(result.stdout);
}
function registered(observation,expected){assert.equal(observation.standard_user,true);assert.equal(observation.administrator_token,false);for(const row of observation.products){assert.equal(row.registered,expected.includes(row.product_code));if(row.registered){assert.equal(row.state,5);assert.equal(row.context,2);assert.equal(row.current_user_verified,true);}else assert.equal(row.state,-1);}}
async function verifyFiles(report){
 const target=pathFor(report.role),files=JSON.parse(await readFile(join(dirname(report.package),'files.sha256.json'),'utf8'));
 for(const row of files){const file=join(target,row.path);assert.equal((await stat(file)).size,row.bytes);assert.equal(sha(await readFile(file)),row.sha256);}
 assert.equal(sha(await readFile(join(target,'files.sha256.json'))),report.regenerated_inventory_sha256);
 if(owners.has(report.role))assert.equal(sha(await readFile(join(target,'owner-retain.txt'))),owners.get(report.role));
 observations.push({phase,role:report.role,product_code:report.product_code,all_payload_hashes_verified:true,payload_files:files.length+1,owner_marker_sha256:owners.get(report.role)??null});
}
async function native(action,report,label,expectedExit){
 phase=label;
 const result=await helper(action,report,label);assert.equal(result.completed,true);assert.equal(result.native_exit,expectedExit,'Unexpected native exit; inspect retained state: '+scratch);assert.equal(result.run_as_requested,false);assert.equal(result.restart_requested,false);assert.equal(result.product_code,report.product_code);
 const log=await text(join(scratch,label+'.msi.log'));assert(log.includes('MainEngineThread is returning '+expectedExit));
 if(expectedExit===1603&&action==='rollback-probe'){
  assert(log.includes('Synthetic upgrade rollback probe; transaction intentionally fails.'));
  const executed=/Action ended [^\r\n]*InstallExecute\. Return value 1/.exec(log),failed=/Action ended [^\r\n]*eef_fail_upgrade_probe\. Return value 3/.exec(log);assert(executed&&failed&&executed.index<failed.index);
  const before=log.slice(0,executed.index),after=log.slice(failed.index);assert.match(before,/Executing op: FileCopy\(/);assert.match(before,/Executing op: RegAddValue\(/);assert.match(after,/Executing op: Header\([^\r\n]*ScriptType=2/);assert.match(after,/Executing op: FileRemove\(/);assert.match(after,/Executing op: FileCopy\(/);assert.match(after,/Executing op: RegRemoveValue\(/);
 }else if(expectedExit===1603){assert(log.includes('A newer version of '+report.role+' is already installed. Downgrades are not supported.'));assert.match(log,/Action ended [^\r\n]*eef_block_downgrade\. Return value 3/);assert(!/Doing action: InstallInitialize/.test(log),'Downgrade must stop before transaction');}
 operations.push({label,action,role:report.role,native_exit:expectedExit,completed:true});return result;
}
try{
 for(const role of ['eef','eefn'])for(const version of versions){
  await unchanged();console.log('Authoring synthetic MSI: '+role+' '+version);
  const bundle=join(scratch,role+'-'+version);await mkdir(bundle);
  const metadata={name:role,version,architecture:'x86_64-windows',runtime:'rust',python:'synthetic',...(role==='eefn'?{models_included:false}:{})};
  const payload={'bundle.json':Buffer.from(JSON.stringify(metadata)),[role+'.exe']:Buffer.from('Synthetic MSI upgrade fixture. NOT AN EXECUTABLE.\n'+role+' '+version),'python/python.exe':Buffer.from('Synthetic Python. NOT AN EXECUTABLE.\n'+version),'nested/payload.txt':Buffer.from('Synthetic nested payload '+role+' '+version)};
  if(role==='eefn')payload['tools/llama-server.exe']=Buffer.from('Synthetic runtime. NOT AN EXECUTABLE.\n'+version);
  const inventory=[];for(const [path,bytes] of Object.entries(payload)){const file=join(bundle,path);await mkdir(dirname(file),{recursive:true});await writeFile(file,bytes,{flag:'wx'});inventory.push({path,bytes:bytes.length,sha256:sha(bytes)});}
  await writeFile(join(bundle,'files.sha256.json'),JSON.stringify(inventory),{flag:'wx'});
  const output=join(scratch,role+'-'+version+'-authored'),result=ps('build-development-msi.ps1',['-BundleDir',bundle,'-Role',role,'-ExpectedVersion',version,'-ProjectNamespace',namespace,'-OutputDirectory',output,'-NodePath',process.execPath,'-DevelopmentOnly']);
  await writeFile(join(scratch,role+'-'+version+'-authoring.log'),(result.stdout||'')+'\n'+(result.stderr||''));assert.equal(result.status,0,'Synthetic authoring failed: '+scratch);const report=JSON.parse(result.stdout);assert.equal(report.upgrade_policy,'major-upgrade-v1');assert.equal(report.installation_attempted,false);assert.equal(report.custom_executable_actions_authored,false);reports.push(report);
 }
 assert.equal(new Set(reports.map(r=>r.product_code)).size,4);assert.equal(new Set(reports.map(r=>r.upgrade_code)).size,2);
 if(rollback){
  const candidates=reports.filter(r=>r.source_version===versions[1]);await writeFile(join(scratch,'rollback-probe-intent.json'),JSON.stringify({payload_kind:'synthetic-non-executable-only',namespace,sources:candidates},null,2),{flag:'wx'});
  for(const candidate of candidates){
   await unchanged();const packagePath=join(dirname(candidate.package),candidate.role+'-synthetic-rollback.msi');
   const made=ps('msi-upgrade-rollback-fixture.ps1',['-Action','create','-FixtureRoot',scratch,'-Source',candidate.package,'-SourceSha256',candidate.package_sha256,'-Package',packagePath]);await writeFile(join(scratch,candidate.role+'-rollback-authoring.log'),(made.stdout||'')+'\n'+(made.stderr||''),{flag:'wx'});assert.equal(made.status,0,'Rollback probe readback failed: '+scratch);const probe=JSON.parse(made.stdout);assert.equal(probe.synthetic_rollback_recipe_verified,true);assert.equal(probe.ordinary_recipe_verified,false);assert.equal(probe.installation_attempted,false);probes.push(probe);
   const refused=ps('inspect-development-msi.ps1',['-Package',probe.package,'-ExpectedSha256',probe.package_sha256]);await writeFile(join(scratch,candidate.role+'-rollback-ordinary-refusal.log'),(refused.stdout||'')+'\n'+(refused.stderr||''),{flag:'wx'});assert.notEqual(refused.status,0);assert.equal(refused.stdout,'');assert.match(refused.stderr,/Not the supported development-only property contract/);
  }
 }
 await writeFile(join(scratch,'fixture.json'),JSON.stringify({schema_version:1,payload_kind:'synthetic-non-executable-only',namespace,packages:reports,rollback_probes:probes},null,2),{flag:'wx'});
 const initial=await helper('inspect',null,'initial-observation');registered(initial,[]);
 for(const role of ['eef','eefn'])await assert.rejects(()=>stat(pathFor(role)),{code:'ENOENT'});
 for(const role of ['eef','eefn']){
  const old=reports.find(r=>r.role===role&&r.source_version===versions[0]);
  const result=await native('install',old,role+'-initial-install',0);current.set(role,old);registered(result.after,[...current.values()].map(r=>r.product_code));await verifyFiles(old);
  const marker=Buffer.from('Generated unowned owner data for '+role+' '+randomUUID());await writeFile(join(pathFor(role),'owner-retain.txt'),marker,{flag:'wx'});owners.set(role,sha(marker));
 }
 await writeFile(join(scratch,'owner-file-baseline.json'),JSON.stringify([...owners].map(([role,sha256])=>({role,path:'owner-retain.txt',sha256})),null,2),{flag:'wx'});
 for(const role of ['eef','eefn']){
  const old=current.get(role),next=reports.find(r=>r.role===role&&r.source_version===versions[1]);assert.equal(old.upgrade_code,next.upgrade_code);assert.notEqual(old.product_code,next.product_code);
  if(rollback){const probe=probes.find(p=>p.role===role);const restored=await native('rollback-probe',probe,role+'-failed-upgrade-rollback',1603);registered(restored.after,[...current.values()].map(r=>r.product_code));for(const report of current.values())await verifyFiles(report);}
  const upgraded=await native('install',next,role+'-major-upgrade',0);current.set(role,next);registered(upgraded.after,[...current.values()].map(r=>r.product_code));
  const upgradeLog=await text(join(scratch,role+'-major-upgrade.msi.log'));assert.match(upgradeLog,/Doing action: FindRelatedProducts/);assert.match(upgradeLog,/Doing action: RemoveExistingProducts/);assert(upgradeLog.includes(old.product_code));
  for(const report of current.values())await verifyFiles(report);
  const refused=await native('install',old,role+'-downgrade-refused',1603);registered(refused.after,[...current.values()].map(r=>r.product_code));for(const report of current.values())await verifyFiles(report);
  const repaired=await native('reinstall',next,role+'-same-package-reinstall',0);registered(repaired.after,[...current.values()].map(r=>r.product_code));for(const report of current.values())await verifyFiles(report);
 }
 for(const role of ['eef','eefn']){
  const report=current.get(role),removed=await native('uninstall',report,role+'-uninstall',0);current.delete(role);registered(removed.after,[...current.values()].map(r=>r.product_code));
  const files=JSON.parse(await readFile(join(dirname(report.package),'files.sha256.json'),'utf8'));for(const row of [...files,{path:'files.sha256.json'}])await assert.rejects(()=>stat(join(pathFor(role),row.path)),{code:'ENOENT'});
  assert.equal(sha(await readFile(join(pathFor(role),'owner-retain.txt'))),owners.get(role));assert.deepEqual((await readdir(pathFor(role))).sort(),['owner-retain.txt']);for(const survivor of current.values())await verifyFiles(survivor);
 }
 const final=await helper('inspect',null,'final-observation');registered(final,[]);for(const report of [...reports,...probes])assert.equal(sha(await readFile(report.package)),report.package_sha256);
 await unchanged();assert.equal(operations.length,rollback?12:10);await writeFile(join(scratch,'results.json'),JSON.stringify({passed:true,namespace,packages:reports,rollback_probes:probes,synthetic_rollback_verified:rollback,operations,file_observations:observations,source_hashes_unchanged:true,standard_user:true,per_user_unmanaged:true,both_roles_coexist:true,role_isolation_verified:true,older_product_removed:true,newer_product_retained_after_downgrade:true,downgrade_before_transaction:true,same_package_reinstall:true,owner_data_retained:true,all_products_unregistered:true,payload_executed:false,application_lifecycle_verified:false,production_migration_verified:false,real_application_rollback_verified:false,signing_defender_verified:false,stable_release_accepted:false},null,2),{flag:'wx'});
 console.log('Synthetic MSI upgrade lifecycle passed: '+scratch);
}catch(error){await writeFile(join(scratch,'PRIVATE-failure.json'),JSON.stringify({message:String(error),namespace,completedOperations:operations,registeredStateNotAutomaticallyChanged:true,automaticRetry:false},null,2),{flag:'wx'});throw error;}
