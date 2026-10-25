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
