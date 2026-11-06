import fs from 'node:fs/promises';import path from 'node:path';import {pathToFileURL} from 'node:url';import {createHash} from 'node:crypto';
const [site,controlFile,outputFile,abiFile]=process.argv.slice(2);
if(!site||!controlFile||!outputFile||!abiFile)throw Error('Use SITE CONTROLS OUTPUT ABI');
let runtime=site;
try{const manifest=JSON.parse(await fs.readFile(path.join(site,'BUILD-MANIFEST.json'),'utf8'));if(!/^assets\/[a-f0-9]{40}$/.test(manifest.runtime))throw Error('Invalid runtime revision');runtime=path.join(site,manifest.runtime);}catch(error){if(error.code!=='ENOENT')throw error;}
const bytes=await fs.readFile(path.join(runtime,'pkg/lattice_web_bg.wasm'));const module=await import(pathToFileURL(path.resolve(runtime,'pkg/lattice_web.js')).href);await module.default({module_or_path:bytes});
const input=await fs.readFile(controlFile,'utf8');if(!input.endsWith('\n'))throw Error('Incomplete control stream');const controls=input.slice(0,-1).split('\n').map(JSON.parse);
function execute(c){try{return JSON.stringify({Ok:module.evaluate_json(c.left,c.right,c.budget,c.operation)});}catch(error){return JSON.stringify({Err:String(error)});}}
const output=await fs.open(outputFile,'wx');try{for(const c of controls)await output.write(execute(c)+'\n');}finally{await output.close();}
const abi=[{left:-1,right:0,budget:8,operation:'join'},{left:1.9,right:0,budget:8,operation:'join'},{left:4294967296,right:0,budget:8,operation:'join'},{left:0,right:NaN,budget:8,operation:'join'},{left:0,right:0,budget:Infinity,operation:'join'},{left:0,right:0,budget:4294967296,operation:'join'}].map(c=>({transport:Object.fromEntries(Object.entries(c).map(([k,v])=>[k,typeof v==='number'?String(v):v])),raw_result:execute(c)}));await fs.writeFile(abiFile,JSON.stringify(abi,null,2)+'\n',{flag:'wx'});
console.log(JSON.stringify({controls:controls.length,raw_abi_observations:abi.length,wasm_bytes:bytes.length,wasm_sha256:createHash('sha256').update(bytes).digest('hex')}));
