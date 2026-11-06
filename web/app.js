import {AlgebraSession} from './algebra-session.js';
const $=selector=>document.querySelector(selector);
const labels=['Task intent','Search design','Validation plan','Interface notes','Supporting evidence'];
let revision=0,completed=null,debounce=null;
for(const side of ['left','right'])for(const[i,text]of labels.entries()){
 const label=document.createElement('label'),input=document.createElement('input');
 input.type='checkbox';input.id=`${side}-${i}`;input.disabled=true;input.checked=side==='left'?i<2:i===1||i===2;
 input.onchange=calculate;label.append(input,document.createTextNode(text));$('#'+side).append(label);
}
const mask=side=>labels.reduce((value,_,i)=>value|($('#'+side+'-'+i).checked?1<<i:0),0);
const selection=()=>({left:mask('left'),right:mask('right'),budget:Number($('#budget').value),operation:$('#operation').value});
const sameSelection=(a,b)=>['left','right','budget','operation'].every(k=>a[k]===b[k]);
function controls(state){
 const enabled=['ready','computing'].includes(state);
 for(const input of document.querySelectorAll('.controls input,.controls select,.controls button'))input.disabled=!enabled;
 $('#budget-value').textContent=$('#budget').value;
 $('#stop-engine').disabled=['failed','stopped'].includes(state);$('#retry').hidden=!['failed','stopped'].includes(state);
}
function clearResult(){
 completed=null;for(const id of ['export','raw-export'])$('#'+id).disabled=true;
 for(const id of ['grains','bonds','deferred'])$('#'+id).textContent='—';
 for(const id of ['retained','frontier'])$('#'+id).replaceChildren();
 for(const id of ['inputs','receipt'])$('#'+id).textContent='';
 $('#none').hidden=true;$('#verdict').textContent='No current result';
}
function error(message){clearTimeout(debounce);revision++;clearResult();$('#verdict').textContent='Cannot close context';$('#status').textContent=message;}
const session=new AlgebraSession({
 onState(state){controls(state);if(state==='initializing')$('#status').textContent='Loading Rust context engine…';if(state==='computing')$('#status').textContent='Closing context…';},
 onError:error,
 onResult(result,raw,ms,input){
  if(input.revision!==revision||!sameSelection(input,selection()))return;
  const inputs=Object.freeze({left:input.left,right:input.right,budget:input.budget,operation:input.operation});
  completed=Object.freeze({inputs,result,raw,revision});
  const r=result.result,entries=[...result.left.frontier.map(v=>['A',v]),...result.right.frontier.map(v=>['B',v]),...r.frontier.map(v=>['Combined',v])];
  $('#grains').textContent=r.grain_count;$('#bonds').textContent=r.bond_count;$('#deferred').textContent=entries.length;
  $('#verdict').textContent=entries.length?'Closed with deferred work':'Closed within budget';
  $('#inputs').textContent=`Context A retains ${result.left.grain_count} grains; B retains ${result.right.grain_count}.`;
  $('#status').textContent=`Synthetic ${result.operation} · ${ms.toFixed(1)} ms · processing stays on your device`;
  $('#status').dataset.calculationMs=String(ms);
  for(const grain of r.grains){const card=document.createElement('div');card.className='grain';card.textContent=grain.label;const kind=document.createElement('small');kind.textContent=grain.kind.replaceAll('_',' ');card.append(kind);$('#retained').append(card);}
  for(const[stage,item]of entries){const li=document.createElement('li');li.textContent=`${stage}: ${item.note} (${item.item})`;$('#frontier').append(li);}
  $('#none').hidden=entries.length>0;$('#receipt').textContent=JSON.stringify({cut_hash:r.cut_hash,receipt_hash:r.receipt_hash,receipts:r.receipts},null,2);
  $('#export').disabled=false;$('#raw-export').disabled=false;controls(session.state);
 }
});
function invalidate(message){revision++;clearTimeout(debounce);session.invalidate();clearResult();controls(session.state);if(message)$('#status').textContent=message;return revision;}
function calculate(){const current=invalidate('Closing context…');debounce=setTimeout(()=>{if(current===revision)session.submit({...selection(),revision:current});},80);}
$('#budget').oninput=calculate;$('#operation').onchange=calculate;
$('#reset').onclick=()=>{
 $('#budget').value='8';$('#operation').value='join';for(let i=0;i<5;i++){$('#left-'+i).checked=i<2;$('#right-'+i).checked=i===1||i===2;}
 history.replaceState(null,'',location.pathname);calculate();
};
$('#stop-engine').onclick=()=>{invalidate();session.stop();$('#status').textContent='Context engine stopped. Your selection is kept. Choose Retry engine.';};
$('#retry').onclick=()=>{session.stop();calculate();};
$('#share').onclick=async()=>{
 const current=revision,inputs=selection(),url=new URL(location.href);url.search='';
 for(const key of ['left','right','budget','operation'])url.searchParams.set(key,String(inputs[key]));history.replaceState(null,'',url);
 try{await navigator.clipboard.writeText(url.href);}catch{}
 if(current===revision&&sameSelection(inputs,selection())&&['ready','computing'].includes(session.state))$('#status').textContent='Selection link is in the address bar.';
};
const currentResult=()=>completed&&completed.revision===revision&&sameSelection(completed.inputs,selection())?completed:null;
function save(raw,name){const url=URL.createObjectURL(new Blob([raw],{type:'application/json'}));const a=document.createElement('a');a.href=url;a.download=name;a.click();setTimeout(()=>URL.revokeObjectURL(url),1000);}
$('#export').onclick=()=>{const value=currentResult();if(value)save(JSON.stringify({inputs:value.inputs,result:value.result},null,2),'lattice-synthetic-algebra.json');};
$('#raw-export').onclick=()=>{const value=currentResult();if(value)save(value.raw,'lattice-synthetic-algebra.raw.json');};
const params=new URLSearchParams(location.search);
for(const side of ['left','right']){const n=Number(params.get(side));if(params.has(side)&&Number.isInteger(n)&&n>=0&&n<=31)for(let i=0;i<5;i++)$('#'+side+'-'+i).checked=Boolean(n&(1<<i));}
const budget=Number(params.get('budget'));if(params.has('budget')&&Number.isInteger(budget)&&budget>=1&&budget<=8)$('#budget').value=budget;
if(['meet','join'].includes(params.get('operation')))$('#operation').value=params.get('operation');
const initial=invalidate('Loading Rust context engine…');session.submit({...selection(),revision:initial});
