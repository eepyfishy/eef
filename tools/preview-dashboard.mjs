// Launch an isolated, loopback-only development preview. No installation changes.
import {mkdtemp, mkdir, readFile, writeFile} from 'node:fs/promises';
import {resolve, join} from 'node:path';
import {spawn} from 'node:child_process';
import {createServer} from 'node:net';
import {randomBytes} from 'node:crypto';

const root=resolve(import.meta.dirname,'..');
await mkdir(join(root,'.validation'),{recursive:true});
const scratch=await mkdtemp(join(root,'.validation','dashboard-preview-'));
const reservations=[];
async function reservePort(){
 const server=createServer();await new Promise((ok,fail)=>{server.once('error',fail);server.listen(0,'127.0.0.1',ok);});
 reservations.push(server);return server.address().port;
}
const eefPort=await reservePort(), nodePort=await reservePort(), gatewayPort=await reservePort();
const eefURL=`http://127.0.0.1:${eefPort}`,nodeURL=`http://127.0.0.1:${nodePort}`;
const nodeConfig=JSON.parse(await readFile(join(root,'config/node.example.json'),'utf8'));
nodeConfig.dashboard.port=nodePort;
nodeConfig.update.policy='off';nodeConfig.update.manifest_url='';
nodeConfig.models.provider='llamacpp';
nodeConfig.metadata={area:[],resources:[]};
await writeFile(join(scratch,'node.json'),JSON.stringify(nodeConfig,null,2));
const yaml=(await readFile(join(root,'config/default_identity.yaml'),'utf8'))
 .replace('port: 51334',`port: ${eefPort}`).replace('port: 51335',`port: ${gatewayPort}`)
 .replace('host: 0.0.0.0','host: 127.0.0.1').replace('policy: prompt','policy: off');
await writeFile(join(scratch,'eef.yaml'),yaml);
const environment={...process.env,
 PATH:join(root,'.tooling/llvm-mingw-20260616-ucrt-x86_64/bin')+';'+process.env.PATH,
 APPDATA:join(scratch,'appdata'),EEF_DISCOVERY_DIR:join(scratch,'discovery'),
 EEF_NODE_PSK:randomBytes(32).toString('hex')};
await Promise.all(reservations.map(server=>new Promise(ok=>server.close(ok))));
const children=[];
function launch(name,args){
 const child=spawn(join(root,'target/debug',name+'.exe'),args,{cwd:scratch,env:environment,windowsHide:true,detached:true,stdio:'ignore'});
 children.push(child);return new Promise((ok,fail)=>{child.once('error',fail);child.once('spawn',()=>{child.unref();ok(child);});});
}
try{
 await launch('eef',['--config',join(scratch,'eef.yaml'),'--database',join(scratch,'eef.db'),'--no-brain']);
 await launch('eefn',['--config',join(scratch,'node.json')]);
 const deadline=Date.now()+45000;
 let ready=false;
 while(Date.now()<deadline){
  try{
   const response=await fetch(nodeURL+'/api/status',{signal:AbortSignal.timeout(1500)});
   const status=await response.json();
   if(response.ok&&status.connection?.state==='connected'){ready=true;break;}
  }catch{}
  await new Promise(ok=>setTimeout(ok,300));
 }
 if(!ready)throw Error('Preview did not connect within 45 seconds.');
 const preview={node_url:nodeURL,eef_url:eefURL,pids:children.map(c=>c.pid),directory:scratch};
 await writeFile(join(scratch,'preview.json'),JSON.stringify(preview,null,2));
 console.log(JSON.stringify(preview,null,2));
}catch(error){
 for(const child of children)if(child.pid)child.kill();
 throw error;
}
