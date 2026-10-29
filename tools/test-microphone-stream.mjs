// Production Rust/Python stream transport + actual energy iterator, fake audio
// device only. This never admits a workload or opens microphone hardware.
import assert from 'node:assert/strict';
import {mkdtemp,mkdir,readFile,writeFile,stat} from 'node:fs/promises';
import {resolve,join} from 'node:path';
import {spawn} from 'node:child_process';
import {randomUUID,createHash} from 'node:crypto';
assert.equal(process.platform,'win32','Owned-job native probe requires Windows');
const root=resolve(import.meta.dirname,'..'),python=process.env.EEF_TEST_PYTHON;
assert(python,'Supply an existing trusted interpreter');await stat(python);
const bin=resolve(process.env.EEF_TEST_BINARY_DIR||join(root,'target/debug')),helper=join(bin,'examples/python-stream-fixture.exe');await stat(helper);
await mkdir(join(root,'.validation'),{recursive:true});const scratch=await mkdtemp(join(root,'.validation/microphone-stream-'));
const adapter=join(root,'python/streams/microphone_energy.py'),plugin=join(scratch,'device.py'),cases=[];
await writeFile(plugin,`import array,importlib.util,json,pathlib,sys,types
spec=importlib.util.spec_from_file_location('actual_energy_source',${JSON.stringify(adapter)})
module=importlib.util.module_from_spec(spec);spec.loader.exec_module(module)

def stream(action,params):
    if action!='observe':raise ValueError('PRIVATE_ACTION_ERROR')
    root=pathlib.Path(params['fixture_root']);mode=params['mode']
    class Device:
        opened=closed=reads=0
        def __init__(self,**options):self.options=options
        def __enter__(self):self.opened+=1;return self
        def __exit__(self,*args):self.closed+=1
        def read(self,count):
            self.reads+=1
            if mode=='malformed' and self.reads==2:return b'',False
            levels=[750,2000,100] if mode=='normal' else [2000,0,750]
            return array.array('h',[levels[self.reads-1]]*count).tobytes(),mode=='gap' and self.reads==2
    devices=[]
    def factory(**options):
        device=Device(**options);devices.append(device);return device
    sys.modules['sounddevice']=types.SimpleNamespace(RawInputStream=factory)
    iterator=module.stream('energy',{'block_frames':80,'max_blocks':3})
    try:yield from iterator
    finally:
        iterator.close()
        (root/'device-result.json').write_text(json.dumps({'instances':len(devices),'opened':sum(d.opened for d in devices),'closed':sum(d.closed for d in devices),'reads':sum(d.reads for d in devices)}))
`);
const pins=await Promise.all([python,helper,adapter,plugin,import.meta.filename].map(async path=>({path,sha256:createHash('sha256').update(await readFile(path)).digest('hex')})));
await writeFile(join(scratch,'inputs.json'),JSON.stringify(pins,null,2),{flag:'wx'});
for(const mode of ['normal','gap','malformed']){
 const folder=join(scratch,mode);await mkdir(folder);
 const input={python,plugin,stream_id:randomUUID(),params:{fixture_root:folder,mode},bounds:{max_frame_bytes:1024,max_stdout_bytes:32768,max_observations:16,max_source_frames:10000},limits:{timeout_seconds:8,max_request_bytes:65536,max_stdout_bytes:32768,max_stderr_bytes:1024,max_job_memory_mib:128},execution_ms:6000,cancel_marker:null,observer_rejection_marker:null};
 await writeFile(join(folder,'request.json'),JSON.stringify(input,null,2),{flag:'wx'});
 const child=spawn(helper,[],{cwd:root,windowsHide:true,env:{...process.env,PATH:join(bin,'../../.tooling/llvm-mingw-20260616-ucrt-x86_64/bin')+';'+process.env.PATH},stdio:['pipe','pipe','pipe']});let out='',err='';
 child.stdout.on('data',b=>out+=b);child.stderr.on('data',b=>err+=b);child.stdin.on('error',()=>{});child.stdin.end(JSON.stringify(input));
 const timer=setTimeout(()=>child.kill(),10000);let code;try{code=await new Promise((r,j)=>{child.once('error',j);child.once('close',r);});}finally{clearTimeout(timer);}
 await writeFile(join(folder,'process-result.json'),JSON.stringify({code,out,err},null,2),{flag:'wx'});
 assert.equal(code,0,'Retain failed native fixture '+scratch);assert(!/PRIVATE_ACTION|data_base64|audio\/wav/.test(out+err));
 const events=out.trim().split('\n').map(l=>JSON.parse(l)),finals=events.filter(e=>e.event_type==='result');assert.equal(finals.length,1);const final=finals[0];assert.equal(final,events.at(-1));
 const observations=events.filter(e=>e.event_type==='observation').map(e=>e.observation);
 for(const observation of observations)assert.deepEqual(Object.keys(observation).sort(),['active','discontinuity','level','sequence','source_frames']);
 assert.equal(final.physical_capture,false);assert.equal(final.workload_authorized,false);assert.equal(final.external_effects_stopped,false);
 const device=JSON.parse(await readFile(join(folder,'device-result.json'),'utf8'));assert.deepEqual(device,{instances:1,opened:1,closed:1,reads:mode==='malformed'?2:3});
 if(mode==='malformed'){assert.equal(final.result.success,false);assert(['Reply','Process'].includes(final.result.failure));assert.equal(observations.length,1);}
 else{
  assert.equal(final.result.success,true);assert.equal(final.result.summary.stream_id,input.stream_id);assert.equal(final.result.summary.source_frames,240);assert.equal(final.result.summary.observations,3);
  assert.deepEqual(observations.map(o=>o.sequence),[1,2,3]);assert.deepEqual(observations.map(o=>o.source_frames),[80,160,240]);
  assert.deepEqual(observations.map(o=>o.active),mode==='normal'?[null,true,false]:[true,null,null]);
  assert.deepEqual(observations.map(o=>o.level),mode==='normal'?[750,2000,100]:[2000,null,750]);
  assert.deepEqual(observations.map(o=>o.discontinuity),mode==='normal'?[false,false,false]:[false,true,false]);
 }
 cases.push({mode,success:final.result.success,observations:observations.length,device});
}
await writeFile(join(scratch,'results.json'),JSON.stringify({passed:true,cases,production_stream_transport:true,actual_energy_iterator:true,device_api_fake:true,physical_capture:false,workload_admission_integrated:false,ambient_capture_enabled:false,continuous_workloads_accepted:false,stable_release_accepted:false},null,2),{flag:'wx'});
console.log('Microphone source/transport fake-device validation passed: '+scratch);
