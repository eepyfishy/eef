// Same-PC fixtures must not inherit production's LAN-facing node gateway.
// This intentionally accepts only the reviewed template layout, not arbitrary YAML.
import {readFile} from 'node:fs/promises';

export function loopbackCoordinatorTemplate(source){
 if(typeof source!=='string')throw new TypeError('Expected coordinator template text');
 const lines=source.split('\n');
 function hostLine(section){
  const headers=lines.flatMap((line,i)=>new RegExp('^'+section+'\\s*:').test(line)?[i]:[]);
  if(headers.length!==1||lines[headers[0]].replace(/\r$/,'')!==section+':')
   throw Error('Unexpected '+section+' section in local test template');
  let end=headers[0]+1;
  while(end<lines.length&&(/^[ \t]/.test(lines[end])||/^\s*$/.test(lines[end])||/^#/.test(lines[end])))end++;
  const hosts=[];
  for(let i=headers[0]+1;i<end;i++)if(/^[ \t]*host\s*:/.test(lines[i]))hosts.push(i);
  if(hosts.length!==1||!/^  host: (?:0\.0\.0\.0|127\.0\.0\.1)\r?$/.test(lines[hosts[0]]))
   throw Error('Unexpected '+section+' host in local test template');
  return hosts[0];
 }
 const web=hostLine('web'),node=hostLine('node');
 if(!/^  host: 127\.0\.0\.1\r?$/.test(lines[web]))throw Error('Local test owner API must already use loopback');
 lines[node]=lines[node].replace('0.0.0.0','127.0.0.1');
 return lines.join('\n');
}

export async function readLoopbackCoordinatorTemplate(){
 return loopbackCoordinatorTemplate(await readFile(new URL('../config/default_identity.yaml',import.meta.url),'utf8'));
}

// Replace both endpoints by their section, in one pass. Chained text replacement
// can rewrite the newly inserted web port when it equals the old node port.
export function coordinatorTemplatePorts(source,webPort,nodePort){
 const local=loopbackCoordinatorTemplate(source),lines=local.split('\n');
 if(![webPort,nodePort].every(p=>Number.isInteger(p)&&p>0&&p<=65535)||webPort===nodePort)
  throw Error('Local fixture endpoints require distinct valid ports');
 for(const [section,port] of [['web',webPort],['node',nodePort]]){
  const start=lines.findIndex(line=>line.replace(/\r$/,'')===section+':');
  let end=start+1;
  while(end<lines.length&&(/^[ \t]/.test(lines[end])||/^\s*$/.test(lines[end])||/^#/.test(lines[end])))end++;
  const ports=[];
  for(let i=start+1;i<end;i++)if(/^[ \t]*port\s*:/.test(lines[i]))ports.push(i);
  if(ports.length!==1||!/^  port: [0-9]+\r?$/.test(lines[ports[0]]))throw Error('Unexpected '+section+' port in local fixture');
  const newline=lines[ports[0]].endsWith('\r')?'\r':'';
  lines[ports[0]]='  port: '+port+newline;
 }
 return lines.join('\n');
}
