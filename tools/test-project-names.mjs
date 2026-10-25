import assert from 'node:assert/strict';
import {violations} from './check-project-names.mjs';

assert.deepEqual(violations('README.md','eef and eefn.\neef starts a sentence.'),[]);
for(const word of ['eef','eefn']){
 for(let mask=1;mask<(1<<word.length);mask++){
  const variant=[...word].map((letter,i)=>mask&(1<<i)?letter.toUpperCase():letter).join('');
  assert.equal(violations('README.md',`# ${variant}\n`).length,1);
  assert.equal(violations('dashboard/app.js',`const message='Restart ${variant}.';`).length,1);
  assert.equal(violations('crates/eef/src/lib.rs',`//! ${variant} coordinator.`).length,1);
 }
}
assert.deepEqual(violations('example.rs','const EEF_NODE_PSK: &str = "eef";'),[]);
const prefix='const PREFIX: &str = "\u0045EF model verification v1\\n";';
assert.deepEqual(violations('crates/eefn/src/model_verification.rs',prefix),[]);
assert.equal(violations('README.md',prefix).length,1);
assert.equal(violations('crates/eefn/src/model_verification.rs',prefix+'\n// '+ 'eef'.toUpperCase()).length,1);
assert.deepEqual(violations('crates/eefn/src/package_health.rs','marker.name == "\u0045EF"'),[]);
assert.deepEqual(violations('crates/eefn/src/setup.rs','.join("\u0045EF Connection")'),[]);
console.log('Lowercase project-name regression passed: mixed-case variants and exact legacy boundaries');
