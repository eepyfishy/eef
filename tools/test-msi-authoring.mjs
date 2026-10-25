// Windows-only authoring regression. Never install MSI or execute its payload.
import assert from 'node:assert/strict';
import {mkdtemp,mkdir,readFile,writeFile,access} from 'node:fs/promises';
import {join,resolve,dirname} from 'node:path';
import {createHash,randomBytes,randomUUID} from 'node:crypto';
import {spawnSync} from 'node:child_process';
assert.equal(process.platform,'win32','Native MSI authoring is a separate Windows-only test');
const root=resolve(import.meta.dirname,'..');await mkdir(join(root,'.validation'),{recursive:true});
const scratch=await mkdtemp(join(root,'.validation/msi-authoring-'));
const namespace=randomUUID(),sha=bytes=>createHash('sha256').update(bytes).digest('hex'),cases=[];
const controller=join(root,'tools/build-development-msi.ps1');
async function fixture(role,version='1.2.3-alpha.1',label=''){
 const bundle=join(scratch,role+' '+label+" source's files");await mkdir(bundle);
 const metadata={name:role,version,architecture:'x86_64-windows',runtime:'rust',python:'synthetic',...(role==='eefn'?{models_included:false}:{})};
 const data={'bundle.json':Buffer.from(JSON.stringify(metadata)),[role+'.exe']:Buffer.from('Synthetic payload. NEVER EXECUTE.'),'python/python.exe':Buffer.from('Synthetic Python. NEVER EXECUTE.'),'nested/level/LICENSE.txt':Buffer.from('UTF-8 payload bytes: สวัสดี'),'library++.dll':randomBytes(192*1024)};
 if(role==='eefn')data['tools/llama-server.exe']=Buffer.from('Synthetic inference runtime. NEVER EXECUTE.');
 const rows=[];for(const [path,bytes] of Object.entries(data)){const absolute=join(bundle,path);await mkdir(dirname(absolute),{recursive:true});await writeFile(absolute,bytes);rows.push({path,bytes:bytes.length,sha256:sha(bytes)});}
 await writeFile(join(bundle,'config.json'),'PRIVATE-owner-sentinel-must-not-be-packaged');rows.push({path:'config.json',bytes:41,sha256:'1'.repeat(64)});
 const inventory=Buffer.from(JSON.stringify(rows));await writeFile(join(bundle,'files.sha256.json'),inventory);
 return {bundle,data,inventory};
}
function run(bundle,role,output,{development=true,sourceVersion='1.2.3-alpha.1',msiVersion='1.2.769'}={}){
 const args=['-NoProfile','-NonInteractive','-ExecutionPolicy','Bypass','-File',controller,'-BundleDir',bundle,'-Role',role,'-ExpectedVersion',sourceVersion,'-ProjectNamespace',namespace,'-OutputDirectory',output,'-NodePath',process.execPath];if(msiVersion!==null)args.push('-MsiVersion',msiVersion);if(development)args.push('-DevelopmentOnly');
 return spawnSync('powershell.exe',args,{cwd:root,encoding:'utf8',windowsHide:true,timeout:120000,maxBuffer:2*1024*1024});
}
const reports=[];
try{
 for(const role of ['eef','eefn']){
  const input=await fixture(role),output=join(scratch,role+' authored output');
  const result=run(input.bundle,role,output,{msiVersion:role==='eefn'?null:'1.2.769'});
  await writeFile(join(scratch,role+'-stdout.log'),result.stdout||'');await writeFile(join(scratch,role+'-stderr.log'),result.stderr||'');
  assert.equal(result.status,0,'Authoring failed; inspect retained fixture '+scratch);
  const report=JSON.parse(result.stdout),saved=JSON.parse(await readFile(join(output,'authoring-results.json'),'utf8'));assert.deepEqual(report,saved);
  const inspected=spawnSync('powershell.exe',['-NoProfile','-NonInteractive','-ExecutionPolicy','Bypass','-File',join(root,'tools/inspect-development-msi.ps1'),'-Package',report.package,'-ExpectedSha256',report.package_sha256,'-Detailed'],{cwd:root,encoding:'utf8',windowsHide:true,timeout:120000,maxBuffer:2*1024*1024});
  await writeFile(join(scratch,role+'-inspection-stdout.json'),inspected.stdout||'');await writeFile(join(scratch,role+'-inspection-stderr.log'),inspected.stderr||'');assert.equal(inspected.status,0,'Independent database readback failed; inspect retained fixture '+scratch);
  const inspection=JSON.parse(inspected.stdout);assert.equal(inspection.success,true);assert.equal(inspection.read_only,true);assert.equal(inspection.file_count,report.files);assert.equal(inspection.component_count,report.files);assert.equal(inspection.registry_count,report.files);assert.equal(inspection.product_code,report.product_code);assert.equal(inspection.upgrade_code,report.upgrade_code);assert.equal(inspection.package_code,report.package_code);assert.equal(inspection.package_sha256,report.package_sha256);assert.equal(inspection.cabinet_sha256,report.cabinet_sha256);assert.equal(inspection.cabinet_bytes,report.cabinet_bytes);
  assert.equal(report.version_policy,'packed-patch-v1');assert.equal(report.version_policy_verified,true);assert.equal(report.source_is_prerelease,true);assert.equal(inspection.version_policy,report.version_policy);assert.equal(inspection.version_policy_verified,true);assert.equal(inspection.legacy_version_policy_opt_in,false);assert.equal(inspection.msi_version,'1.2.769');
  assert.equal(report.upgrade_policy,'major-upgrade-v1');assert.equal(report.upgrade_recipe_authored,true);assert.equal(report.downgrade_guard_authored,true);assert.equal(report.message_only_downgrade_guard,true);assert.equal(report.custom_actions_authored,true);assert.equal(report.custom_executable_actions_authored,false);assert.equal(inspection.upgrade_policy,report.upgrade_policy);assert.equal(inspection.upgrade_recipe_verified,true);assert.equal(inspection.legacy_upgrade_policy_opt_in,false);assert.equal(inspection.custom_executable_actions_present,false);assert.equal(inspection.message_only_downgrade_guard,true);assert.equal(inspection.installed_upgrade_verified,false);
  const plan=JSON.parse(await readFile(join(output,'payload-plan.json'),'utf8'));
  const expectedFiles=[...plan.files,plan.regenerated_inventory];assert.deepEqual(inspection.files.map(row=>row.path).sort(),expectedFiles.map(row=>row.path).sort());
  for(const file of inspection.files){const expected=expectedFiles.find(row=>row.path===file.path);for(const key of ['file_id','component_id','component_code','directory_id'])assert.equal(file[key],expected[key],key);}
  assert.equal(inspection.payload_bytes,Object.values(input.data).reduce((sum,bytes)=>sum+bytes.length,0)+(await readFile(join(output,'files.sha256.json'))).length);
  for(const key of ['authored','development_only','every_cabinet_file_verified','embedded_cabinet_verified','package_format_verified'])assert.equal(report[key],true,key);
  for(const key of ['installation_attempted','application_launched','mutable_configuration_packaged','models_downloaded','installed_upgrade_verified','startup_choices_implemented','upgrade_migration_verified','real_application_rollback_verified','full_ice_validation','signing_defender_verified','version_ordering_approved','msi_release_accepted','stable_release_accepted'])assert.equal(report[key],false,key);
  assert.equal(report.role,role);assert.equal(report.source_version,'1.2.3-alpha.1');assert.equal(report.msi_version,'1.2.769');assert.equal(report.files,Object.keys(input.data).length+1);
  assert(report.cabinet_bytes>65536,'Exercise more than one embedded-stream read');
  const packageBytes=await readFile(report.package);assert.equal(packageBytes.length,report.package_bytes);assert.equal(sha(packageBytes),report.package_sha256);
  assert.deepEqual(await readFile(join(input.bundle,'files.sha256.json')),input.inventory);assert.equal(await readFile(join(input.bundle,'config.json'),'utf8'),'PRIVATE-owner-sentinel-must-not-be-packaged');
  for(const [path,bytes] of Object.entries(input.data))assert.deepEqual(await readFile(join(input.bundle,path)),bytes);
  const regenerated=JSON.parse(await readFile(join(output,'files.sha256.json'),'utf8'));assert.equal(regenerated.length,Object.keys(input.data).length);assert(!regenerated.some(row=>row.path==='config.json'||row.path==='files.sha256.json'));for(const row of regenerated)assert.equal(row.sha256,sha(input.data[row.path]));
  assert.equal(report.intermediate_copies_retained,false);await assert.rejects(()=>access(join(output,'verify')),{code:'ENOENT'});await assert.rejects(()=>access(join(output,'cab/payload.cab')),{code:'ENOENT'});
  const denied=run(input.bundle,role,output);assert.notEqual(denied.status,0,'Existing output must never be overwritten');assert.match(denied.stderr,/Output directory exists/);assert.equal(sha(await readFile(report.package)),report.package_sha256);
  const noConsentOutput=join(scratch,role+' no development consent');const noConsent=run(input.bundle,role,noConsentOutput,{development:false});assert.notEqual(noConsent.status,0);assert.match(noConsent.stderr,/DevelopmentOnly|development-only/);await assert.rejects(()=>access(noConsentOutput),{code:'ENOENT'});
  const badVersionOutput=join(scratch,role+' invalid numeric version');const badVersion=run(input.bundle,role,badVersionOutput,{msiVersion:'1.2.1'});assert.notEqual(badVersion.status,0);assert.match(badVersion.stderr,/msi_version_does_not_match_source|Version policy verification failed/);await assert.rejects(()=>access(badVersionOutput),{code:'ENOENT'});
  const wrongHash=spawnSync('powershell.exe',['-NoProfile','-NonInteractive','-ExecutionPolicy','Bypass','-File',join(root,'tools/inspect-development-msi.ps1'),'-Package',report.package,'-ExpectedSha256','0'.repeat(64)],{cwd:root,encoding:'utf8',windowsHide:true,timeout:30000});assert.notEqual(wrongHash.status,0);assert.equal(wrongHash.stdout,'');assert.match(wrongHash.stderr,/MSI digest differs from the expected artifact/);
  const mutants=join(output,'inspection-negative');await mkdir(mutants);
  const rejected=[];
  const reasons={'machine-context':/Not the supported development-only property contract/,guard:/Development guard or compressed-media mapping differs/,registry:/Unsupported registry ownership/,directory:/Unexpected authored default installation scope/,'custom-action':/Unsupported MSI custom action/,'component-identity':/Unsupported component ownership/,'ui-position':/Unexpected UI action or guard position/,'version-policy':/MSI version does not match source policy/,'version-policy-missing':/Version policy marker is required/,'version-policy-unknown':/Unsupported MSI version policy/,'source-policy-format':/Invalid source version for MSI policy/,'source-policy-bounds':/Source version exceeds MSI policy bounds/,'source-policy-prerelease':/Source prerelease exceeds MSI policy bounds/,
   'upgrade-policy-missing':/Upgrade policy marker is required/,'upgrade-policy-unknown':/Unsupported MSI upgrade policy/,'upgrade-code':/Unsupported MSI upgrade ranges/,'upgrade-newer-range':/Unsupported MSI upgrade ranges/,'upgrade-older-range':/Unsupported MSI upgrade ranges/,'upgrade-detect-flags':/Unsupported MSI upgrade ranges/,'upgrade-secure':/Unsupported upgrade secure properties/,'upgrade-error-type':/Unsupported MSI custom action/,'upgrade-error-condition':/Unexpected action condition/,'upgrade-remove-position':/Unsupported execute sequence/,'upgrade-rows':/Unsupported MSI upgrade ranges/};
  for(const mutation of Object.keys(reasons)){
   const destination=join(mutants,mutation+'.msi');
   const made=spawnSync('powershell.exe',['-NoProfile','-NonInteractive','-ExecutionPolicy','Bypass','-File',join(root,'tools/msi-test-mutation.ps1'),'-FixtureRoot',scratch,'-Source',report.package,'-ExpectedSha256',report.package_sha256,'-Output',destination,'-Mutation',mutation],{cwd:root,encoding:'utf8',windowsHide:true,timeout:30000});
   await writeFile(join(mutants,mutation+'-mutation.log'),(made.stdout||'')+(made.stderr||''));assert.equal(made.status,0,'Mutation fixture creation failed; inspect '+scratch);
   const refused=spawnSync('powershell.exe',['-NoProfile','-NonInteractive','-ExecutionPolicy','Bypass','-File',join(root,'tools/inspect-development-msi.ps1'),'-Package',destination],{cwd:root,encoding:'utf8',windowsHide:true,timeout:30000});
   await writeFile(join(mutants,mutation+'-refusal.log'),(refused.stdout||'')+(refused.stderr||''));assert.notEqual(refused.status,0,'Unsafe recipe was not refused: '+mutation);assert.equal(refused.stdout,'');assert.match(refused.stderr,reasons[mutation],'The intended structural refusal must be observed');rejected.push(mutation);
  }
  const legacy=spawnSync('powershell.exe',['-NoProfile','-NonInteractive','-ExecutionPolicy','Bypass','-File',join(root,'tools/inspect-development-msi.ps1'),'-Package',join(mutants,'version-policy-missing.msi'),'-AllowLegacyExperimentalVersion'],{cwd:root,encoding:'utf8',windowsHide:true,timeout:30000});
  await writeFile(join(mutants,'legacy-opt-in-inspection.json'),legacy.stdout||'');await writeFile(join(mutants,'legacy-opt-in-stderr.log'),legacy.stderr||'');assert.equal(legacy.status,0);const legacyReport=JSON.parse(legacy.stdout);assert.equal(legacyReport.version_policy,'legacy-explicit-development');assert.equal(legacyReport.version_policy_verified,false);assert.equal(legacyReport.legacy_version_policy_opt_in,true);assert.equal(legacyReport.msi_release_accepted,false);
  const unknown=spawnSync('powershell.exe',['-NoProfile','-NonInteractive','-ExecutionPolicy','Bypass','-File',join(root,'tools/inspect-development-msi.ps1'),'-Package',join(mutants,'version-policy-unknown.msi'),'-AllowLegacyExperimentalVersion'],{cwd:root,encoding:'utf8',windowsHide:true,timeout:30000});assert.notEqual(unknown.status,0);assert.equal(unknown.stdout,'');assert.match(unknown.stderr,/Unsupported MSI version policy/);await writeFile(join(mutants,'unknown-policy-legacy-opt-in-refusal.log'),unknown.stderr||'');
  const unknownUpgrade=spawnSync('powershell.exe',['-NoProfile','-NonInteractive','-ExecutionPolicy','Bypass','-File',join(root,'tools/inspect-development-msi.ps1'),'-Package',join(mutants,'upgrade-policy-unknown.msi'),'-AllowLegacyExperimentalUpgrade'],{cwd:root,encoding:'utf8',windowsHide:true,timeout:30000});assert.notEqual(unknownUpgrade.status,0);assert.equal(unknownUpgrade.stdout,'');assert.match(unknownUpgrade.stderr,/Unsupported MSI upgrade policy/);await writeFile(join(mutants,'unknown-upgrade-policy-legacy-opt-in-refusal.log'),unknownUpgrade.stderr||'');
  assert.equal(sha(await readFile(report.package)),report.package_sha256);assert.equal(rejected.length,24);
  await writeFile(join(output,'inspection-negative-results.json'),JSON.stringify({passed:true,rejected,expected_hash_mismatch_refused:true,original_package_unchanged:true,installation_attempted:false},null,2));
  reports.push({...report,output});cases.push({role,authoring_passed:true,source_unchanged:true,regenerated_inventory_verified:true,larger_than_stream_chunk:true,existing_output_preserved:true});
 }
 assert.notEqual(reports[0].product_code,reports[1].product_code);assert.notEqual(reports[0].upgrade_code,reports[1].upgrade_code);assert.notEqual(reports[0].install_directory_leaf,reports[1].install_directory_leaf);
 // Construct and inspect an intentional-failure variant without installing it.
 // Native rollback acceptance is a separate standard-user, explicitly opted-in test.
 await writeFile(join(scratch,'rollback-probe-intent.json'),JSON.stringify({payload_kind:'synthetic-non-executable-only',namespace,sources:reports},null,2));
 const rollbackChecks=[];
 for(const report of reports){
  const probePath=join(report.output,report.role+'-synthetic-rollback.msi'),probeTool=join(root,'tools/msi-upgrade-rollback-fixture.ps1');
  const probeArgs=['-FixtureRoot',scratch,'-Source',report.package,'-SourceSha256',report.package_sha256,'-Package',probePath];
  const made=spawnSync('powershell.exe',['-NoProfile','-NonInteractive','-File',probeTool,'-Action','create',...probeArgs],{cwd:root,encoding:'utf8',windowsHide:true,timeout:120000,maxBuffer:2*1024*1024});
  await writeFile(join(report.output,'rollback-probe-authoring.log'),(made.stdout||'')+'\n'+(made.stderr||''));assert.equal(made.status,0,'Synthetic rollback authoring/readback failed: '+scratch);const probe=JSON.parse(made.stdout);assert.equal(probe.synthetic_only,true);assert.equal(probe.synthetic_rollback_recipe_verified,true);assert.equal(probe.ordinary_recipe_verified,false);assert.equal(probe.installation_attempted,false);assert.equal(probe.payload_launched,false);assert.notEqual(probe.package_code,report.package_code);assert.equal(probe.cabinet_sha256,report.cabinet_sha256);
  const refused=spawnSync('powershell.exe',['-NoProfile','-NonInteractive','-File',join(root,'tools/inspect-development-msi.ps1'),'-Package',probePath],{cwd:root,encoding:'utf8',windowsHide:true,timeout:30000});assert.notEqual(refused.status,0);assert.equal(refused.stdout,'');assert.match(refused.stderr,/Not the supported development-only property contract/);await writeFile(join(report.output,'rollback-probe-ordinary-refusal.log'),refused.stderr||'');
  const reasons={'rollback-execute-position':/Unexpected rollback probe table difference: InstallExecuteSequence/,'rollback-failure-condition':/Unexpected rollback probe table difference: InstallExecuteSequence/,'rollback-failure-type':/Unexpected rollback probe table difference: CustomAction/,'rollback-file-size':/Unexpected rollback probe table difference: File/,'rollback-package-code':/Rollback probe requires its own package code/,'rollback-guard':/Unexpected rollback probe table difference: LaunchCondition/};
  const rejected=[];
  for(const [mutation,reason] of Object.entries(reasons)){
   const changedDirectory=join(report.output,'inspection-negative',mutation);await mkdir(changedDirectory);const changedPath=join(changedDirectory,report.role+'-synthetic-rollback.msi');
   const changed=spawnSync('powershell.exe',['-NoProfile','-NonInteractive','-File',join(root,'tools/msi-test-mutation.ps1'),'-FixtureRoot',scratch,'-Source',probePath,'-ExpectedSha256',probe.package_sha256,'-Output',changedPath,'-Mutation',mutation],{cwd:root,encoding:'utf8',windowsHide:true,timeout:30000});assert.equal(changed.status,0,'Rollback mutation failed: '+mutation);
   const inspected=spawnSync('powershell.exe',['-NoProfile','-NonInteractive','-File',probeTool,'-Action','inspect','-FixtureRoot',scratch,'-Source',report.package,'-SourceSha256',report.package_sha256,'-Package',changedPath],{cwd:root,encoding:'utf8',windowsHide:true,timeout:30000});
   // Output names are part of the exact probe contract. Mutations use the same
   // leaf in their own directory, retaining the original immutable probe.
   await writeFile(join(report.output,'inspection-negative',mutation+'-rollback-refusal.log'),(inspected.stdout||'')+'\n'+(inspected.stderr||''));assert.notEqual(inspected.status,0);assert.equal(inspected.stdout,'');assert.match(inspected.stderr,reason);rejected.push(mutation);
  }
  for(const [label,sourceHash,packageHash,reason] of [['source-hash','0'.repeat(64),probe.package_sha256,/Canonical source changed/],['package-hash',report.package_sha256,'0'.repeat(64),/Rollback probe digest differs/]]){
   const checked=spawnSync('powershell.exe',['-NoProfile','-NonInteractive','-File',probeTool,'-Action','inspect','-FixtureRoot',scratch,'-Source',report.package,'-SourceSha256',sourceHash,'-Package',probePath,'-ExpectedSha256',packageHash],{cwd:root,encoding:'utf8',windowsHide:true,timeout:30000});assert.notEqual(checked.status,0);assert.equal(checked.stdout,'');assert.match(checked.stderr,reason);await writeFile(join(report.output,'inspection-negative','rollback-'+label+'-refusal.log'),checked.stderr||'');rejected.push(label);
  }
  assert.equal(rejected.length,8);assert.equal(sha(await readFile(probePath)),probe.package_sha256);assert.equal(sha(await readFile(report.package)),report.package_sha256);rollbackChecks.push({role:report.role,probe,ordinary_inspector_refused:true,rejected,installation_attempted:false});
 }
 const boundaries=[];
 for(const [sourceVersion,msiVersion] of [['0.4.0-alpha.63','0.4.63'],['0.4.0-beta.1','0.4.65'],['0.4.0-rc.63','0.4.191'],['0.4.0','0.4.255'],['0.4.1-alpha.1','0.4.257'],['255.255.255','255.255.65535']]){
  const input=await fixture('eef',sourceVersion,'boundary-'+boundaries.length),output=join(scratch,'boundary-output-'+boundaries.length);
  const authored=run(input.bundle,'eef',output,{sourceVersion,msiVersion:null});await writeFile(join(scratch,'boundary-'+boundaries.length+'-stdout.json'),authored.stdout||'');await writeFile(join(scratch,'boundary-'+boundaries.length+'-stderr.log'),authored.stderr||'');assert.equal(authored.status,0,'Boundary authoring failed; inspect '+scratch);const report=JSON.parse(authored.stdout);assert.equal(report.msi_version,msiVersion);assert.equal(report.version_policy_verified,true);assert.equal(report.installation_attempted,false);
  const readback=spawnSync('powershell.exe',['-NoProfile','-NonInteractive','-ExecutionPolicy','Bypass','-File',join(root,'tools/inspect-development-msi.ps1'),'-Package',report.package,'-ExpectedSha256',report.package_sha256],{cwd:root,encoding:'utf8',windowsHide:true,timeout:30000});await writeFile(join(scratch,'boundary-'+boundaries.length+'-inspection.json'),readback.stdout||'');await writeFile(join(scratch,'boundary-'+boundaries.length+'-inspection-stderr.log'),readback.stderr||'');assert.equal(readback.status,0,'Independent policy boundary failed; inspect '+scratch);const inspected=JSON.parse(readback.stdout);assert.equal(inspected.msi_version,msiVersion);assert.equal(inspected.source_version,sourceVersion);assert.equal(inspected.version_policy_verified,true);assert.equal(inspected.legacy_version_policy_opt_in,false);boundaries.push({sourceVersion,msiVersion,passed:true});
 }
 await writeFile(join(scratch,'results.json'),JSON.stringify({passed:true,cases,reports,namespace,independent_database_readback_verified:true,version_policy_boundaries:boundaries,version_policy_negative_recipes_per_role:13,upgrade_negative_recipes_per_role:11,total_negative_recipes_per_role:24,rollback_probe_checks:rollbackChecks,rollback_probe_negative_recipes_per_role:8,msi_installed:false,payload_launched:false,stable_release_accepted:false},null,2));
 assert.equal(boundaries.length,6);console.log('MSI version policy native boundaries checked: '+boundaries.length);
 console.log('MSI authoring rejected recipes per role: 24');
 console.log('Synthetic MSI rollback probe rejected recipes per role: 8');
 console.log('Development MSI authoring checks passed: '+scratch);
}catch(error){await writeFile(join(scratch,'failure.json'),JSON.stringify({message:String(error)},null,2));throw error;}
