// Experimental read-only MSI payload/identity planning, NOT package authoring.
// No installs, copies, downloads, current.txt changes, permissions or model work.
import {open,lstat,realpath} from 'node:fs/promises';
import {createHash} from 'node:crypto';
import {resolve,join,relative,sep,isAbsolute} from 'node:path';
import {pathToFileURL} from 'node:url';
import {parseArgs} from 'node:util';
const MAX_INVENTORY=4*1024*1024,MAX_FILES=20000,MAX_TOTAL=2*1024*1024*1024;
function requireValue(value,code){if(!value)throw Error(code);}
function digest(bytes){return createHash('sha256').update(bytes).digest('hex');}
function safePath(value){
 requireValue(typeof value==='string'&&value.length>0&&value.length<=240,'inventory_path_invalid');
 requireValue(!value.includes('\\')&&!value.startsWith('/')&&value.split('/').every(part=>
  /^[A-Za-z0-9_. +()-]+$/.test(part)&&part!=='.'&&part!=='..'&&!/[. ]$/.test(part)&&
  !/^(?:CON|PRN|AUX|NUL|COM[0-9]|LPT[0-9])(?:\.|$)/i.test(part)),'inventory_path_unsafe');
 return value;
}
function excluded(path){
 const name=path.toLowerCase(),top=name.split('/')[0];
 if(top==='config'||name==='config.json')return 'mutable_configuration';
 if(['models','data','logs','state','versions'].includes(top)||name==='current.txt'||top.startsWith('.'))return 'mutable_runtime_data';
 return null;
}
function generatedPythonSource(path){
 const parts=path.split('/'),index=parts.indexOf('__pycache__');
 if(index<0||parts[0]!=='python')return null;
 const match=parts.at(-1).match(/^(.+)\.cpython-[0-9]{2,3}(?:\.opt-[0-9]+)?\.pyc$/);
 requireValue(index===parts.length-2&&match,'generated_cache_layout_unsupported');
 return [...parts.slice(0,index),match[1]+'.py'].join('/');
}
export function uuidV5(namespace,name){
 requireValue(/^[a-f0-9]{8}-[a-f0-9]{4}-[a-f0-9]{4}-[a-f0-9]{4}-[a-f0-9]{12}$/i.test(namespace),'project_namespace_required');
 const bytes=createHash('sha1').update(Buffer.from(namespace.replaceAll('-',''),'hex')).update(name).digest().subarray(0,16);
 bytes[6]=(bytes[6]&15)|0x50;bytes[8]=(bytes[8]&63)|0x80;
 const hex=bytes.toString('hex').toUpperCase();
 return '{'+[hex.slice(0,8),hex.slice(8,12),hex.slice(12,16),hex.slice(16,20),hex.slice(20)].join('-')+'}';
}
async function regular(root,path){
 let current=root;
 for(const [index,part] of path.split('/').entries()){
  current=join(current,part);const entry=await lstat(current);
  requireValue(!entry.isSymbolicLink(),'payload_link_refused');
  requireValue(index===path.split('/').length-1?entry.isFile():entry.isDirectory(),'payload_type_invalid');
 }
 const actual=await realpath(current),inside=relative(root,actual);
 requireValue(inside!==''&&!isAbsolute(inside)&&!inside.startsWith('..'+sep)&&inside!=='..'&&!resolve(actual).startsWith('\\\\'),'payload_outside_root');
 return current;
}
async function metadata(root,path,limit){
 const location=await regular(root,path),file=await open(location,'r');
 try{const before=await file.stat();requireValue(before.isFile()&&before.size<=limit,'metadata_size_invalid');
  const buffer=Buffer.alloc(limit+1);let count=0;
  for(;;){requireValue(count<=limit,'metadata_size_invalid');const {bytesRead}=await file.read(buffer,count,buffer.length-count,null);if(!bytesRead)break;count+=bytesRead;}
  const after=await file.stat();requireValue(count===before.size&&after.size===before.size&&after.mtimeMs===before.mtimeMs,'metadata_changed');return buffer.subarray(0,count);
 }finally{await file.close();}
}
function parseJSON(bytes,code){try{return JSON.parse(bytes.toString('utf8').replace(/^\uFEFF/,''));}catch{throw Error(code);}}
async function hashFile(root,row){
 const location=await regular(root,row.path),file=await open(location,'r');
 try{
  const before=await file.stat();requireValue(before.isFile()&&before.size===row.bytes,'payload_size_mismatch');
  const hash=createHash('sha256'),buffer=Buffer.alloc(256*1024);let count=0;
  for(;;){const {bytesRead}=await file.read(buffer,0,buffer.length,null);if(!bytesRead)break;count+=bytesRead;requireValue(count<=row.bytes,'payload_grew');hash.update(buffer.subarray(0,bytesRead));}
  const after=await file.stat();requireValue(count===row.bytes&&after.size===before.size&&after.mtimeMs===before.mtimeMs,'payload_changed');
  const actual=hash.digest('hex');requireValue(actual===row.sha256,'payload_digest_mismatch');return actual;
 }finally{await file.close();}
}
export async function planPayload(bundle,{role,namespace,msiVersion,expectedVersion}={}){
 requireValue(typeof bundle==='string'&&bundle.length>0,'bundle_required');
 requireValue(!/^[/\\]{2}/.test(bundle),'bundle_must_be_local');
 requireValue(['eef','eefn'].includes(role),'role_required');
 requireValue(typeof msiVersion==='string'&&/^(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)$/.test(msiVersion),'explicit_three_field_msi_version_required');
 const numbers=msiVersion.split('.').map(Number);requireValue(numbers[0]<=255&&numbers[1]<=255&&numbers[2]<=65535,'msi_version_out_of_range');
 // Namespace is explicit project metadata, never a host path, account or email.
 uuidV5(namespace,'validate');
 const requested=resolve(bundle);requireValue(!requested.startsWith('\\\\'),'bundle_must_be_local');requireValue((await lstat(requested)).isDirectory()&&!(await lstat(requested)).isSymbolicLink(),'bundle_root_invalid');
 const root=await realpath(requested);
 const inventoryBytes=await metadata(root,'files.sha256.json',MAX_INVENTORY);
 const inventory=parseJSON(inventoryBytes,'inventory_json_invalid');requireValue(Array.isArray(inventory)&&inventory.length>0&&inventory.length<=MAX_FILES,'inventory_count_invalid');
 const bundleBytes=await metadata(root,'bundle.json',16384),manifest=parseJSON(bundleBytes,'bundle_json_invalid');
 requireValue(manifest.name===role&&manifest.architecture==='x86_64-windows'&&manifest.runtime==='rust','bundle_role_architecture_invalid');
 requireValue(typeof manifest.version==='string'&&/^(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)(?:-(?:alpha|beta|rc)\.[1-9][0-9]*)?$/.test(manifest.version),'bundle_version_invalid');
 requireValue(typeof expectedVersion==='string'&&expectedVersion===manifest.version,'source_version_must_match_explicitly');
 requireValue(manifest.models_included!==true,'bundled_models_refused');
 const seen=new Set(),spellings=new Map(),selected=[],cacheSources=[],omissions={mutable_configuration:0,mutable_runtime_data:0,generated_python_cache:0};let bytes=0;
 for(const row of inventory){
  requireValue(row&&typeof row==='object'&&!Array.isArray(row)&&Object.keys(row).sort().join(',')==='bytes,path,sha256','inventory_row_invalid');
  safePath(row.path);const key=row.path.toLowerCase();requireValue(!seen.has(key),'case_aliased_inventory_path');seen.add(key);
  const parts=row.path.split('/');for(let index=1;index<=parts.length;index++){
   const prefix=parts.slice(0,index).join('/'),canonical=prefix.toLowerCase();
   requireValue(!spellings.has(canonical)||spellings.get(canonical)===prefix,'case_aliased_inventory_directory');spellings.set(canonical,prefix);
  }
  requireValue(Number.isSafeInteger(row.bytes)&&row.bytes>=0&&row.bytes<=MAX_TOTAL&&typeof row.sha256==='string'&&/^[a-f0-9]{64}$/.test(row.sha256),'inventory_metadata_invalid');
  requireValue(!/\.(?:gguf|safetensors)$/i.test(key),'model_weights_refused');
  const reason=excluded(row.path);if(reason){omissions[reason]++;continue;}
  const source=generatedPythonSource(row.path);
  if(source){cacheSources.push(source);omissions.generated_python_cache++;continue;}
  requireValue(key!=='files.sha256.json','self_referencing_inventory_refused');
  requireValue(role!=='eef'||!key.startsWith('tools/llama-'),'coordinator_inference_runtime_refused');
  bytes+=row.bytes;requireValue(bytes<=MAX_TOTAL,'payload_total_exceeded');
  const sha256=await hashFile(root,row);
  selected.push({path:row.path,bytes:row.bytes,sha256});
 }
 for(const required of ['bundle.json',role+'.exe','python/python.exe'])requireValue(selected.some(row=>row.path===required),'required_payload_missing');
 for(const source of cacheSources)requireValue(selected.some(row=>row.path===source),'generated_cache_source_missing');
 const bundleEntry=selected.find(row=>row.path==='bundle.json');requireValue(bundleEntry.sha256===digest(bundleBytes),'bundle_metadata_changed');
 if(role==='eefn')requireValue(selected.some(row=>row.path==='tools/llama-server.exe'),'node_runtime_missing');
 // This inventory includes mutable templates, so it cannot be blindly copied
 // into the transformed MSI payload. Require regeneration instead of publishing
 // a known-stale file list. No metadata is rewritten by this planner.
 selected.sort((a,b)=>a.path<b.path?-1:a.path>b.path?1:0);
 const identity=role+'\0x86_64-windows\0';
 const directories=new Map([['','INSTALLDIR']]);
 for(const file of selected){const parts=file.path.split('/');parts.pop();for(let index=1;index<=parts.length;index++){const path=parts.slice(0,index).join('/');directories.set(path,'D_'+digest(identity+path.toLowerCase()).slice(0,24));}}
 const files=selected.map(row=>{
  const parts=row.path.split('/'),name=parts.pop(),directory=parts.join('/');
  const id=digest(identity+row.path.toLowerCase()).slice(0,24);
  return {...row,file_id:'F_'+id,component_id:'C_'+id,component_code:uuidV5(namespace,identity+'component\0'+row.path.toLowerCase()),directory_id:directories.get(directory),filename:name};
 });
 requireValue(new Set(files.map(row=>row.file_id)).size===files.length&&new Set(files.map(row=>row.component_code)).size===files.length&&new Set(directories.values()).size===directories.size,'component_identity_collision');
 return {schema_version:1,report_type:'msi_payload_plan',read_only:true,role,architecture:'x86_64-windows',
  source_version:manifest.version,explicit_msi_version:msiVersion,source_inventory_sha256:digest(inventoryBytes),
  upgrade_code:uuidV5(namespace,identity+'upgrade'),product_code:uuidV5(namespace,identity+'product\0'+manifest.version+'\0'+msiVersion),
  package_code:null,files,selected_bytes:bytes,omissions,
  directories:[...directories].map(([path,id])=>({path,id,parent_id:path?directories.get(path.split('/').slice(0,-1).join('/')):null})),
  requires_new_distribution_inventory:true,source_inventory_not_copied:true,mutable_paths_not_read:true,
  generated_python_caches_not_owned:true,omitted_caches_have_verified_source:true,
  excluded_inventory_hashes_not_disclosed:true,absolute_source_paths_not_disclosed:true,
  package_authored:false,publisher_trust_verified:false,disk_preflight_complete:false,
  application_launch_verified:false,backend_initialization_compatibility_verified:false,
  version_ordering_approved:false,upgrade_migration_verified:false,signing_or_defender_verified:false,
  real_installer_accepted:false,stable_release_accepted:false};
}
if(process.argv[1]&&pathToFileURL(resolve(process.argv[1])).href===import.meta.url){
 try{const {values}=parseArgs({options:{bundle:{type:'string'},role:{type:'string'},namespace:{type:'string'},'msi-version':{type:'string'},'expected-version':{type:'string'},help:{type:'boolean'}}});
  if(values.help){console.log('Usage: node tools/msi-payload-plan.mjs --bundle PATH --role eef|eefn --namespace PROJECT_UUID --expected-version SEMVER --msi-version MAJOR.MINOR.BUILD\nRead-only experimental plan; no package, install, migration or version-ordering approval.');}
  else {
  requireValue(typeof values.bundle==='string','bundle_required');
  console.log(JSON.stringify(await planPayload(values.bundle,{role:values.role,namespace:values.namespace,msiVersion:values['msi-version'],expectedVersion:values['expected-version']}),null,2));
  }
 }catch(error){const code=/^[a-z][a-z0-9_]{1,96}$/.test(error.message)?error.message:(['ENOENT','ENOTDIR','EACCES','EPERM'].includes(error.code)?'payload_file_unavailable':'payload_plan_failed');console.error(code);process.exitCode=1;}
}
