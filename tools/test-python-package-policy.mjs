// Read-only native PowerShell staging policy; no interpreter/download/installer.
import assert from 'node:assert/strict';
import {mkdtemp,mkdir,writeFile,readFile,symlink,readdir} from 'node:fs/promises';
import {spawnSync} from 'node:child_process';
import {resolve,join} from 'node:path';
const root=resolve(import.meta.dirname,'..');
await mkdir(join(root,'.validation'),{recursive:true});
const scratch=await mkdtemp(join(root,'.validation/python-package-policy-'));
const script=join(root,'tools/assert-python-payload.ps1'),shell=process.platform==='win32'?'powershell.exe':'pwsh';
let checks=0;
function check(path,success,pattern){
 // A fresh Windows runner can spend over 15 seconds starting Windows PowerShell.
 // Bound the whole child once, retain errors, and never retry a policy check.
 const result=spawnSync(shell,['-NoProfile','-NonInteractive','-File',script,'-PythonDirectory',path],{encoding:'utf8',windowsHide:true,timeout:60000});
 assert.ifError(result.error);
 assert.equal(result.signal,null,'Payload guard ended by signal');
 assert.equal(result.status,success?0:1,result.stderr);
 if(success){const report=JSON.parse(result.stdout);assert.equal(report.passed,true);assert.equal(report.read_only,true);assert.equal(report.payload_changed,false);assert.equal(report.links_followed,false);}else assert(pattern.test(result.stderr),result.stderr);
 checks++;
}
const clean=join(scratch,"clean source ' literal"),module=join(clean,'Lib/site-packages/example');await mkdir(module,{recursive:true});await writeFile(join(module,'__init__.py'),'# immutable source fixture\n');const before=await readFile(join(module,'__init__.py'));
check(clean,true);assert.deepEqual(await readFile(join(module,'__init__.py')),before);
for(const name of ['__pycache__','__PyCache__']){const dirty=join(scratch,name==='__pycache__'?'dirty':'case-dirty'),cache=join(dirty,'Lib/site-packages/example',name);await mkdir(cache,{recursive:true});check(dirty,false,/Generated Python cache directory refused/);assert.deepEqual(await readdir(cache),[],'Guard must not remove or rewrite even an empty cache');}
const file=join(scratch,'not-a-directory');await writeFile(file,'owner bytes');check(file,false,/regular directory/);assert.equal(await readFile(file,'utf8'),'owner bytes');
const alias=join(scratch,'root-link');await symlink(clean,alias,process.platform==='win32'?'junction':'dir');check(alias,false,/regular directory/);
const linked=join(scratch,'linked-payload');await mkdir(linked);await symlink(clean,join(linked,'external'),process.platform==='win32'?'junction':'dir');check(linked,false,/links are refused/);assert.deepEqual(await readFile(join(module,'__init__.py')),before);
check(join(scratch,'missing'),false,/Cannot find path|does not exist/);
assert.equal(checks,7);console.log('Python payload policy validation passed: 7 read-only native checks; cache/link refusal without deletion');
