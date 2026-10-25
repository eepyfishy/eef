// Test state only. Connection acceptance is not authenticated registration.
export class OutputBackpressureTarget {
 #target;
 claim(session) {
  if(this.#target||!session||typeof session!=='object'||session.authenticated!==true||session.registered!==true||session.closed!==false)return false;
  this.#target=session;return true;
 }
 owns(session) {return this.#target!==undefined&&this.#target===session;}
}

const phases=new Set(['startup','initial_connection','saving_setting','retiring_output','after_reconnect','done']);
const connections=new Set(['starting','waiting','paused','checking','connecting','connected','disconnected','retrying','error','unknown']);
const channels=new Set(['ready','busy','unavailable','closed','starting','unknown']);
const count=value=>Number.isSafeInteger(value)&&value>=0?value:null;
const code=value=>Number.isInteger(value)&&value>=-2147483648&&value<=4294967295?value:null;
export function fixtureProcessStopped(child) {
 return !!child&&(child.fixtureSpawnFailed===true||(child.exitCode!==null&&child.exitCode!==undefined)||!!child.signalCode);
}
// Never include errors, process text, IDs, endpoints, paths, model names or peers'
// arbitrary fields in hosted diagnostics. Detailed evidence remains local/private.
export function outputFixtureDiagnostic({phase,children,sessions,target,runtimeObserved,status}={}) {
 const processes=Array.isArray(children)?children:[],links=Array.isArray(sessions)?sessions:[];
 return {
  schema_version:1,report_type:'node_output_fixture_diagnostic',phase:phases.has(phase)?phase:'unknown',
  runtime_observed:runtimeObserved===true,process_count:processes.length,session_count:links.length,
  processes:processes.slice(-8).map(child=>({role:child?.fixtureRole==='runtime'?'runtime':'cli',
   exited:child?.exitCode!==null&&child?.exitCode!==undefined,exit_code:code(child?.exitCode),spawn_failed:child?.fixtureSpawnFailed===true})),
  sessions:links.slice(0,16).map((session,index)=>{
   const frames={probe:0,auth:0,register:0,heartbeat:0,other:0};
   for(const type of Array.isArray(session?.messages)?session.messages:[])frames[['probe','auth','register','heartbeat'].includes(type)?type:'other']++;
   return {index,authenticated:session?.authenticated===true,registered:session?.registered===true,
    closed:session?.closed===true,blocked_target:target?.owns(session)===true,frames};
  }),
  connection_state:connections.has(status?.connection_state)?status.connection_state:'unknown',
  input_channel:channels.has(status?.input_channel)?status.input_channel:'unknown',
  successful_connections:count(status?.successful_connections),connection_attempts:count(status?.connection_attempts),
 };
}
