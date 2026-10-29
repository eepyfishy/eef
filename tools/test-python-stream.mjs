// Real trusted interpreter + production owned stream bridge; synthetic metadata,
// not a microphone/camera, listening authority, continuous-job or physical proof.
import assert from 'node:assert/strict';
import {mkdtemp,mkdir,writeFile,readFile,stat} from 'node:fs/promises';
import {resolve,join} from 'node:path';
import {spawn} from 'node:child_process';
import {randomUUID,createHash} from 'node:crypto';
assert.equal(process.platform,'win32','Native owned-job acceptance is Windows-only');
const root=resolve(import.meta.dirname,'..'),python=process.env.EEF_TEST_PYTHON;
assert(python,'Supply an existing trusted interpreter; no download is attempted');await stat(python);
const bin=resolve(process.env.EEF_TEST_BINARY_DIR||join(root,'target/debug')),
 helper=join(bin,'examples/python-stream-fixture.exe'),observer=join(bin,'examples/process-fixture.exe');
await stat(helper);await stat(observer);
const scratch=await mkdtemp(join(root,'.validation/python-stream-')),plugin=join(scratch,'source.py'),children=[],cases=[];
const env={...process.env,PATH:join(root,'.tooling/llvm-mingw-20260616-ucrt-x86_64/bin')+';'+process.env.PATH};
// Keep exact inputs and held-handle observations so a later reader can audit
// this run without invoking capture or replaying native work.
const pin=async path=>({path,bytes:(await stat(path)).size,sha256:createHash('sha256').update(await readFile(path)).digest('hex')});
const inputs=await Promise.all([helper,observer,python,import.meta.filename].map(pin));
await writeFile(join(scratch,'inputs.json'),JSON.stringify(inputs,null,2),{flag:'wx'});
await writeFile(plugin,`import ctypes,json,os,pathlib,subprocess,sys,time
from ctypes import wintypes

def identities(child):
    kernel=ctypes.WinDLL('kernel32',use_last_error=True)
    kernel.GetCurrentProcess.restype=wintypes.HANDLE
    kernel.GetProcessTimes.argtypes=[wintypes.HANDLE]+[ctypes.POINTER(wintypes.FILETIME)]*4
    kernel.GetProcessTimes.restype=wintypes.BOOL
    def created(handle):
        values=[wintypes.FILETIME() for _ in range(4)]
        if not kernel.GetProcessTimes(handle,*[ctypes.byref(v) for v in values]):
            raise OSError(ctypes.get_last_error())
        return (values[0].dwHighDateTime<<32)|values[0].dwLowDateTime
    return {'root':{'pid':os.getpid(),'created':created(kernel.GetCurrentProcess())},
            'child':{'pid':child.pid,'created':created(wintypes.HANDLE(int(child._handle)))}}

def stream(action,params):
    root=pathlib.Path(params['root']);mode=params['mode']
    if action!='observe':raise ValueError('PRIVATE_ACTION_ERROR')
    if mode=='handler_error':raise RuntimeError('PRIVATE_STREAM_ERROR')
    if mode=='stdout':os.write(1,b'PRIVATE_STREAM_BYTES'*100000)
    if mode=='stderr':os.write(2,b'PRIVATE_STREAM_BYTES'*100000)
    if mode=='invalid':
        yield {'data_base64':'PRIVATE_STREAM_BYTES'}
        return
    if mode=='empty':return
    if mode=='memory':
        succeeded=False
        try:
            memory=bytearray(256*1024*1024)
            succeeded=True
        except MemoryError:pass
        (root/'memory.json').write_text(json.dumps({'allocation_succeeded':succeeded}))
    if mode in ('controlled','hold'):
        code="import pathlib,time; time.sleep(30); pathlib.Path("+repr(str(root/'late-child-effect'))+").write_text('unexpected')"
        child=subprocess.Popen([sys.executable,'-I','-B','-c',code],stdin=subprocess.DEVNULL,stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL)
        (root/'pids.json').write_text(json.dumps(identities(child)))
    try:
        yield {'source_frames':80,'active':None,'level':None,'discontinuity':True}
        if mode=='controlled':
            while not (root/'release').exists():time.sleep(.005)
        if mode=='hold':time.sleep(30)
        yield {'source_frames':160,'active':False,'level':7,'discontinuity':False}
        yield {'source_frames':240,'active':True,'level':64,'discontinuity':False}
    finally:
        (root/'iterator-closed').write_text('closed')
`);

