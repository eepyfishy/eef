// Pure fixture isolation checks. No app, listener, firewall mutation or elevation.
import assert from 'node:assert/strict';
import {readFile,readdir} from 'node:fs/promises';
import {loopbackCoordinatorTemplate,readLoopbackCoordinatorTemplate,coordinatorTemplatePorts} from './local-test-network.mjs';

const original=await readFile(new URL('../config/default_identity.yaml',import.meta.url),'utf8');
const local=await readLoopbackCoordinatorTemplate();
assert.equal(local,original.replace('  host: 0.0.0.0','  host: 127.0.0.1'));
assert.equal(loopbackCoordinatorTemplate(local),local);
assert(original.includes('node:\n  host: 0.0.0.0')||original.includes('node:\r\n  host: 0.0.0.0'),'Production LAN gateway must remain unchanged');
let accepted=0,rejected=0;
for(const newline of ['\n','\r\n']){
 const source=original.replace(/\r?\n/g,newline);
 assert.equal(loopbackCoordinatorTemplate(source),source.replace('  host: 0.0.0.0','  host: 127.0.0.1'));accepted++;
 const bad=[
  source.replace('node:','nodes:'),
  source+'\nnode:\n  host: 0.0.0.0\n',
  source.replace('node:','node: # changed layout'),
  source.replace('  host: 0.0.0.0','  host: 192.0.2.1'),
  source.replace('  host: 0.0.0.0','  host: ::'),
  source.replace('  host: 0.0.0.0','  host: localhost'),
  source.replace('  host: 0.0.0.0','  host: "127.0.0.1"'),
  source.replace('  host: 0.0.0.0','  port: 51335'),
  source.replace('  host: 0.0.0.0','  host: 0.0.0.0\n  host: 127.0.0.1'),
  source.replace('  host: 0.0.0.0','    host: 0.0.0.0'),
  source.replace('  host: 0.0.0.0','\thost: 0.0.0.0'),
  source.replace('web:','webs:'),
  source+'\nweb:\n  host: 127.0.0.1\n',
  source.replace('  host: 127.0.0.1','  host: 0.0.0.0'),
  source.replace('  host: 127.0.0.1','  host: 127.0.0.1\n  host: 127.0.0.1'),
 ];
 for(const value of bad){assert.throws(()=>loopbackCoordinatorTemplate(value));rejected++;}
}
assert.throws(()=>loopbackCoordinatorTemplate(null),TypeError);rejected++;
// Exercise exact replacement collisions without opening sockets or starting apps.
let portCases=0;
for(const newline of ['\n','\r\n']){
 const source=original.replace(/\r?\n/g,newline);
 for(const [web,node] of [[51335,51334],[51335,60123],[60123,51334],[51334,51335]]){
  const actual=coordinatorTemplatePorts(source,web,node);
  const expected=loopbackCoordinatorTemplate(source).split('\n');
  let section;
  for(let i=0;i<expected.length;i++){
   if(/^[a-z_]+:/.test(expected[i]))section=expected[i].split(':')[0];
   if((section==='web'||section==='node')&&/^  port:/.test(expected[i]))
    expected[i]='  port: '+(section==='web'?web:node)+(expected[i].endsWith('\r')?'\r':'');
  }
  assert.equal(actual,expected.join('\n'));portCases++;
 }
 for(const [web,node] of [[0,51335],[51334,65536],[51335,51335],[1.5,2],['51334',51335]]){
  assert.throws(()=>coordinatorTemplatePorts(source,web,node));portCases++;
 }
 for(const bad of [source.replace('  port: 51334','  port: "51334"'),source.replace('  port: 51335','  port: 51335'+newline+'  port: 51336')]){
  assert.throws(()=>coordinatorTemplatePorts(bad,60123,60124));portCases++;
 }
}
// Guard the migrated read sites; genuine no-config first initialization and
// deliberately exposed owner-API rejection tests remain separate unchanged gates.
let migrated=0;
for(const name of await readdir(new URL('.',import.meta.url))){
 if(!/^test-.*\.mjs$/.test(name)||name==='test-local-test-network.mjs')continue;
 const source=await readFile(new URL(name,import.meta.url),'utf8');
 assert(!/await readFile\(join\(root, ?'config\/default_identity.yaml'\), ?'utf8'\)/.test(source),name+' must use the local fixture helper');
 if(source.includes("from './local-test-network.mjs'"))migrated++;
}
assert(migrated>=36,'All existing template-based same-PC fixtures must be isolated');
console.log(`Local test network validation passed: ${accepted} newline cases, ${rejected} refusals, ${portCases} port assignment cases, ${migrated} fixture readers; production networking unchanged`);
