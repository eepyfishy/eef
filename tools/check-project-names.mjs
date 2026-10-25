// Read-only branding check. Legacy machine interfaces are not display branding.
import {readFileSync} from 'node:fs';
import {execFileSync} from 'node:child_process';
import {resolve} from 'node:path';
import {pathToFileURL} from 'node:url';

const legacy = [
 ['crates/eefn/src/model_verification.rs', 'const PREFIX: &str = "\u0045EF model verification v1\\n";'],
 ['crates/eefn/src/package_health.rs', 'marker.name == "\u0045EF"'],
 ['crates/eefn/src/setup.rs', '.join("\u0045EF Connection")'],
];

export function violations(path, text) {
 const exceptions=legacy.filter(([file])=>file===path).flatMap(([,literal])=>{
  const spans=[];let start=0;
  while((start=text.indexOf(literal,start))!==-1){spans.push([start,start+literal.length]);start+=literal.length;}
  return spans;
 });
 return [...text.matchAll(/\beefn?\b/gi)].filter(match=>match[0]!==match[0].toLowerCase()&&!exceptions.some(([start,end])=>match.index>=start&&match.index+match[0].length<=end))
  .map(match=>({path,line:text.slice(0,match.index).split('\n').length,expected:match[0].toLowerCase()}));
}

export function check(root) {
 const paths=execFileSync('git',['ls-files','--cached','--others','--exclude-standard','-z'],{cwd:root,encoding:'utf8'}).split('\0')
  .filter(path=>path&&!path.startsWith('docs/future/')&&(/\.(?:md|rs|toml|yaml|yml|json|html|js|mjs|py|ps1|txt|css)$/.test(path)||path==='LICENSE'));
 const results=[];let files=0;
 for(const path of [...new Set(paths)]){
  let source;try{source=readFileSync(resolve(root,path),'utf8');}catch(error){if(error.code==='ENOENT')continue;throw error;}
  files++;results.push(...violations(path,source));
 }
 return {files,violations:results};
}

if(process.argv[1]&&import.meta.url===pathToFileURL(resolve(process.argv[1])).href){
 const result=check(resolve(import.meta.dirname,'..'));
 for(const issue of result.violations)process.stderr.write(`${issue.path}:${issue.line}: use lowercase ${issue.expected}\n`);
 process.stdout.write(JSON.stringify({passed:result.violations.length===0,files:result.files,violations:result.violations.length})+'\n');
 if(result.violations.length)process.exitCode=1;
}