function launch(exe,args,input){
 const child=spawn(exe,args,{cwd:root,env,windowsHide:true,stdio:[input===undefined?'ignore':'pipe','pipe','pipe']});children.push(child);
 let out='',err='',pending='';const events=[];
 child.stdout.on('data',b=>{out+=b;pending+=b;for(;;){const at=pending.indexOf('\n');if(at<0)break;const line=pending.slice(0,at);pending=pending.slice(at+1);try{events.push(JSON.parse(line));}catch{events.push({invalid_output:true});}}});
 child.stderr.on('data',b=>err+=b);
 child.logs=()=>({out,err});child.events=events;
 child.result=new Promise((r,j)=>{child.once('error',j);child.once('close',code=>r({code,out,err,events}));});
 if(input!==undefined){child.stdin.on('error',()=>{});child.stdin.end(JSON.stringify(input));}
 return child;
}
const exists=path=>stat(path).then(()=>true,()=>false);
async function until(fn,label){const deadline=Date.now()+15000;while(Date.now()<deadline){if(await fn())return;await new Promise(r=>setTimeout(r,10));}throw Error('Timeout: '+label+'; retain '+scratch);}
async function start(name,mode,extra={}){
 const folder=join(scratch,name);await mkdir(folder);
 const request={python,plugin,stream_id:randomUUID(),params:{root:folder,mode},
  bounds:{max_frame_bytes:1024,max_stdout_bytes:32768,max_observations:16,max_source_frames:10000},
  limits:{timeout_seconds:10,max_request_bytes:65536,max_stdout_bytes:32768,max_stderr_bytes:1024,max_job_memory_mib:128},
  execution_ms:8000,cancel_marker:null,observer_rejection_marker:null,...extra};
 await writeFile(join(folder,'request.json'),JSON.stringify(request,null,2),{flag:'wx'});
 return {name,folder,request,child:launch(helper,[],request)};
}
async function finish(test,expected){
 const r=await test.child.result;
 await writeFile(join(test.folder,'process-result.json'),JSON.stringify(r,null,2),{flag:'wx'});
 assert.equal(r.code,0,'Fixture exit; private '+scratch);assert(!r.events.some(e=>e.invalid_output));
 const finals=r.events.filter(e=>e.event_type==='result');assert.equal(finals.length,1);const final=finals[0];
 assert.equal(final.result.success,expected==='success');assert.equal(final.physical_capture,false);assert.equal(final.workload_authorized,false);assert.equal(final.external_effects_stopped,false);
 if(expected!=='success')assert(expected.split('|').includes(final.result.failure),JSON.stringify(final));
 assert(!r.out.includes('PRIVATE_STREAM'));assert(!r.out.includes('PRIVATE_OBSERVER'));assert(!r.err.includes('PRIVATE_STREAM'));assert(!r.err.includes('PRIVATE_OBSERVER'));
 cases.push({name:test.name,success:final.result.success,failure:final.result.failure??null,observations:r.events.filter(e=>e.event_type==='observation').length,elapsed_ms:final.elapsed_ms});return {r,final};
}
async function held(test){
 await until(()=>exists(join(test.folder,'pids.json')),'exact producer/child identities');
 const identities=await readFile(join(test.folder,'pids.json'),'utf8'),watch=launch(observer,['observe',test.folder]);
 await until(()=>exists(join(test.folder,'observer-ready')),'live creation-verified handles held');return {identities,watch};
}
async function exited(test,proof){
 const r=await proof.watch.result;
 await writeFile(join(test.folder,'held-observer-result.json'),JSON.stringify(r,null,2),{flag:'wx'});
 assert.equal(r.code,0,'Held observer; private '+scratch);const observed=JSON.parse(r.out);assert.equal(observed.both_exited,true);assert.equal(observed.held_creation_verified_handles,true);
 assert.equal(await readFile(join(test.folder,'pids.json'),'utf8'),proof.identities);assert.equal(await exists(join(test.folder,'late-child-effect')),false);return observed;
}
try{
 const live=await start('live','controlled');await until(()=>live.child.events.some(e=>e.event_type==='observation'),'live metadata before EOF');
 assert.equal(live.child.exitCode,null);assert.equal(live.child.events.some(e=>e.event_type==='result'),false);
 const initial=live.child.events.find(e=>e.event_type==='observation').observation;assert.equal(initial.active,null);assert.equal(initial.discontinuity,true);
 const liveProof=await held(live);await writeFile(join(live.folder,'release'),'release owned synthetic iterator');
 const liveResult=await finish(live,'success');assert.deepEqual(liveResult.r.events.filter(e=>e.event_type==='observation').map(e=>e.observation.active),[null,false,true]);
 assert.equal(liveResult.final.result.summary.observations,3);assert.equal(liveResult.final.result.summary.source_frames,240);assert.equal(liveResult.final.result.summary.stream_id,live.request.stream_id);
 assert.equal(await readFile(join(live.folder,'iterator-closed'),'utf8'),'closed');await exited(live,liveProof);
 for(const [mode,expected] of [['handler_error','Reply|Process'],['stdout','StdoutLimit'],['stderr','StderrLimit'],['invalid','Reply|Process'],['empty','Reply|Process']])await finish(await start(mode,mode),expected);
 const memory=await start('memory','memory');await finish(memory,'success');assert.equal(JSON.parse(await readFile(join(memory.folder,'memory.json'),'utf8')).allocation_succeeded,false,'Owned-job commit ceiling must not be ignored');
 const rejectedFolder=join(scratch,'observer-rejected'),rejected=await start('observer-rejected','hold',{observer_rejection_marker:join(rejectedFolder,'reject')});
 await until(()=>rejected.child.events.some(e=>e.event_type==='observation'),'observer entered');const rejectedProof=await held(rejected);await writeFile(join(rejected.folder,'reject'),'reject');await finish(rejected,'Reply');await exited(rejected,rejectedProof);
 const cancelFolder=join(scratch,'cancelled'),cancel=await start('cancelled','hold',{cancel_marker:join(cancelFolder,'cancel')});
 await until(()=>cancel.child.events.some(e=>e.event_type==='observation'),'capture active before cancellation');const cancelProof=await held(cancel);const cancelAt=Date.now();await writeFile(join(cancel.folder,'cancel'),'ready');
 const cancelled=await finish(cancel,'Cancelled');await exited(cancel,cancelProof);assert(Date.now()-cancelAt<5000);assert(cancelled.final.elapsed_ms<cancel.request.execution_ms,'Cancellation is not waiting for original deadline');
 const deadline=await start('deadline','hold',{execution_ms:4000});await until(()=>deadline.child.events.some(e=>e.event_type==='observation'),'capture active before deadline');const deadlineProof=await held(deadline);
 const expired=await finish(deadline,'Deadline');await exited(deadline,deadlineProof);assert(expired.final.elapsed_ms>=3000&&expired.final.elapsed_ms<8000,'Original four-second parent deadline remains enforced');
 assert.equal(cases.length,10);assert.equal(await exists(join(scratch,'__pycache__')),false);
 await writeFile(join(scratch,'results.json'),JSON.stringify({passed:true,cases,real_python:true,production_owned_bridge:true,live_delivery_before_end_and_eof:true,unknown_not_false:true,strict_frame_and_total_output_bounds:true,private_errors_not_exposed:true,configured_owned_commit_ceiling:true,held_creation_verified_exit_cases:4,normal_iterator_closed:true,observer_failure_cancellation_and_deadline_retire_owned_tree:true,original_parent_deadlines_unchanged:true,bytecode_writes_disabled:true,
  physical_capture:false,ambient_listening_enabled:false,workload_authority_integrated:false,continuous_job_lifecycle_complete:false,event_transport_to_coordinator_complete:false,physical_resource_quota_accepted:false,stable_release_accepted:false},null,2));
 console.log('Native Python metadata stream validation passed: '+scratch);
}catch(error){await writeFile(join(scratch,'PRIVATE-failure.json'),JSON.stringify({error:String(error)},null,2));await writeFile(join(scratch,'PRIVATE-logs.json'),JSON.stringify(children.map(c=>({exe:c.spawnfile,pid:c.pid,code:c.exitCode,...c.logs()})),null,2));throw error;}
finally{for(const child of children)if(child.exitCode===null)child.kill();await Promise.allSettled(children.map(c=>c.result));}
