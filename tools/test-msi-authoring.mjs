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
async function fixture(role){
 const bundle=join(scratch,role+" source's files");await mkdir(bundle);
 const metadata={name:role,version:'1.2.3-alpha.1',architecture:'x86_64-windows',runtime:'rust',python:'synthetic',...(role==='eefn'?{models_included:false}:{})};
 const data={'bundle.json':Buffer.from(JSON.stringify(metadata)),[role+'.exe']:Buffer.from('Synthetic payload. NEVER EXECUTE.'),'python/python.exe':Buffer.from('Synthetic Python. NEVER EXECUTE.'),'nested/level/LICENSE.txt':Buffer.from('UTF-8 payload bytes: สวัสดี'),'library++.dll':randomBytes(192*1024)};
 if(role==='eefn')data['tools/llama-server.exe']=Buffer.from('Synthetic inference runtime. NEVER EXECUTE.');
 const rows=[];for(const [path,bytes] of Object.entries(data)){const absolute=join(bundle,path);await mkdir(dirname(absolute),{recursive:true});await writeFile(absolute,bytes);rows.push({path,bytes:bytes.length,sha256:sha(bytes)});}
 await writeFile(join(bundle,'config.json'),'PRIVATE-owner-sentinel-must-not-be-packaged');rows.push({path:'config.json',bytes:41,sha256:'1'.repeat(64)});
 const inventory=Buffer.from(JSON.stringify(rows));await writeFile(join(bundle,'files.sha256.json'),inventory);
 return {bundle,data,inventory};
}
function run(bundle,role,output,{development=true}={}){
 const args=['-NoProfile','-NonInteractive','-ExecutionPolicy','Bypass','-File',controller,'-BundleDir',bundle,'-Role',role,'-ExpectedVersion','1.2.3-alpha.1','-MsiVersion','1.2.1','-ProjectNamespace',namespace,'-OutputDirectory',output,'-NodePath',process.execPath];if(development)args.push('-DevelopmentOnly');
 return spawnSync('powershell.exe',args,{cwd:root,encoding:'utf8',windowsHide:true,timeout:120000,maxBuffer:2*1024*1024});
}
const reports=[];
try{
 for(const role of ['eef','eefn']){
  const input=await fixture(role),output=join(scratch,role+' authored output');
  const result=run(input.bundle,role,output);
  await writeFile(join(scratch,role+'-stdout.log'),result.stdout||'');await writeFile(join(scratch,role+'-stderr.log'),result.stderr||'');
  assert.equal(result.status,0,'Authoring failed; inspect retained fixture '+scratch);
  const report=JSON.parse(result.stdout),saved=JSON.parse(await readFile(join(output,'authoring-results.json'),'utf8'));assert.deepEqual(report,saved);
  const inspected=spawnSync('powershell.exe',['-NoProfile','-NonInteractive','-ExecutionPolicy','Bypass','-File',join(root,'tools/inspect-development-msi.ps1'),'-Package',report.package,'-ExpectedSha256',report.package_sha256,'-Detailed'],{cwd:root,encoding:'utf8',windowsHide:true,timeout:120000,maxBuffer:2*1024*1024});
  await writeFile(join(scratch,role+'-inspection-stdout.json'),inspected.stdout||'');await writeFile(join(scratch,role+'-inspection-stderr.log'),inspected.stderr||'');assert.equal(inspected.status,0,'Independent database readback failed; inspect retained fixture '+scratch);
  const inspection=JSON.parse(inspected.stdout);assert.equal(inspection.success,true);assert.equal(inspection.read_only,true);assert.equal(inspection.file_count,report.files);assert.equal(inspection.component_count,report.files);assert.equal(inspection.registry_count,report.files);assert.equal(inspection.product_code,report.product_code);assert.equal(inspection.upgrade_code,report.upgrade_code);assert.equal(inspection.package_code,report.package_code);assert.equal(inspection.package_sha256,report.package_sha256);assert.equal(inspection.cabinet_sha256,report.cabinet_sha256);assert.equal(inspection.cabinet_bytes,report.cabinet_bytes);
  const plan=JSON.parse(await readFile(join(output,'payload-plan.json'),'utf8'));
  const expectedFiles=[...plan.files,plan.regenerated_inventory];assert.deepEqual(inspection.files.map(row=>row.path).sort(),expectedFiles.map(row=>row.path).sort());
  for(const file of inspection.files){const expected=expectedFiles.find(row=>row.path===file.path);for(const key of ['file_id','component_id','component_code','directory_id'])assert.equal(file[key],expected[key],key);}
  assert.equal(inspection.payload_bytes,Object.values(input.data).reduce((sum,bytes)=>sum+bytes.length,0)+(await readFile(join(output,'files.sha256.json'))).length);
  for(const key of ['authored','development_only','every_cabinet_file_verified','embedded_cabinet_verified','package_format_verified'])assert.equal(report[key],true,key);
  for(const key of ['installation_attempted','application_launched','mutable_configuration_packaged','models_downloaded','custom_actions_authored','startup_choices_implemented','upgrade_migration_verified','real_application_rollback_verified','full_ice_validation','signing_defender_verified','version_ordering_approved','msi_release_accepted','stable_release_accepted'])assert.equal(report[key],false,key);
  assert.equal(report.role,role);assert.equal(report.source_version,'1.2.3-alpha.1');assert.equal(report.msi_version,'1.2.1');assert.equal(report.files,Object.keys(input.data).length+1);
  assert(report.cabinet_bytes>65536,'Exercise more than one embedded-stream read');
  const packageBytes=await readFile(report.package);assert.equal(packageBytes.length,report.package_bytes);assert.equal(sha(packageBytes),report.package_sha256);
  assert.deepEqual(await readFile(join(input.bundle,'files.sha256.json')),input.inventory);assert.equal(await readFile(join(input.bundle,'config.json'),'utf8'),'PRIVATE-owner-sentinel-must-not-be-packaged');
  for(const [path,bytes] of Object.entries(input.data))assert.deepEqual(await readFile(join(input.bundle,path)),bytes);
  const regenerated=JSON.parse(await readFile(join(output,'files.sha256.json'),'utf8'));assert.equal(regenerated.length,Object.keys(input.data).length);assert(!regenerated.some(row=>row.path==='config.json'||row.path==='files.sha256.json'));for(const row of regenerated)assert.equal(row.sha256,sha(input.data[row.path]));
  assert.equal(report.intermediate_copies_retained,false);await assert.rejects(()=>access(join(output,'verify')),{code:'ENOENT'});await assert.rejects(()=>access(join(output,'cab/payload.cab')),{code:'ENOENT'});
  const denied=run(input.bundle,role,output);assert.notEqual(denied.status,0,'Existing output must never be overwritten');assert.match(denied.stderr,/Output directory exists/);assert.equal(sha(await readFile(report.package)),report.package_sha256);
  const noConsentOutput=join(scratch,role+' no development consent');const noConsent=run(input.bundle,role,noConsentOutput,{development:false});assert.notEqual(noConsent.status,0);assert.match(noConsent.stderr,/DevelopmentOnly|development-only/);await assert.rejects(()=>access(noConsentOutput),{code:'ENOENT'});
  const wrongHash=spawnSync('powershell.exe',['-NoProfile','-NonInteractive','-ExecutionPolicy','Bypass','-File',join(root,'tools/inspect-development-msi.ps1'),'-Package',report.package,'-ExpectedSha256','0'.repeat(64)],{cwd:root,encoding:'utf8',windowsHide:true,timeout:30000});assert.notEqual(wrongHash.status,0);assert.equal(wrongHash.stdout,'');assert.match(wrongHash.stderr,/MSI digest differs from the expected artifact/);
  const mutants=join(output,'inspection-negative');await mkdir(mutants);
  const rejected=[];
  const reasons={'machine-context':/Not the supported development-only property contract/,guard:/Development guard or compressed-media mapping differs/,registry:/Unsupported registry ownership/,directory:/Unexpected authored default installation scope/,'custom-action':/Unsupported MSI table in this development recipe/,'component-identity':/Unsupported component ownership/,'ui-position':/Unexpected UI action or guard position/};
  for(const mutation of ['machine-context','guard','registry','directory','custom-action','component-identity','ui-position']){
   const destination=join(mutants,mutation+'.msi');
   const made=spawnSync('powershell.exe',['-NoProfile','-NonInteractive','-ExecutionPolicy','Bypass','-File',join(root,'tools/msi-test-mutation.ps1'),'-FixtureRoot',scratch,'-Source',report.package,'-ExpectedSha256',report.package_sha256,'-Output',destination,'-Mutation',mutation],{cwd:root,encoding:'utf8',windowsHide:true,timeout:30000});
   await writeFile(join(mutants,mutation+'-mutation.log'),(made.stdout||'')+(made.stderr||''));assert.equal(made.status,0,'Mutation fixture creation failed; inspect '+scratch);
   const refused=spawnSync('powershell.exe',['-NoProfile','-NonInteractive','-ExecutionPolicy','Bypass','-File',join(root,'tools/inspect-development-msi.ps1'),'-Package',destination],{cwd:root,encoding:'utf8',windowsHide:true,timeout:30000});
   await writeFile(join(mutants,mutation+'-refusal.log'),(refused.stdout||'')+(refused.stderr||''));assert.notEqual(refused.status,0,'Unsafe recipe was not refused: '+mutation);assert.equal(refused.stdout,'');assert.match(refused.stderr,reasons[mutation],'The intended structural refusal must be observed');rejected.push(mutation);
  }
  assert.equal(sha(await readFile(report.package)),report.package_sha256);assert.equal(rejected.length,7);
  await writeFile(join(output,'inspection-negative-results.json'),JSON.stringify({passed:true,rejected,expected_hash_mismatch_refused:true,original_package_unchanged:true,installation_attempted:false},null,2));
  reports.push({...report,output});cases.push({role,authoring_passed:true,source_unchanged:true,regenerated_inventory_verified:true,larger_than_stream_chunk:true,existing_output_preserved:true});
 }
 assert.notEqual(reports[0].product_code,reports[1].product_code);assert.notEqual(reports[0].upgrade_code,reports[1].upgrade_code);assert.notEqual(reports[0].install_directory_leaf,reports[1].install_directory_leaf);
 await writeFile(join(scratch,'results.json'),JSON.stringify({passed:true,cases,reports,namespace,independent_database_readback_verified:true,msi_installed:false,payload_launched:false,stable_release_accepted:false},null,2));
 console.log('Development MSI authoring checks passed: '+scratch);
}catch(error){await writeFile(join(scratch,'failure.json'),JSON.stringify({message:String(error)},null,2));throw error;}
