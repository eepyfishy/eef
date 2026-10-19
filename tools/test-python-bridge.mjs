// Actual explicitly supplied interpreter + production Rust bridge, not devices,
// workload authority, an installer, or a bundled/new interpreter download.
import assert from 'node:assert/strict';
import {mkdtemp,writeFile,readFile,stat,unlink} from 'node:fs/promises';
import {resolve,join} from 'node:path';
import {spawn} from 'node:child_process';
const root=resolve(import.meta.dirname,'..'),python=process.env.EEF_TEST_PYTHON;
assert.equal(process.platform,'win32','This native owned-job acceptance fixture is Windows-only');
assert(python,'Set EEF_TEST_PYTHON to an existing trusted interpreter; no download is attempted');
await stat(python);
const scratch=await mkdtemp(join(root,'.validation/python-bridge-'));
const plugin=join(scratch,'plugin.py'),children=[],memoryResults=[];
await writeFile(plugin,`import os,sys,time,subprocess,pathlib
CAPABILITY='camera.capture'
ACTIONS=['run']
ROOT=pathlib.Path(__file__).parent
def handle(action,params):
    mode=params.get('mode','ok')
    if mode=='memory':
        try: memory=bytearray(192*1024*1024)
        except MemoryError: return {'allocation_succeeded':False}
        return {'allocation_succeeded':True,'bytes':len(memory)}
    if mode=='child-memory':
        code=chr(10).join(["import json,sys; result=False", "try: memory=bytearray(192*1024*1024); result=True", "except MemoryError: pass", "sys.stdout.write(json.dumps({'allocation_succeeded':result}))"])
        child=subprocess.run([sys.executable,'-I','-c',code],stdin=subprocess.DEVNULL,stdout=subprocess.PIPE,stderr=subprocess.DEVNULL,timeout=5)
        if child.returncode!=0: raise RuntimeError('child allocation probe failed')
        import json
        return json.loads(child.stdout)
    if mode=='stdout':
        os.write(1,b'PRIVATE_CAPTURE_BYTES'*10000)
    if mode=='stderr':
        os.write(2,b'PRIVATE_CAPTURE_BYTES'*10000)
    if mode=='invalid':
        os.write(1,b'PRIVATE_CAPTURE_BYTES')
    if mode=='error':
        raise RuntimeError('PRIVATE_CAPTURE_BYTES')
    if mode in ('deadline','cancel','descendant','inherited'):
        child_code="import time,pathlib; p=pathlib.Path("+repr(str(ROOT))+ "); (p/'child-started').write_text('ready'); time.sleep(3); (p/'late-child-effect').write_text('unexpected')"
        subprocess.Popen([sys.executable,'-I','-c',child_code],stdin=subprocess.DEVNULL,stdout=None if mode=='inherited' else subprocess.DEVNULL,stderr=None if mode=='inherited' else subprocess.DEVNULL)
        end=time.monotonic()+1
        while not (ROOT/'child-started').exists() and time.monotonic()<end: time.sleep(.005)
        (ROOT/'root-started').write_text('ready')
        if mode not in ('descendant','inherited'): time.sleep(30)
    return {'bounded':'valid','request':params.get('text','')}
`);
async function probe(params,limits={},cancel_after_ms=null,cancel_after_marker=null){
 const exe=resolve(process.env.EEF_TEST_BINARY_DIR||join(root,'target/debug'),'examples/python-bridge-fixture.exe');
 const child=spawn(exe,[],{cwd:root,windowsHide:true,env:{...process.env,PATH:join(root,'.tooling/llvm-mingw-20260616-ucrt-x86_64/bin')+';'+process.env.PATH},stdio:['pipe','pipe','pipe']});children.push(child);
 let out='',err='';child.stdout.on('data',b=>out+=b);child.stderr.on('data',b=>err+=b);
 child.stdin.end(JSON.stringify({python,plugin,action:'run',params,limits:{timeout_seconds:2,max_request_bytes:65536,max_stdout_bytes:8192,max_stderr_bytes:1024,...limits},cancel_after_ms,cancel_after_marker}));
 const code=await new Promise((r,j)=>{const timer=setTimeout(()=>{child.kill();j(Error('Fixture deadline '+scratch));},12000);child.on('error',j);child.on('close',c=>{clearTimeout(timer);r(c);});});
 assert.equal(code,0,'Native fixture failed; private '+scratch);assert(!err.includes('PRIVATE_CAPTURE_BYTES'));const result=JSON.parse(out);assert(!out.includes('PRIVATE_CAPTURE_BYTES'),'private plugin output escaped into diagnostics');return result;
}
try{
 assert.equal((await probe({text:'bounded input'})).result.data.request,'bounded input');
 for(const mode of ['memory','child-memory']){
  const control=(await probe({mode},{timeout_seconds:8})).result.data.allocation_succeeded;
  const capped=(await probe({mode},{timeout_seconds:8,max_job_memory_mib:128})).result.data.allocation_succeeded;
  assert.equal(control,true,'control without commit ceiling must allocate');
  assert.equal(capped,false,'owned-job commit ceiling must cover root and descendant allocations');
  memoryResults.push({mode,allocation_bytes:192*1024*1024,ceiling_mib:128,uncapped_allocation_succeeded:control,capped_allocation_succeeded:capped});
 }
 assert.equal((await probe({text:'bounded startup'},{max_job_memory_mib:128})).result.success,true);
 const unicode='สวัสดี世界🌊'.repeat(250);
 assert.equal((await probe({text:unicode})).result.data.request,unicode,'UTF-8 content must round-trip within its byte budget without locale decoding or ASCII-escape expansion');
 for(const [mode,expected] of [['stdout','StdoutLimit'],['stderr','StderrLimit'],['invalid','Reply'],['error','Process']]){
  const value=await probe({mode});assert.equal(value.result.success,false);assert.equal(value.result.failure,expected);
 }
 const request=await probe({text:'x'.repeat(2048)},{max_request_bytes:1024});assert.equal(request.result.failure,'RequestLimit');
 // Root exits successfully with a detached child. Windows job retirement must
 // not wait for that child's delayed side effect or leave it running.
 async function resetMarkers(){for(const name of ['child-started','root-started','late-child-effect'])await unlink(join(scratch,name)).catch(e=>{if(e.code!=='ENOENT')throw e;});}
 await resetMarkers();
 assert.equal((await probe({mode:'descendant'})).result.success,true);
 assert.equal(await readFile(join(scratch,'child-started'),'utf8'),'ready');
 await new Promise(r=>setTimeout(r,3300));
 assert.equal(await stat(join(scratch,'late-child-effect')).then(()=>true,()=>false),false);
 await resetMarkers();
 assert.equal((await probe({mode:'inherited'})).result.success,true,'owned child retaining stdout/stderr must not hold EOF after its parent exits');
 assert.equal(await readFile(join(scratch,'root-started'),'utf8'),'ready');
 await new Promise(r=>setTimeout(r,3300));
 assert.equal(await stat(join(scratch,'late-child-effect')).then(()=>true,()=>false),false);
 await resetMarkers();
 const deadline=await probe({mode:'deadline'},{timeout_seconds:1});assert.equal(deadline.result.failure,'Deadline');assert(deadline.elapsed_ms<4000);
 assert.equal(await readFile(join(scratch,'root-started'),'utf8'),'ready');
 await new Promise(r=>setTimeout(r,3300));
 assert.equal(await stat(join(scratch,'late-child-effect')).then(()=>true,()=>false),false);
 await resetMarkers();
 // Trigger cancellation only after the root has spawned/observed its child.
 // A fixed delay can expire during inspection/startup and never test cleanup.
 const cancelled=await probe({mode:'cancel'},{timeout_seconds:10},null,join(scratch,'root-started'));assert.equal(cancelled.result.failure,'Cancelled');assert(cancelled.elapsed_ms<4000);
 assert.equal(await readFile(join(scratch,'root-started'),'utf8'),'ready');
 assert.equal(await readFile(join(scratch,'child-started'),'utf8'),'ready');
 await new Promise(r=>setTimeout(r,3300));
 assert.equal(await stat(join(scratch,'late-child-effect')).then(()=>true,()=>false),false);
 assert.equal((await probe({text:'still usable'})).result.success,true);
 await writeFile(join(scratch,'result.json'),JSON.stringify({passed:true,scope:'real-interpreter, bounded synthetic plugin; no hardware or grant authority',memoryResults}));
 console.log('Python bridge native checks passed: '+scratch);
}finally{for(const child of children)if(child.exitCode===null)child.kill();}
