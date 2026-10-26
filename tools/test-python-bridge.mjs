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
const plugin=join(scratch,'plugin.py'),children=[],memoryResults=[];let probeNumber=0,deadlineParentStarted=false,deadlineChildStarted=false;
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
    if mode in ('deadline','deadline-after-start','cancel','descendant','inherited'):
        child_delay=30 if mode=='deadline-after-start' else 3
        child_code="import time,pathlib; p=pathlib.Path("+repr(str(ROOT))+ "); (p/'child-started').write_text('ready'); time.sleep("+str(child_delay)+"); (p/'late-child-effect').write_text('unexpected')"
        child=subprocess.Popen([sys.executable,'-I','-c',child_code],stdin=subprocess.DEVNULL,stdout=None if mode=='inherited' else subprocess.DEVNULL,stderr=None if mode=='inherited' else subprocess.DEVNULL)
        (ROOT/'root-pid').write_text(str(os.getpid()))
        (ROOT/'child-pid').write_text(str(child.pid))
        end=time.monotonic()+1
        while not (ROOT/'child-started').exists() and time.monotonic()<end: time.sleep(.005)
        (ROOT/'root-started').write_text('ready')
        if mode not in ('descendant','inherited'): time.sleep(30)
    return {'bounded':'valid','request':params.get('text',''),'bytecode_writes_disabled':sys.dont_write_bytecode}
