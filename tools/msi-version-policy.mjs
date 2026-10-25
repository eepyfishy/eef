// Packaging protocol, not a runtime preference or update/release authorization.
import {parseArgs} from 'node:util';
import {resolve} from 'node:path';
import {pathToFileURL} from 'node:url';

export const VERSION_POLICY='packed-patch-v1';
const STRIDE=256,MAX_COMPONENT=255,MAX_PRERELEASE=63;
const OFFSETS=Object.freeze({alpha:0,beta:64,rc:128});
function requireValue(value,code){if(!value)throw Error(code);}

export function validateMsiVersion(value){
 requireValue(typeof value==='string'&&value.length<=16,'explicit_three_field_msi_version_required');
 const match=value.match(/^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)$/);
 requireValue(match&&match[0]===value,'explicit_three_field_msi_version_required');
 const fields=match.slice(1).map(Number);
 requireValue(fields[0]<=255&&fields[1]<=255&&fields[2]<=65535,'msi_version_out_of_range');
 return fields;
}

export function sourceVersionPolicy(source){
 requireValue(typeof source==='string'&&source.length>0&&source.length<=64,'source_version_invalid');
 const match=source.match(/^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)(?:-(alpha|beta|rc)\.([1-9][0-9]*))?$/);
 requireValue(match&&match[0]===source,'source_version_invalid');
 const [major,minor,patch]=match.slice(1,4).map(Number),channel=match[4]||'stable',sequence=match[5]?Number(match[5]):null;
 requireValue([major,minor,patch].every(n=>Number.isSafeInteger(n)&&n<=MAX_COMPONENT),'source_version_component_out_of_range');
 requireValue(sequence===null||(Number.isSafeInteger(sequence)&&sequence<=MAX_PRERELEASE),'source_prerelease_sequence_out_of_range');
 const ordinal=channel==='stable'?255:OFFSETS[channel]+sequence;
 const productVersion=[major,minor,patch*STRIDE+ordinal].join('.');validateMsiVersion(productVersion);
 return {schema_version:1,report_type:'msi_version_policy',version_policy:VERSION_POLICY,source_version:source,
  product_version:productVersion,source_channel:channel,source_is_prerelease:channel!=='stable',
  patch_stride:STRIDE,ordinal_within_patch:ordinal,
  limits:{major:MAX_COMPONENT,minor:MAX_COMPONENT,patch:MAX_COMPONENT,prerelease_sequence:MAX_PRERELEASE},
  automatic_update_authorized:false,installation_requested:false,release_accepted:false};
}

export function verifyVersionPair(source,msiVersion){
 validateMsiVersion(msiVersion);const policy=sourceVersionPolicy(source);
 requireValue(msiVersion===policy.product_version,'msi_version_does_not_match_source');
 return policy;
}

if(process.argv[1]&&import.meta.url===pathToFileURL(resolve(process.argv[1])).href){
 try{
  const {values}=parseArgs({options:{'source-version':{type:'string'},'msi-version':{type:'string'},help:{type:'boolean'}}});
  if(values.help)console.log('Usage: node tools/msi-version-policy.mjs --source-version SEMVER [--msi-version NUMERIC_VERSION]\nRead-only packed-patch-v1 mapping/verification; no package, installation, update or release authorization.');
  else console.log(JSON.stringify(values['msi-version']===undefined?sourceVersionPolicy(values['source-version']):verifyVersionPair(values['source-version'],values['msi-version']),null,2));
 }catch(error){console.error(/^[a-z][a-z0-9_]{1,96}$/.test(error.message)?error.message:'msi_version_policy_failed');process.exitCode=1;}
}
