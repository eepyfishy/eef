// Invoked by the existing required native source suite; no hardware/package/model.
import assert from 'node:assert/strict';
import {readFile,writeFile,mkdir,mkdtemp,stat} from 'node:fs/promises';
import {join,resolve} from 'node:path';
import {spawn} from 'node:child_process';
import {createHash,randomUUID} from 'node:crypto';

export async function verifyCameraMotion({root,python}) {
 assert.equal(process.platform,'win32','Owned streaming probe requires Windows');
 const scratch=await mkdtemp(join(root,'.validation/camera-motion-'));
 const bin=resolve(process.env.EEF_TEST_BINARY_DIR||join(root,'target/debug'));
 const helper=join(bin,'examples/python-stream-fixture.exe');await stat(helper);
 const sourcePath=join(root,'python/streams/camera_motion.py'),fakePath=join(root,'python/tests/camera_motion_fakes.py');
 const unitPath=join(root,'python/tests/test_camera_motion.py');
 const sources=await Promise.all([sourcePath,fakePath].map(p=>readFile(p,'utf8')));
 const pins=await Promise.all([python,helper,sourcePath,fakePath,unitPath,import.meta.filename].map(async path=>({path,
  sha256:createHash('sha256').update(await readFile(path)).digest('hex')})));
 await writeFile(join(scratch,'inputs.json'),JSON.stringify(pins,null,2),{flag:'wx'});
 async function execute(program,args,input,folder,env=process.env) {
  const child=spawn(program,args,{cwd:root,windowsHide:true,env,stdio:['pipe','pipe','pipe']});let out='',err='';
  child.stdout.on('data',b=>out+=b);child.stderr.on('data',b=>err+=b);child.stdin.on('error',()=>{});child.stdin.end(input);
  const timer=setTimeout(()=>child.kill(),10000);let code;
  try {code=await new Promise((ok,fail)=>{child.once('error',fail);child.once('close',ok);});}finally{clearTimeout(timer);}
  await writeFile(join(folder,'process-result.json'),JSON.stringify({code,out,err},null,2),{flag:'wx'});
  assert.equal(code,0,'Retain failed camera source fixture '+folder);return {out,err};
 }
 const unitDir=join(scratch,'unit');await mkdir(unitDir);
 const unit=await execute(python,['-I','-B','-X','utf8',unitPath],'',unitDir);
 assert.match(unit.err,/Ran 19 tests/);assert.match(unit.err,/\nOK\s*$/);
 // Execute the real source from owned bytes in the existing production stream
 // host. Only the device API is fake; no source file is reopened by this wrapper.
 const pinned=`import json,pathlib,sys,types
actual=types.ModuleType('reviewed_camera_motion')
exec(compile(${JSON.stringify(sources[0])},'<eef-camera-motion>','exec'),actual.__dict__)
fake=types.ModuleType('camera_motion_fakes')
exec(compile(${JSON.stringify(sources[1])},'<eef-camera-fakes>','exec'),fake.__dict__)
def stream(action,params):
    if action!='observe':raise ValueError('PRIVATE_CAMERA_ACTION')
    root=pathlib.Path(params['fixture_root']);mode=params['mode']
    frames=[fake.Frame(0),fake.Frame(20),fake.Frame(20)]
    if mode=='resize':frames[2]=fake.Frame(80,width=320,height=240)
    if mode=='read-loss':frames[1]=False
    camera=fake.Camera(frames)
    sys.modules['cv2']=fake.cv2(camera)
    iterator=actual.stream('motion',{'max_frames':3})
    try:yield from iterator
    finally:
        iterator.close()
        (root/'camera-result.json').write_text(json.dumps({'instances':camera.creations,'closed':camera.closed,'reads':camera.reads}))
`;
 const digest=createHash('sha256').update(pinned).digest('hex'),cases=[];
 assert(Buffer.byteLength(pinned)<48*1024);
 for(const mode of ['normal','resize','read-loss']) {
  const folder=join(scratch,mode);await mkdir(folder);
  const plugin=join(folder,'replaced-source.py'),canary=join(folder,'unreviewed-camera-opened.txt');
  await writeFile(plugin,`import pathlib\npathlib.Path(${JSON.stringify(canary)}).write_text('unreviewed')\nraise RuntimeError('PRIVATE_REPLACED_CAMERA')\n`);
  const input={python,plugin,stream_id:randomUUID(),params:{fixture_root:folder,mode},
   pinned_source:{source_utf8:pinned,source_sha256:digest},
   bounds:{max_frame_bytes:1024,max_stdout_bytes:32768,max_observations:16,max_source_frames:100},
   limits:{timeout_seconds:8,max_request_bytes:65536,max_stdout_bytes:32768,max_stderr_bytes:1024,max_job_memory_mib:128},
   execution_ms:6000,cancel_marker:null,observer_rejection_marker:null};
  await writeFile(join(folder,'request.json'),JSON.stringify(input,null,2),{flag:'wx'});
  const {out,err}=await execute(helper,[],JSON.stringify(input),folder,{...process.env,
   PATH:join(bin,'../../.tooling/llvm-mingw-20260616-ucrt-x86_64/bin')+';'+process.env.PATH});
  assert(!/PRIVATE_CAMERA|PRIVATE_REPLACED_CAMERA|data_base64|image\/jpeg|rtsp:\/\//.test(out+err));
  assert.equal(await stat(canary).then(()=>true,()=>false),false);
  const events=out.trim().split('\n').map(line=>JSON.parse(line)),finals=events.filter(e=>e.event_type==='result');
  assert.equal(finals.length,1);const final=finals[0];assert.equal(final,events.at(-1));
  assert.equal(final.physical_capture,false);assert.equal(final.workload_authorized,false);assert.equal(final.external_effects_stopped,false);
  const observed=events.filter(e=>e.event_type==='observation').map(e=>e.observation);
  for(const event of observed)assert.deepEqual(Object.keys(event).sort(),['active','discontinuity','level','sequence','source_frames']);
  const device=JSON.parse(await readFile(join(folder,'camera-result.json'),'utf8'));
  assert.deepEqual(device,{instances:1,closed:1,reads:mode==='read-loss'?2:3});
  assert.equal(final.result.success,mode!=='read-loss');
  if(mode==='read-loss') {
   assert.equal(observed.length,1);assert(['Reply','Process'].includes(final.result.failure));
  }else {
   assert.equal(final.result.summary.stream_id,input.stream_id);assert.equal(final.result.summary.observations,3);
   assert.equal(final.result.summary.source_frames,3);assert.deepEqual(observed.map(o=>o.sequence),[1,2,3]);
   assert.deepEqual(observed.map(o=>o.source_frames),[1,2,3]);
   assert.deepEqual(observed.map(o=>o.active),mode==='normal'?[null,true,false]:[null,true,null]);
   assert.deepEqual(observed.map(o=>o.level),mode==='normal'?[null,20,0]:[null,20,null]);
   assert.deepEqual(observed.map(o=>o.discontinuity),mode==='normal'?[false,false,false]:[false,false,true]);
  }
  cases.push({mode,device,observations:observed.length,success:final.result.success});
 }
 for(const path of ['python/streams/__pycache__','python/tests/__pycache__'])
  assert.equal(await stat(join(root,path)).then(()=>true,()=>false),false);
 await writeFile(join(scratch,'results.json'),JSON.stringify({passed:true,unit_cases:19,cases,
  actual_source_and_owned_transport:true,pinned_bytes_executed:true,replaced_source_not_opened:true,
  device_api_fake:true,real_opencv_tested:false,physical_capture:false,camera_workload_admission:false,
  local_vlm_trigger_integrated:false,ambient_capture_enabled:false,stable_release_accepted:false},null,2),{flag:'wx'});
 console.log('Camera motion source and transport fake-device validation passed: '+scratch);
}
