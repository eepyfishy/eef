// Standalone synthetic planner regression; no payload is launched or installed.
import assert from 'node:assert/strict';
import {mkdtemp,mkdir,writeFile,readFile,lstat,symlink} from 'node:fs/promises';
import {join,dirname,resolve} from 'node:path';
import {createHash} from 'node:crypto';
import {spawnSync} from 'node:child_process';
import {planPayload,uuidV5} from './msi-payload-plan.mjs';
const workspace=resolve(import.meta.dirname,'..');
await mkdir(join(workspace,'.validation'),{recursive:true});
const root=await mkdtemp(join(workspace,'.validation','msi-payload-plan-')),cases=[];
const namespace='15cb0ac4-08b1-4a6e-a318-927804566590',version='1.2.3-alpha.1';
const opts=role=>({role,namespace,msiVersion:'1.2.769',expectedVersion:version});
const sha=bytes=>createHash('sha256').update(bytes).digest('hex');
async function fixture(name,role='eefn',{junction=false}={}){
 const bundle=join(root,name);await mkdir(bundle);
 const manifest={name:role,version,architecture:'x86_64-windows',runtime:'rust',python:'synthetic',...(role==='eefn'?{models_included:false}:{})};
 const values={'bundle.json':JSON.stringify(manifest),[role+'.exe']:'synthetic application; never execute','python/python.exe':'synthetic Python; never execute','LICENSE.txt':'synthetic license'};
 if(role==='eefn')values['tools/llama-server.exe']='synthetic runtime; never execute';
 if(junction){const outside=join(root,name+'-external-python');await mkdir(outside);await writeFile(join(outside,'python.exe'),values['python/python.exe']);await symlink(outside,join(bundle,'python'),'junction');}
 for(const [path,value] of Object.entries(values)){if(junction&&path.startsWith('python/'))continue;const absolute=join(bundle,path);await mkdir(dirname(absolute),{recursive:true});await writeFile(absolute,value);}
 // An inventoried mutable config is deliberately a DIRECTORY, not readable
 // configuration bytes. The plan must omit it before any file-type/hash read.
 await mkdir(join(bundle,'config.json'));await writeFile(join(bundle,'config.json','PRIVATE-owner.txt'),'NEVER_PACKAGE_THIS_OWNER_SENTINEL');
 const rows=Object.entries(values).map(([path,value])=>({path,bytes:Buffer.byteLength(value),sha256:sha(value)}));
 rows.push({path:'config.json',bytes:37,sha256:'1'.repeat(64)});
 await writeFile(join(bundle,'files.sha256.json'),JSON.stringify(rows));return {bundle,rows,values};
}
async function test(name,fn){await fn();cases.push({name,passed:true});}
async function changed(name,mutate,pattern,role='eefn'){
 const item=await fixture(name,role);await mutate(item);await assert.rejects(()=>planPayload(item.bundle,opts(role)),pattern);
}
const one=await fixture("one bundle's files"),two=await fixture('second-root');
let first,second;
await test('rfc9562-uuidv5-vector',async()=>assert.equal(uuidV5('6ba7b810-9dad-11d1-80b4-00c04fd430c8','www.example.com'),'{2ED6657D-E927-568B-95E1-2665A8AEA6A2}'));
await test('read-only-deterministic-and-owner-configuration-omitted',async()=>{
 const inventory=await readFile(join(one.bundle,'files.sha256.json'));first=await planPayload(one.bundle,opts('eefn'));second=await planPayload(two.bundle,opts('eefn'));
 assert.deepEqual(first,second);assert.equal(first.read_only,true);assert.equal(first.omissions.mutable_configuration,1);
 assert.equal(first.version_policy,'packed-patch-v1');assert.equal(first.version_policy_verified,true);assert.equal(first.source_is_prerelease,true);
 assert.equal(first.files.some(row=>row.path==='config.json'),false);assert.equal(first.files.some(row=>row.path==='files.sha256.json'),false);
 assert.equal(first.requires_new_distribution_inventory,true);assert.equal(first.source_inventory_not_copied,true);
 assert.equal(first.regenerated_inventory.path,'files.sha256.json');assert.equal(first.regenerated_inventory.requires_generation,true);
 assert.equal(first.regenerated_inventory.directory_id,'INSTALLDIR');
 assert(!first.files.some(row=>row.component_code===first.regenerated_inventory.component_code||row.file_id===first.regenerated_inventory.file_id));
 assert((await lstat(join(one.bundle,'config.json'))).isDirectory());assert.equal(await readFile(join(one.bundle,'config.json','PRIVATE-owner.txt'),'utf8'),'NEVER_PACKAGE_THIS_OWNER_SENTINEL');
 assert.deepEqual(await readFile(join(one.bundle,'files.sha256.json')),inventory);
 const output=JSON.stringify(first);assert(!output.includes(root));assert(!output.includes('NEVER_PACKAGE_THIS_OWNER_SENTINEL'));assert(!output.includes('1'.repeat(64)));
 assert.equal(first.selected_bytes,first.files.reduce((total,row)=>total+row.bytes,0));
 assert.equal(new Set(first.files.map(row=>row.file_id)).size,first.files.length);
 assert.equal(new Set(first.files.map(row=>row.component_code)).size,first.files.length);
 for(const row of first.files){assert.match(row.component_code,/^\{[A-F0-9]{8}-[A-F0-9]{4}-5[A-F0-9]{3}-[89AB][A-F0-9]{3}-[A-F0-9]{12}\}$/);assert(first.directories.some(dir=>dir.id===row.directory_id));}
 for(const key of ['package_authored','publisher_trust_verified','disk_preflight_complete','application_launch_verified','backend_initialization_compatibility_verified','version_ordering_approved','upgrade_migration_verified','signing_or_defender_verified','real_installer_accepted','stable_release_accepted'])assert.equal(first[key],false,key);
});
await test('separate-role-identities',async()=>{const item=await fixture('coordinator','eef'),plan=await planPayload(item.bundle,opts('eef'));assert.notEqual(plan.upgrade_code,first.upgrade_code);assert.notEqual(plan.product_code,first.product_code);assert(!plan.files.some(row=>row.path==='tools/llama-server.exe'));});
await test('fork-namespace-changes-identities',async()=>{const other=await planPayload(one.bundle,{...opts('eefn'),namespace:'9f676f62-fca6-4bdd-a651-339c98824c6f'});assert.notEqual(other.upgrade_code,first.upgrade_code);assert.notEqual(other.files[0].component_code,first.files[0].component_code);assert.notEqual(other.regenerated_inventory.component_code,first.regenerated_inventory.component_code);});
await test('renaming-root-does-not-change-identities',async()=>assert.deepEqual(first.files,second.files));
await test('full-file-hash-change-refused',()=>changed('hash-change',async({bundle})=>writeFile(join(bundle,'eefn.exe'),'different but same file owner'),/payload_size_mismatch|payload_digest_mismatch/));
await test('same-size-byte-change-refused',()=>changed('same-size',async({bundle,values})=>writeFile(join(bundle,'eefn.exe'),'X'.repeat(values['eefn.exe'].length)),/payload_digest_mismatch/));
await test('missing-file-refused',()=>changed('missing',async({bundle,rows})=>{rows.push({path:'missing.dll',bytes:1,sha256:sha('x')});await writeFile(join(bundle,'files.sha256.json'),JSON.stringify(rows));},/ENOENT/));
await test('duplicate-case-file-refused',()=>changed('case-file',async({bundle,rows})=>{rows.push({...rows.find(row=>row.path==='LICENSE.txt'),path:'license.TXT'});await writeFile(join(bundle,'files.sha256.json'),JSON.stringify(rows));},/case_aliased_inventory_path/));
await test('duplicate-case-directory-refused',()=>changed('case-directory',async({bundle,rows})=>{rows.push({path:'Python/another.dll',bytes:1,sha256:sha('x')});await writeFile(join(bundle,'files.sha256.json'),JSON.stringify(rows));},/case_aliased_inventory_directory/));
for(const [label,path] of [['traversal','../outside.txt'],['drive','C:/private.txt'],['ads','LICENSE.txt:stream'],['backslash','python\\private.dll'],['reserved','NUL.txt'],['trailing-dot','file.']])await test('unsafe-path-'+label,()=>changed('unsafe-'+label,async({bundle,rows})=>{rows.unshift({path,bytes:1,sha256:sha('x')});await writeFile(join(bundle,'files.sha256.json'),JSON.stringify(rows));},/inventory_path_unsafe/));
await test('metadata-size-bounded',()=>changed('large-metadata',async({bundle})=>writeFile(join(bundle,'files.sha256.json'),' '.repeat(4*1024*1024+1)),/metadata_size_invalid/));
await test('future-inventory-row-refused',()=>changed('future-row',async({bundle,rows})=>{rows[0].extra=true;await writeFile(join(bundle,'files.sha256.json'),JSON.stringify(rows));},/inventory_row_invalid/));
await test('model-weights-refused',()=>changed('weight',async({bundle,rows})=>{rows.push({path:'models/model.gguf',bytes:1,sha256:sha('x')});await writeFile(join(bundle,'files.sha256.json'),JSON.stringify(rows));},/model_weights_refused/));
await test('selected-junction-refused',async()=>{const item=await fixture('junction','eefn',{junction:true});await assert.rejects(()=>planPayload(item.bundle,opts('eefn')),/payload_link_refused/);});
await test('wrong-role-refused',async()=>assert.rejects(()=>planPayload(one.bundle,opts('eef')),/bundle_role_architecture_invalid/));
await test('old-bundle-cannot-be-relabeled',async()=>assert.rejects(()=>planPayload(one.bundle,{...opts('eefn'),expectedVersion:'1.2.3',msiVersion:'1.2.1023'}),/source_version_must_match_explicitly/));
await test('arbitrary-numeric-msi-version-refused',async()=>assert.rejects(()=>planPayload(one.bundle,{...opts('eefn'),msiVersion:'1.2.1'}),/msi_version_does_not_match_source/));
await test('implicit-msi-version-refused',async()=>assert.rejects(()=>planPayload(one.bundle,{...opts('eefn'),msiVersion:undefined}),/explicit_three_field_msi_version_required/));
await test('alpha-suffix-msi-version-refused',async()=>assert.rejects(()=>planPayload(one.bundle,{...opts('eefn'),msiVersion:'1.2.3-alpha.1'}),/explicit_three_field_msi_version_required/));
await test('msi-version-limit-refused',async()=>assert.rejects(()=>planPayload(one.bundle,{...opts('eefn'),msiVersion:'1.2.65536'}),/msi_version_out_of_range/));
await test('implicit-project-identity-refused',async()=>assert.rejects(()=>planPayload(one.bundle,{...opts('eefn'),namespace:undefined}),/project_namespace_required/));
await test('unc-input-refused-before-filesystem-lookup',async()=>assert.rejects(()=>planPayload('\\\\nonexistent.invalid\\payload',opts('eefn')),/bundle_must_be_local/));
await test('legitimate-cxx-runtime-name-accepted',async()=>{const item=await fixture('cxx-runtime');await writeFile(join(item.bundle,'libc++.dll'),'synthetic library; never execute');item.rows.push({path:'libc++.dll',bytes:Buffer.byteLength('synthetic library; never execute'),sha256:sha('synthetic library; never execute')});await writeFile(join(item.bundle,'files.sha256.json'),JSON.stringify(item.rows));const plan=await planPayload(item.bundle,opts('eefn'));assert(plan.files.some(row=>row.filename==='libc++.dll'));});
await test('generated-cache-omitted-only-with-verified-source',async()=>{
 const item=await fixture('generated-cache'),source='python/Lib/site-packages/example.py',cache='python/Lib/site-packages/__pycache__/example.cpython-311.pyc';
 await mkdir(dirname(join(item.bundle,source)),{recursive:true});await writeFile(join(item.bundle,source),'value = 1\n');
 await mkdir(join(item.bundle,cache),{recursive:true});await writeFile(join(item.bundle,cache,'PRIVATE-owner-cache'),'never read a mutable cache');
 item.rows.push({path:source,bytes:10,sha256:sha('value = 1\n')},{path:cache,bytes:123,sha256:'1'.repeat(64)});
 await writeFile(join(item.bundle,'files.sha256.json'),JSON.stringify(item.rows));const plan=await planPayload(item.bundle,opts('eefn'));
 assert.equal(plan.omissions.generated_python_cache,1);assert(plan.files.some(row=>row.path===source));assert(!plan.files.some(row=>row.path===cache));
 assert.equal(plan.generated_python_caches_not_owned,true);assert.equal(plan.omitted_caches_have_verified_source,true);
 await writeFile(join(item.bundle,source),'value = 2\n');await assert.rejects(()=>planPayload(item.bundle,opts('eefn')),/payload_digest_mismatch/);
});
await test('orphan-bytecode-cache-not-silently-dropped',()=>changed('orphan-cache',async({bundle,rows})=>{rows.push({path:'python/Lib/__pycache__/orphan.cpython-311.pyc',bytes:1,sha256:sha('x')});await writeFile(join(bundle,'files.sha256.json'),JSON.stringify(rows));},/generated_cache_source_missing/));
await test('non-cache-content-not-silently-dropped',()=>changed('malformed-cache',async({bundle,rows})=>{rows.push({path:'python/Lib/__pycache__/unexpected.exe',bytes:1,sha256:sha('x')});await writeFile(join(bundle,'files.sha256.json'),JSON.stringify(rows));},/generated_cache_layout_unsupported/));
await test('cli-help-is-explicit-about-scope',async()=>{const result=spawnSync(process.execPath,[join(import.meta.dirname,'msi-payload-plan.mjs'),'--help'],{encoding:'utf8',windowsHide:true,timeout:10000});assert.equal(result.status,0,result.stderr);assert.match(result.stdout,/Read-only experimental plan/);});
await test('cli-plan-matches-library',async()=>{const result=spawnSync(process.execPath,[join(import.meta.dirname,'msi-payload-plan.mjs'),'--bundle',one.bundle,'--role','eefn','--namespace',namespace,'--expected-version',version,'--msi-version','1.2.769'],{encoding:'utf8',windowsHide:true,timeout:10000});assert.equal(result.status,0,result.stderr);assert.deepEqual(JSON.parse(result.stdout),first);});
await test('cli-file-errors-do-not-disclose-host-path',async()=>{const missing=join(root,'PRIVATE-missing-input');const result=spawnSync(process.execPath,[join(import.meta.dirname,'msi-payload-plan.mjs'),'--bundle',missing,'--role','eefn','--namespace',namespace,'--expected-version',version,'--msi-version','1.2.769'],{encoding:'utf8',windowsHide:true,timeout:10000});assert.equal(result.status,1);assert.equal(result.stderr.trim(),'payload_file_unavailable');assert(!result.stderr.includes(root));assert.equal(result.stdout,'');});
assert.equal(cases.length,35,'Keep the independently audited case count explicit');
await writeFile(join(root,'results.json'),JSON.stringify({passed:true,cases,case_count:cases.length,read_only_payload_planning:true,owner_configuration_omitted_without_reading:true,deterministic_project_scoped_identities:true,files_copied:false,msi_authored:false,application_launched:false,real_installer_accepted:false,stable_release_accepted:false},null,2));
console.log('MSI payload planning case count: '+cases.length);
console.log('MSI payload planning checks passed: '+root);
