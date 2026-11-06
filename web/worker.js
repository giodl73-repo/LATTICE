import init,{evaluate_json} from './pkg/lattice_web.js';
let generation;
self.onmessage=async({data})=>{
 if(data.type==='init'){
  generation=data.generation;
  try{await init();self.postMessage({type:'ready',generation});}
  catch{self.postMessage({type:'error',generation,message:'Context engine could not load.'});}
  return;
 }
 if(data.type!=='evaluate'||data.generation!==generation)return;
 try{
  const started=performance.now();const raw=evaluate_json(data.left,data.right,data.budget,data.operation);
  self.postMessage({type:'result',id:data.id,generation,raw,ms:performance.now()-started});
 }catch(error){self.postMessage({type:'error',id:data.id,generation,message:String(error),fatal:typeof error!=='string'});}
};