`);
async function probe(params,limits={},cancel_after_ms=null,cancel_after_marker=null){
 // Ordinary success probes must allow process inspection/startup. The explicit
 // one-second deadline below remains strict; production defaults stay unchanged.
 const appliedLimits={timeout_seconds:8,max_request_bytes:65536,max_stdout_bytes:8192,max_stderr_bytes:1024,...limits};
 const exe=resolve(process.env.EEF_TEST_BINARY_DIR||join(root,'target/debug'),'examples/python-bridge-fixture.exe');
 const child=spawn(exe,[],{cwd:root,windowsHide:true,env:{...process.env,PATH:join(root,'.tooling/llvm-mingw-20260616-ucrt-x86_64/bin')+';'+process.env.PATH},stdio:['pipe','pipe','pipe']});children.push(child);
 let out='',err='';child.stdout.on('data',b=>out+=b);child.stderr.on('data',b=>err+=b);
 child.stdin.end(JSON.stringify({python,plugin,action:'run',params,limits:appliedLimits,cancel_after_ms,cancel_after_marker}));
 const code=await new Promise((r,j)=>{const timer=setTimeout(()=>{child.kill();j(Error('Fixture deadline '+scratch));},12000);child.on('error',j);child.on('close',c=>{clearTimeout(timer);r(c);});});
 assert.equal(code,0,'Native fixture failed; private '+scratch);assert(!err.includes('PRIVATE_CAPTURE_BYTES'));const result=JSON.parse(out);assert(!out.includes('PRIVATE_CAPTURE_BYTES'),'private plugin output escaped into diagnostics');
 await writeFile(join(scratch,`PRIVATE-probe-${probeNumber++}.json`),JSON.stringify({params,limits:appliedLimits,result},null,2));return result;
}
function successfulData(probeResult,label){assert.equal(probeResult.result.success,true,`${label}: ${probeResult.result.failure||'unknown failure'}; private ${scratch}`);assert(probeResult.result.data&&typeof probeResult.result.data==='object',`${label}: missing data; private ${scratch}`);return probeResult.result.data;}
try{
 const initial=successfulData(await probe({text:'bounded input'}),'initial bounded probe');
 assert.equal(initial.request,'bounded input');
 assert.equal(initial.bytecode_writes_disabled,true,'Managed Python must suppress automatic import-cache writes');
 await assert.rejects(()=>stat(join(scratch,'__pycache__')),{code:'ENOENT'},'Inspection and invocation must not create import caches beside plugin sources');
 for(const mode of ['memory','child-memory']){
  const control=successfulData(await probe({mode},{timeout_seconds:8}),mode+' control').allocation_succeeded;
  const capped=successfulData(await probe({mode},{timeout_seconds:8,max_job_memory_mib:128}),mode+' capped').allocation_succeeded;
  assert.equal(control,true,'control without commit ceiling must allocate');
  assert.equal(capped,false,'owned-job commit ceiling must cover root and descendant allocations');
  memoryResults.push({mode,allocation_bytes:192*1024*1024,ceiling_mib:128,uncapped_allocation_succeeded:control,capped_allocation_succeeded:capped});
 }
 assert.equal((await probe({text:'bounded startup'},{max_job_memory_mib:128})).result.success,true);
 const unicode='สวัสดี世界🌊'.repeat(250);
 assert.equal(successfulData(await probe({text:unicode}),'UTF-8 probe').request,unicode,'UTF-8 content must round-trip within its byte budget without locale decoding or ASCII-escape expansion');
 for(const [mode,expected] of [['stdout','StdoutLimit'],['stderr','StderrLimit'],['invalid','Reply'],['error','Process']]){
  const value=await probe({mode});assert.equal(value.result.success,false);assert.equal(value.result.failure,expected);
 }
 const request=await probe({text:'x'.repeat(2048)},{max_request_bytes:1024});assert.equal(request.result.failure,'RequestLimit');
 // Root exits successfully with a detached child. Windows job retirement must
 // not wait for that child's delayed side effect or leave it running.
 async function resetMarkers(){for(const name of ['child-started','root-started','late-child-effect','root-pid','child-pid'])await unlink(join(scratch,name)).catch(e=>{if(e.code!=='ENOENT')throw e;});}
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
 // Deadline covers inspection/startup as well as execution. Do not claim that
 // the plugin/child ran when it expired before either marker. The preceding
 // descendant/inherited controls and following marker-triggered cancellation
 // independently require real children and prove their retirement.
 deadlineParentStarted=await readFile(join(scratch,'root-started'),'utf8').then(value=>value==='ready',error=>{if(error.code==='ENOENT')return false;throw error;});
 deadlineChildStarted=await readFile(join(scratch,'child-started'),'utf8').then(value=>value==='ready',error=>{if(error.code==='ENOENT')return false;throw error;});
 await new Promise(r=>setTimeout(r,3300));
 assert.equal(await stat(join(scratch,'late-child-effect')).then(()=>true,()=>false),false);
 await resetMarkers();
 // Separately prove deadline retirement after actual parent/child startup. The
 // longer explicit test budget is still below the unchanged 12-second fixture
 // guard. No production bridge default or owner-configured limit is changed.
 const postStartDeadline=await probe({mode:'deadline-after-start'},{timeout_seconds:8});
 assert.equal(postStartDeadline.result.failure,'Deadline');
 for(const marker of ['root-started','child-started'])assert.equal(await readFile(join(scratch,marker),'utf8'),'ready');
 const ownedPids=await Promise.all(['root-pid','child-pid'].map(async name=>Number(await readFile(join(scratch,name),'utf8'))));
 for(const pid of ownedPids)assert(Number.isInteger(pid)&&pid>0&&pid!==process.pid);
 const alive=pid=>{try{process.kill(pid,0);return true;}catch(error){if(error.code==='ESRCH')return false;throw error;}};
 const retiredBy=Date.now()+1500;
 while(ownedPids.some(alive)&&Date.now()<retiredBy)await new Promise(r=>setTimeout(r,20));
 assert(ownedPids.every(pid=>!alive(pid)),'deadline must retire both observed owned Python processes');
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
 await writeFile(join(scratch,'result.json'),JSON.stringify({passed:true,scope:'real-interpreter, bounded synthetic plugin; no hardware or grant authority',memoryResults,
  ordinary_probe_timeout_seconds:8,explicit_deadline_seconds:1,deadlineParentStarted,deadlineChildStarted,
  deadline_post_start_not_assumed:true,post_start_deadline_seconds:8,post_start_deadline_parent_and_child_retired:true,
  marker_triggered_cancel_requires_parent_and_child:true,managed_bytecode_writes_disabled:true,plugin_import_cache_not_created:true}));
 console.log('Python bridge native checks passed: '+scratch);
}finally{for(const child of children)if(child.exitCode===null)child.kill();}
