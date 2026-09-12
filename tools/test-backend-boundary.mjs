// Architecture guard: runtime services cannot import a browser or HTTP adapter.
import assert from 'node:assert/strict';
import {readFile,readdir} from 'node:fs/promises';
import {resolve,join} from 'node:path';
const root=resolve(import.meta.dirname,'..');
const adapters=new Set(['api.rs','dashboard.rs','web_ui.rs','main.rs','lib.rs']);
for(const role of ['eef','eefn']){
 const dir=join(root,'crates',role,'src');
 for(const file of await readdir(dir)){
  if(!file.endsWith('.rs')||adapters.has(file))continue;
  const source=await readFile(join(dir,file),'utf8');
  assert(!/\b(?:crate|eefn)::(?:api|dashboard|web_ui)::/.test(source),`${role}/${file} imports an adapter`);
  assert(!/\bNodeDashboard\b/.test(source),`${role}/${file} depends on dashboard state`);
  assert(!/include_(?:str|bytes)!\([^\n]*dashboard\//.test(source),`${role}/${file} embeds browser assets`);
 }
 const cargo=await readFile(join(root,'crates',role,'Cargo.toml'),'utf8');
 assert(cargo.includes('default = ["dashboard"]')&&cargo.includes('dashboard = []'),`${role} needs optional browser assets`);
}
console.log('Backend-to-browser dependency boundary passed (source check; compile/integration tests are separate)');
