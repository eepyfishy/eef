// Pure, bounded version-policy regression. No filesystem payload or MSI actions.
import assert from 'node:assert/strict';
import {spawnSync} from 'node:child_process';
import {join} from 'node:path';
import {VERSION_POLICY,sourceVersionPolicy,validateMsiVersion,verifyVersionPair} from './msi-version-policy.mjs';
const cases=[];
function test(name,fn){fn();cases.push(name);}
const mapped=source=>sourceVersionPolicy(source).product_version;
const compare=(a,b)=>{const x=a.split('.').map(Number),y=b.split('.').map(Number);for(let i=0;i<3;i++)if(x[i]!==y[i])return Math.sign(x[i]-y[i]);return 0;};
test('explicit-policy-and-no-authorization',()=>{const p=sourceVersionPolicy('0.4.0-alpha.6');assert.equal(p.version_policy,VERSION_POLICY);assert.equal(VERSION_POLICY,'packed-patch-v1');assert.equal(p.source_is_prerelease,true);assert.equal(p.product_version,'0.4.6');for(const key of ['automatic_update_authorized','installation_requested','release_accepted'])assert.equal(p[key],false);});
test('known-phase-and-patch-boundaries',()=>{for(const [source,target] of [['0.4.0-alpha.1','0.4.1'],['0.4.0-alpha.63','0.4.63'],['0.4.0-beta.1','0.4.65'],['0.4.0-beta.63','0.4.127'],['0.4.0-rc.1','0.4.129'],['0.4.0-rc.63','0.4.191'],['0.4.0','0.4.255'],['0.4.1-alpha.1','0.4.257'],['0.4.1','0.4.511'],['255.255.255','255.255.65535']])assert.equal(mapped(source),target);});
test('stable-source-is-not-release-approval',()=>{const p=sourceVersionPolicy('0.4.0');assert.equal(p.source_channel,'stable');assert.equal(p.source_is_prerelease,false);assert.equal(p.automatic_update_authorized,false);assert.equal(p.release_accepted,false);});
let enumerated=0;
test('all-patch-phase-sequence-ordinals-are-unique-and-increasing',()=>{
 let previous=null;const seen=new Set();
 for(let patch=0;patch<=255;patch++)for(const channel of ['alpha','beta','rc','stable'])for(let n=1;n<=(channel==='stable'?1:63);n++){
  const source=`0.4.${patch}`+(channel==='stable'?'':`-${channel}.${n}`),value=mapped(source);
  assert(!seen.has(value));seen.add(value);if(previous)assert.equal(compare(previous,value),-1);previous=value;enumerated++;
 }
 assert.equal(enumerated,48640);assert.equal(previous,'0.4.65535');
});
test('minor-and-major-boundaries-outrank-all-patch-slots',()=>{for(const component of [0,1,127,254]){assert.equal(compare(mapped(`0.${component}.255`),mapped(`0.${component+1}.0-alpha.1`)),-1);assert.equal(compare(mapped(`${component}.255.255`),mapped(`${component+1}.0.0-alpha.1`)),-1);}});
test('unrepresentable-components-fail-without-wrapping',()=>{for(const source of ['256.0.0','0.256.0','0.0.256','9007199254740993.0.0','0.0.999999999999999999999999999999999999'])assert.throws(()=>mapped(source),/source_version_component_out_of_range/);});
test('prerelease-overflow-fails-instead-of-changing-channel',()=>{for(const channel of ['alpha','beta','rc'])for(const sequence of ['64','255','99999999999999999999999999999999999999'])assert.throws(()=>mapped(`0.4.0-${channel}.${sequence}`),/source_prerelease_sequence_out_of_range/);});
test('noncanonical-source-is-rejected',()=>{for(const source of [undefined,null,0,[],{},'', ' 0.4.0','0.4.0 ','0.4.0\n','0.4.0\r\n','v0.4.0','00.4.0','0.04.0','0.4.00','0.4.0-alpha.0','0.4.0-alpha.01','0.4.0-alpha','0.4.0-preview.1','0.4.0-ALPHA.1','0.4.0-rc.1.extra','0.4.0+build.1','0.4.0.1','x'.repeat(65)])assert.throws(()=>sourceVersionPolicy(source),/source_version_invalid/);});
test('numeric-msi-format-and-platform-limits',()=>{assert.deepEqual(validateMsiVersion('255.255.65535'),[255,255,65535]);for(const value of [undefined,null,4,'','1.2','1.2.3.4','01.2.3','1.2.03','1.2.3\n','1.2.3-alpha.1',' 1.2.3'])assert.throws(()=>validateMsiVersion(value),/explicit_three_field_msi_version_required/);for(const value of ['256.0.0','0.256.0','0.0.65536'])assert.throws(()=>validateMsiVersion(value),/msi_version_out_of_range/);});
test('explicit-pair-cannot-relabel-source-or-choose-arbitrary-ordinal',()=>{assert.equal(verifyVersionPair('1.2.3-alpha.1','1.2.769').product_version,'1.2.769');for(const value of ['1.2.1','1.2.770','1.2.1023','1.2.1025'])assert.throws(()=>verifyVersionPair('1.2.3-alpha.1',value),/msi_version_does_not_match_source/);});
const cli=args=>spawnSync(process.execPath,[join(import.meta.dirname,'msi-version-policy.mjs'),...args],{encoding:'utf8',windowsHide:true,timeout:10000});
test('cli-derives-and-verifies-same-policy',()=>{for(const args of [['--source-version','0.4.0'],['--source-version','0.4.0','--msi-version','0.4.255']]){const r=cli(args);assert.equal(r.status,0,r.stderr);assert.deepEqual(JSON.parse(r.stdout),sourceVersionPolicy('0.4.0'));}});
test('cli-errors-do-not-echo-private-inputs',()=>{for(const args of [['--source-version','PRIVATE-invalid-input'],['--source-version','0.4.0','--msi-version','0.4.0'],['--PRIVATE-path=C:/private/owner']]){const r=cli(args);assert.equal(r.status,1);assert.equal(r.stdout,'');assert.match(r.stderr,/^[a-z][a-z0-9_]+\r?\n$/);assert(!r.stderr.includes('PRIVATE'));}});
test('cli-help-is-read-only',()=>{const r=cli(['--help']);assert.equal(r.status,0);assert.match(r.stdout,/Read-only packed-patch-v1/);});
assert.equal(cases.length,13);
console.log('MSI version policy case count: '+cases.length);
console.log('MSI version policy ordinals checked: '+enumerated);
console.log('MSI version policy checks passed (read-only; no installer actions)');
