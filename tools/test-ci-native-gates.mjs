// Closed expressions and canonical runner outcomes only, not a general GitHub
// expression/YAML interpreter. No processes, installers, models or network work.
import assert from 'node:assert/strict';
import {readFile} from 'node:fs/promises';
const workflow=(await readFile(new URL('../.github/workflows/verify.yml',import.meta.url),'utf8')).replace(/\r\n/g,'\n');
function step(name){const result=workflow.split(/^      - name: /m).find(block=>block.startsWith(name+'\n'));assert(result,'Missing guarded step');return result;}
const tests=step('Workspace and all-target regression tests'),build=step('Build Windows command services and isolated helpers'),native=step('Native Windows synthetic regressions (no installer or model download)');
assert.match(tests,/^        id: workspace_tests$/m);assert.match(build,/^        id: native_build$/m);
assert.match(tests,/cargo test --workspace --all-targets @features --locked --offline --quiet -j2\n          exit \$LASTEXITCODE/);
assert.match(build,/cargo build --workspace --bins --examples --no-default-features --locked --offline --quiet -j2\n          exit \$LASTEXITCODE/);
assert(!/^\s*continue-on-error\s*:/m.test(workflow),'No failure gate may be converted to success');
function condition(block,expected){
 const value=/^        if: \$\{\{ (.*) \}\}$/m.exec(block)?.[1];assert.equal(value,expected,'Review changed condition semantics explicitly');
 // Equality operands are only canonical strings in the table below. The exact
 // expression above is checked before evaluating its shared Boolean operators.
 return new Function('matrix','steps','cancelled',`"use strict";return Boolean(${value});`);
}
const canBuild=condition(build,"!cancelled() && matrix.os == 'windows-2025' && matrix.mode == 'headless' && (steps.workspace_tests.outcome == 'success' || steps.workspace_tests.outcome == 'failure')");
const canRun=condition(native,"!cancelled() && matrix.os == 'windows-2025' && matrix.mode == 'headless' && steps.native_build.outcome == 'success'");
let checked=0;
for(const os of ['windows-2025','ubuntu-24.04'])for(const mode of ['headless','dashboard'])for(const cancelled of [false,true])for(const rust of ['success','failure','skipped','cancelled','','unknown'])for(const fresh of ['success','failure','skipped','cancelled','','unknown']){
 const matrix={os,mode},steps={workspace_tests:{outcome:rust},native_build:{outcome:fresh}},eligible=!cancelled&&os==='windows-2025'&&mode==='headless';
 assert.equal(canBuild(matrix,steps,()=>cancelled),eligible&&['success','failure'].includes(rust));
 assert.equal(canRun(matrix,steps,()=>cancelled),eligible&&fresh==='success');checked++;
 const reachable={...steps,native_build:{outcome:canBuild(matrix,steps,()=>cancelled)?fresh:'skipped'}};
 if(!canBuild(matrix,steps,()=>cancelled))assert.equal(canRun(matrix,reachable,()=>cancelled),false,'Skipped prerequisites must not reach native execution');
}
const scripts=[...native.matchAll(/'(test-[\w-]+\.mjs)'/g)].map(match=>match[1]);assert.equal(scripts.length,32);assert.equal(new Set(scripts).size,32);
assert(scripts.includes('test-setup-model-check.mjs'));
assert(scripts.includes('test-coordinator-initialization.mjs'));assert(scripts.includes('test-bundled-components.mjs'));assert(scripts.includes('test-python-bridge.mjs'));
assert.match(native,/EEF_TEST_NODE_OUTPUT_PREFIX_CLOSE = '1'/);
assert.match(native,/foreach \(\$script in \$scripts\)[\s\S]*?if \(\$LASTEXITCODE -ne 0\) \{ exit \$LASTEXITCODE \}/);
console.log(`CI native gate checks passed: ${checked} combinations; all failure and native acceptance gates retained`);
