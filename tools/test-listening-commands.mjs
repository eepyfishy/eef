// Real local CLI/API + owned Python root/child, synthetic source only. No model,
// hardware, installation, private configuration or external network listener.
import assert from 'node:assert/strict';
import {mkdtemp,readFile,writeFile,stat} from 'node:fs/promises';
import {join,resolve} from 'node:path';
import {spawn} from 'node:child_process';
import {createServer} from 'node:http';
import {createHash} from 'node:crypto';
assert.equal(process.platform,'win32');
const root=resolve(import.meta.dirname,'..'),bin=resolve(process.env.EEF_TEST_BINARY_DIR||join(root,'target/debug')),python=process.env.EEF_TEST_PYTHON;
assert(python,'Supply an existing trusted interpreter');await stat(python);
const scratch=await mkdtemp(join(root,'.validation/listening-commands-')),children=[],plugin=join(scratch,'source.py'),config=join(scratch,'node.json');
const env={...process.env,EEF_NODE_PSK:'',EEF_DISCOVERY_DIR:join(scratch,'discovery'),APPDATA:join(scratch,'appdata'),PATH:join(bin,'../../.tooling/llvm-mingw-20260616-ucrt-x86_64/bin')+';'+process.env.PATH};
await writeFile(plugin,`import ctypes,json,os,pathlib,subprocess,sys,time
from ctypes import wintypes
def stream(action,params):
    root=pathlib.Path(params['root'])
    kernel=ctypes.WinDLL('kernel32',use_last_error=True)
    kernel.GetCurrentProcess.restype=wintypes.HANDLE
    kernel.GetProcessTimes.argtypes=[wintypes.HANDLE]+[ctypes.POINTER(wintypes.FILETIME)]*4
    kernel.GetProcessTimes.restype=wintypes.BOOL
    def created(handle):
        times=[wintypes.FILETIME() for _ in range(4)]
        if not kernel.GetProcessTimes(handle,*[ctypes.byref(v) for v in times]):raise OSError(ctypes.get_last_error())
        return (times[0].dwHighDateTime<<32)|times[0].dwLowDateTime
    code="import pathlib,time; time.sleep(30); pathlib.Path("+repr(str(root/'late-child-effect'))+").write_text('unexpected')"
    child=subprocess.Popen([sys.executable,'-I','-B','-c',code],stdin=subprocess.DEVNULL,stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL)
    (root/'pids.json').write_text(json.dumps({'root':{'pid':os.getpid(),'created':created(kernel.GetCurrentProcess())},'child':{'pid':child.pid,'created':created(wintypes.HANDLE(int(child._handle)))}}))
    yield {'source_frames':80,'active':None,'level':None,'discontinuity':True}
    time.sleep(30)
    yield {'source_frames':160,'active':False,'level':0,'discontinuity':False}
`);
function launch(exe,args,input){const child=spawn(exe,args,{cwd:root,env,windowsHide:true,stdio:[input===undefined?'ignore':'pipe','pipe','pipe']});children.push(child);let out='',err='';child.stdout.on('data',b=>out+=b);child.stderr.on('data',b=>err+=b);child.logs=()=>({out,err});child.result=new Promise((r,j)=>{child.once('error',j);child.once('close',code=>r({code,out,err}));});if(input!==undefined){child.stdin.on('error',()=>{});child.stdin.end(JSON.stringify(input));}return child;}
const exists=path=>stat(path).then(()=>true,()=>false);
async function until(fn,label){const end=Date.now()+15000;while(Date.now()<end){if(await fn())return;await new Promise(r=>setTimeout(r,10));}throw Error('Timeout '+label+'; retain '+scratch);}
const commandResults=[];
async function cli(action,success=true){const child=launch(join(bin,'eefn.exe'),['--config',config,'--json','listening',action]);const timer=setTimeout(()=>child.kill(),15000);let result;try{result=await child.result;}finally{clearTimeout(timer);}commandResults.push({action,...result});assert.equal(result.code,success?0:1,'CLI result; retain '+scratch);return JSON.parse(result.out);}
let writes=0,api;
const proxy=createServer(async(req,res)=>{try{let text='';for await(const b of req)text+=b;const body=text&&JSON.parse(text),mutation=body?.command?.operation==='set';if(mutation)writes++;const response=await fetch(api+req.url,{method:req.method,headers:{'Content-Type':'application/json'},body:text||undefined,signal:AbortSignal.timeout(10000)});const value=await response.text();if(mutation)return res.destroy();res.writeHead(response.status,{'Content-Type':'application/json'});res.end(value);}catch{res.destroy();}});
try{
 const helper=launch(join(bin,'examples/listening-fixture.exe'),[],{root:scratch,python,plugin});await until(()=>exists(join(scratch,'ready')),'local API ready');
 const marker=config+'.api.json',original=await readFile(marker,'utf8'),endpoint=JSON.parse(original);api='http://'+endpoint.address;
 const initial=await cli('show');assert.equal(initial.result.consent.enabled,false);assert.equal(initial.result.capture_integration_available,false);assert.equal(await exists(join(scratch,'pids.json')),false);
 const enabled=await cli('enable');assert.equal(enabled.result.consent.enabled,true);assert.equal(enabled.result.capture_started,false);assert.equal(await exists(join(scratch,'pids.json')),false,'Consent must not start a source');
 await writeFile(join(scratch,'begin'),'ready');await until(()=>exists(join(scratch,'observation')),'owned source observation');
 const identities=await readFile(join(scratch,'pids.json'),'utf8'),observer=launch(join(bin,'examples/process-fixture.exe'),['observe',scratch]);await until(()=>exists(join(scratch,'observer-ready')),'creation-verified process handles');
 const beforeOff=Date.now();const disabled=await cli('disable');assert.equal(disabled.result.consent.enabled,false);assert.equal(disabled.result.runtime_blocked,true);
 await until(()=>exists(join(scratch,'supervision-result.json')),'supervised future retired');const result=JSON.parse(await readFile(join(scratch,'supervision-result.json'),'utf8'));assert.equal(result.revoked,true);assert.equal(result.physical_capture,false);assert.equal(result.workload_authorized,false);assert.equal(result.external_effects_stopped,false);assert(Date.now()-beforeOff<5000);assert(result.elapsed_ms<20000);
 const held=await observer.result;await writeFile(join(scratch,'held-observer-result.json'),JSON.stringify(held,null,2));assert.equal(held.code,0);const proof=JSON.parse(held.out);assert.equal(proof.both_exited,true);assert.equal(proof.held_creation_verified_handles,true);assert.equal(await exists(join(scratch,'late-child-effect')),false);
 const after=await cli('show');assert.equal(after.result.runtime_id,enabled.result.runtime_id);assert.equal(after.result.consent.enabled,false);assert.equal((await fetch(api+'/api/diagnostics').then(r=>r.json())).success,true);
 await cli('enable');assert.equal(await readFile(join(scratch,'pids.json'),'utf8'),identities,'No replay from consent changes');
 await new Promise(r=>proxy.listen(0,'127.0.0.1',r));endpoint.address='127.0.0.1:'+proxy.address().port;await writeFile(marker,JSON.stringify(endpoint));
 try{const lost=await cli('disable',false);assert.equal(lost.outcome_unknown,true);assert.equal(lost.automatically_retried,false);assert.equal(writes,1);}finally{await writeFile(marker,original);}
 const final=await cli('show');assert.equal(final.result.consent.enabled,false);assert.equal(final.result.runtime_id,enabled.result.runtime_id);
 const denied=await fetch(api+'/api/commands/listening',{method:'POST',headers:{'Content-Type':'application/json','Origin':'https://example.invalid'},body:JSON.stringify({schema_version:1,expected_node_id:'listening-fixture',expected_runtime_id:result.runtime_id,command:{operation:'show'}})});assert.equal(denied.status,403);
 await writeFile(join(scratch,'finish'),'ready');assert.equal((await helper.result).code,0);
 await writeFile(join(scratch,'command-results.json'),JSON.stringify(commandResults,null,2));
 const pins=await Promise.all([python,join(bin,'eefn.exe'),join(bin,'examples/listening-fixture.exe'),join(bin,'examples/process-fixture.exe'),plugin,import.meta.filename].map(async path=>({path,sha256:createHash('sha256').update(await readFile(path)).digest('hex')})));
 await writeFile(join(scratch,'inputs.json'),JSON.stringify(pins,null,2));await writeFile(join(scratch,'results.json'),JSON.stringify({passed:true,real_local_cli_and_api:true,consent_does_not_start_capture:true,one_supervised_owned_root_and_child:true,held_creation_verified_exit:true,disable_retires_before_original_deadline:true,services_and_runtime_remain_available:true,off_on_does_not_replay:true,lost_ack_one_write_no_retry:true,cross_origin_refused:true,physical_capture:false,workload_authority_integrated:false,continuous_workloads_accepted:false,stable_release_accepted:false},null,2));
 console.log('Local listening consent and owned revocation validation passed: '+scratch);
}catch(error){await writeFile(join(scratch,'PRIVATE-failure.json'),JSON.stringify({error:String(error),commands:commandResults,processes:children.map(c=>({pid:c.pid,code:c.exitCode,...c.logs()}))},null,2));throw error;}
finally{proxy.closeAllConnections();if(proxy.listening)await new Promise(r=>proxy.close(r));for(const child of children)if(child.exitCode===null)child.kill();await Promise.allSettled(children.map(c=>c.result));}
