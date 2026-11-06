// Public WITNESS/TERRAIN lifecycle; original grants: notices/COMPONENTS.json.
export const INITIALIZATION_DEADLINE_MS=15_000;
export const EVALUATION_DEADLINE_MS=10_000;

export class AlgebraSession{
 constructor({onState,onResult,onError}){
  Object.assign(this,{onState,onResult,onError});
  this.generation=0;this.request=0;this.worker=null;this.pending=null;this.active=null;this.timer=null;this.state='stopped';
 }
 stateChanged(state){this.state=state;this.onState(state);}
 retire(){this.generation++;clearTimeout(this.timer);this.timer=null;this.worker?.terminate();this.worker=null;this.active=null;}
 fail(message){this.pending=null;this.retire();this.stateChanged('failed');this.onError(message);}
 start(){
  this.retire();const generation=this.generation;this.stateChanged('initializing');
  try{
   const worker=new Worker(new URL('./worker.js',import.meta.url),{type:'module'});this.worker=worker;
   const current=()=>this.worker===worker&&this.generation===generation;
   worker.onerror=event=>{if(current()){event.preventDefault();this.fail('Context engine could not load or run. Choose Retry engine, or use the source downloads.');}};
   worker.onmessageerror=()=>{if(current())this.fail('Context response could not be read. Choose Retry engine.');};
   worker.onmessage=({data})=>{
    if(!current()||!data||data.generation!==generation)return;
    if(data.type==='ready'&&this.state==='initializing'){
     clearTimeout(this.timer);this.timer=null;this.stateChanged('ready');if(this.pending)this.dispatch();return;
    }
    if(data.type==='error'&&this.state==='initializing'){
     this.fail('Context engine could not load. Choose Retry engine, or use the source downloads.');return;
    }
    if(!this.active||data.id!==this.active.id||this.state!=='computing')return;
    if(data.type==='error'&&data.fatal){this.fail('Context engine stopped. Choose Retry engine.');return;}
    if(data.type==='error'){
     clearTimeout(this.timer);this.timer=null;this.active=null;this.stateChanged('ready');this.onError(data.message);return;
    }
    if(data.type!=='result')return;
    let frame;
    try{
     if(typeof data.raw!=='string'||new TextEncoder().encode(data.raw).length>65536)throw Error('Unreadable frame');
     frame=JSON.parse(data.raw);
     if(!frame||Array.isArray(frame)||frame.schema!=='lattice.browser.v1'||frame.synthetic!==true||frame.budget!==this.active.input.budget||frame.operation!==this.active.input.operation)throw Error('Wrong result');
     if(!Number.isFinite(data.ms)||data.ms<0)throw Error('Unreadable timing');
     for(const key of ['left','right','result']){
      const cut=frame[key];
      if(!cut||!['grain_count','bond_count'].every(k=>Number.isSafeInteger(cut[k])&&cut[k]>=0)||!['cut_hash','receipt_hash'].every(k=>typeof cut[k]==='string'))throw Error('Unreadable cut');
      if(!Array.isArray(cut.grains)||!cut.grains.every(g=>g&&['id','label','kind'].every(k=>typeof g[k]==='string')))throw Error('Unreadable grains');
      if(!Array.isArray(cut.frontier)||!cut.frontier.every(v=>v&&['item','reason','note'].every(k=>typeof v[k]==='string')))throw Error('Unreadable frontier');
      if(!Array.isArray(cut.receipts)||!cut.receipts.every(v=>v&&['rule','note','hash'].every(k=>typeof v[k]==='string')))throw Error('Unreadable receipts');
     }
    }catch{this.fail('Context response could not be read. Choose Retry engine.');return;}
    clearTimeout(this.timer);this.timer=null;const input=this.active.input;this.active=null;this.stateChanged('ready');
    this.onResult(frame,data.raw,data.ms,input);
   };
   this.timer=setTimeout(()=>{if(current())this.fail('Context engine loading timed out after 15 seconds. Choose Retry engine.');},INITIALIZATION_DEADLINE_MS);
   worker.postMessage({type:'init',generation});
  }catch{this.fail('Context engine could not start. Choose Retry engine, or use the source downloads.');}
 }
 invalidate(){this.pending=null;this.request++;if(this.active){this.retire();this.stateChanged('stopped');}}
 submit(input){this.invalidate();this.pending=Object.freeze({...input});if(!this.worker)this.start();if(this.state==='ready')this.dispatch();}
 dispatch(){
  if(!this.pending||!this.worker||this.state!=='ready')return;
  const input=this.pending;this.pending=null;const id=++this.request,generation=this.generation;
  this.active={id,input};this.stateChanged('computing');
  this.timer=setTimeout(()=>{if(this.generation===generation&&this.active?.id===id)this.fail('Context calculation timed out after 10 seconds. Choose Retry engine.');},EVALUATION_DEADLINE_MS);
  try{this.worker.postMessage({type:'evaluate',id,generation,left:input.left,right:input.right,budget:input.budget,operation:input.operation});}
  catch{this.fail('Context engine could not accept the selected context. Choose Retry engine.');}
 }
 stop(){this.pending=null;this.request++;this.retire();this.stateChanged('stopped');}
}
