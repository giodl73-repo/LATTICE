import {test,expect} from '@playwright/test';
import fs from 'node:fs/promises';
import {createReadStream} from 'node:fs';
import {createGunzip} from 'node:zlib';
import {createInterface} from 'node:readline';
const cases=[[3,6,8,'join'],[3,6,8,'meet'],[31,31,3,'join'],[31,31,3,'meet'],[31,0,4,'join'],[0,31,4,'join'],[31,31,8,'join'],[0,0,8,'meet'],[21,10,5,'join'],[15,30,6,'meet'],[1,16,4,'join'],[2,6,8,'join']].map(([left,right,budget,operation])=>({left,right,budget,operation}));
const index=c=>((c.left*32+c.right)*8+c.budget-1)*2+(c.operation==='meet'?1:0);
const wanted=new Set(cases.map(index));const rawFrames=new Map();let line=0;
const original=new URL('../../context/waves/2026-11-06-context-algebra-self-service/validation/original-frame-parity-v1-native.stdout.gz',import.meta.url);
for await(const value of createInterface({input:createReadStream(original).pipe(createGunzip()),crlfDelay:Infinity})){if(wanted.has(line))rawFrames.set(line,JSON.parse(value).Ok);line++;}
if(line!==16398||rawFrames.size!==cases.length)throw Error('Incomplete frozen original result corpus');
const rawFor=c=>rawFrames.get(index(c));
const url=c=>'/LATTICE/?'+new URLSearchParams(Object.entries(c).map(([k,v])=>[k,String(v)]));
async function budget(page,n){await page.locator('#budget').evaluate((e,n)=>{e.value=String(n);e.dispatchEvent(new Event('input'));},n);}
async function blank(page){for(const id of ['grains','bonds','deferred'])await expect(page.locator('#'+id)).toHaveText('—');for(const id of ['inputs','receipt'])await expect(page.locator('#'+id)).toHaveText('');await expect(page.locator('#retained .grain')).toHaveCount(0);await expect(page.locator('#frontier li')).toHaveCount(0);for(const id of ['export','raw-export'])await expect(page.locator('#'+id)).toBeDisabled();}
async function observe(page){await page.addInitScript(()=>{window.__workers=[];const RealWorker=window.Worker;window.Worker=class extends RealWorker{constructor(...args){super(...args);this.sent=[];this.received=[];this.retired=false;window.__workers.push(this);this.addEventListener('message',e=>this.received.push(structuredClone(e.data)));}postMessage(data,...args){this.sent.push(structuredClone(data));return super.postMessage(data,...args);}terminate(){this.retired=true;return super.terminate();}};});}
async function bounded(page){expect(await page.evaluate(()=>window.__workers.filter(w=>!w.retired).length)).toBeLessThanOrEqual(1);}
async function stall(page,stage){await page.route('**/worker.js',async route=>{const response=await route.fetch(),body=await response.text();const condition=stage==='init'?"event.data.type==='init'":"event.data.type==='evaluate'&&event.data.budget===4";await route.fulfill({response,body:body+`\nconst original=self.onmessage;self.onmessage=event=>{if(${condition}){self.postMessage({type:'test-stalled',generation:event.data.generation,id:event.data.id});${stage==='cpu'?'while(true){}':'return new Promise(()=>{});'}}return original(event);};\n`});});}
async function blocked(page){await expect.poll(()=>page.evaluate(()=>window.__workers.some(w=>w.received.some(v=>v.type==='test-stalled')))).toBe(true);}
async function late(page,index=0){await page.evaluate(({index,raw})=>{const w=window.__workers[index],init=w.sent.find(v=>v.type==='init'),request=w.sent.findLast(v=>v.type==='evaluate');w.onmessage({data:{type:'ready',generation:init.generation}});w.onmessage({data:{type:'result',generation:init.generation,id:request?.id,raw,ms:0}});w.onmessage({data:{type:'error',generation:init.generation,id:request?.id,message:'RETIRED ERROR',fatal:true}});},{index,raw:rawFor(cases[0])});}
async function download(page,id){const pending=page.waitForEvent('download');await page.locator('#'+id).click();const file=await pending;return {name:file.suggestedFilename(),bytes:await fs.readFile(await file.path())};}

test('a pending changed context clears the previous metrics and receipts',async({page})=>{
 await page.route('**/worker.js',async route=>{const response=await route.fetch();await route.fulfill({response,body:await response.text()+`\nconst original=self.onmessage;self.onmessage=event=>{if(event.data.budget===4)return new Promise(()=>{});return original(event);};\n`});});
 await page.goto('/LATTICE/');await expect(page.locator('#grains')).toHaveText('6');await expect(page.locator('#receipt')).toContainText('receipt_hash');await budget(page,4);await blank(page);
});

test('real raw and original wrapped exports match frozen native cuts and keyboard selection',async({browser,page,baseURL})=>{
 for(const c of cases){const context=await browser.newContext({baseURL,acceptDownloads:true});try{const p=await context.newPage();await p.goto(url(c));await expect(p.locator('#export')).toBeEnabled();const raw=await download(p,'raw-export'),wrapped=await download(p,'export');expect(raw.name).toBe('lattice-synthetic-algebra.raw.json');expect(raw.bytes.equals(Buffer.from(rawFor(c)))).toBe(true);expect(wrapped.name).toBe('lattice-synthetic-algebra.json');expect(wrapped.bytes.equals(Buffer.from(JSON.stringify({inputs:c,result:JSON.parse(rawFor(c))},null,2)))).toBe(true);}finally{await context.close();}}
 await page.goto('/LATTICE/');await expect(page.locator('#export')).toBeEnabled();await page.locator('#left-0').focus();await page.keyboard.press('Space');await expect(page.locator('#left-0')).not.toBeChecked();await expect(page.locator('#export')).toBeEnabled();expect((await download(page,'raw-export')).bytes.equals(Buffer.from(rawFor(cases.at(-1))))).toBe(true);
 await page.locator('#budget').focus();await page.keyboard.press('Home');await expect(page.locator('#verdict')).toHaveText('Cannot close context');await blank(page);await page.keyboard.press('End');await expect(page.locator('#export')).toBeEnabled();
});

test('Stop retires a CPU-blocked worker and Retry preserves the selection',async({page})=>{
 await observe(page);await stall(page,'cpu');await page.goto('/LATTICE/?left=31&right=0&budget=8&operation=meet');await expect(page.locator('#export')).toBeEnabled();await budget(page,4);await blocked(page);await page.locator('#stop-engine').click();await blank(page);await expect(page.locator('#status')).toContainText('engine stopped');expect(await page.evaluate(()=>window.__workers[0].retired)).toBe(true);await late(page);await expect(page.locator('#status')).toContainText('engine stopped');await page.unroute('**/worker.js');await page.locator('#retry').click();await expect(page.locator('#export')).toBeEnabled();await expect(page.locator('#budget')).toHaveValue('4');await expect(page.locator('#operation')).toHaveValue('meet');for(let i=0;i<5;i++){await expect(page.locator('#left-'+i)).toBeChecked();await expect(page.locator('#right-'+i)).not.toBeChecked();}await bounded(page);
});

test('initialization deadline retires the engine and ignores late readiness',async({page})=>{
 await observe(page);await page.clock.install();await stall(page,'init');await page.goto('/LATTICE/');await blocked(page);await page.clock.fastForward(15001);await expect(page.locator('#status')).toContainText('loading timed out');await blank(page);expect(await page.evaluate(()=>window.__workers[0].retired)).toBe(true);await late(page);await expect(page.locator('#status')).toContainText('loading timed out');await page.unroute('**/worker.js');await page.locator('#retry').click();await expect(page.locator('#grains')).toHaveText('6');await bounded(page);
});

test('calculation deadline runs on the main thread while a worker is CPU-blocked',async({page})=>{
 await observe(page);await page.clock.install();await stall(page,'cpu');await page.goto('/LATTICE/');await expect(page.locator('#export')).toBeEnabled();await budget(page,4);await blocked(page);await page.clock.fastForward(10001);await expect(page.locator('#status')).toContainText('timed out after 10 seconds');await blank(page);expect(await page.evaluate(()=>window.__workers[0].retired)).toBe(true);await page.unroute('**/worker.js');await page.locator('#retry').click();await expect(page.locator('#grains')).toHaveText('4');await late(page);await expect(page.locator('#grains')).toHaveText('4');await bounded(page);
});

test('failed WASM download retries the original URL selection without reload',async({page})=>{
 await observe(page);await page.route('**/*.wasm',r=>r.fulfill({status:503,body:'unavailable'}));await page.goto('/LATTICE/?left=31&right=0&budget=5&operation=meet');await expect(page.locator('#status')).toContainText('could not load');await blank(page);expect(await page.evaluate(()=>window.__workers[0].retired)).toBe(true);await page.unroute('**/*.wasm');await page.locator('#retry').click();await expect(page.locator('#grains')).toHaveText('3');await expect(page.locator('#budget')).toHaveValue('5');await expect(page.locator('#operation')).toHaveValue('meet');await bounded(page);
});

test('malformed current response retires the engine and cannot leave a receipt',async({page})=>{
 await observe(page);await stall(page,'promise');await page.goto('/LATTICE/');await expect(page.locator('#export')).toBeEnabled();await budget(page,4);await blocked(page);await page.evaluate(()=>{const w=window.__workers.at(-1),r=w.sent.findLast(v=>v.type==='evaluate');w.onmessage({data:{type:'result',generation:r.generation,id:r.id,raw:'{}',ms:0}});});await expect(page.locator('#status')).toContainText('could not be read');await blank(page);await page.unroute('**/worker.js');await page.locator('#retry').click();await expect(page.locator('#grains')).toHaveText('4');await bounded(page);
});

test('rapid selection changes retire old work and reject old generations',async({page})=>{
 await observe(page);await stall(page,'promise');await page.goto('/LATTICE/');await expect(page.locator('#export')).toBeEnabled();await budget(page,4);await blocked(page);await page.locator('#budget').evaluate(e=>{for(const n of [3,7,6,5]){e.value=String(n);e.dispatchEvent(new Event('input'));}});await expect(page.locator('#grains')).toHaveText('5');await late(page);await expect(page.locator('#grains')).toHaveText('5');await bounded(page);
});

test('late clipboard completion cannot replace a newer result or budget failure',async({page})=>{
 await page.addInitScript(()=>Object.defineProperty(navigator,'clipboard',{value:{writeText:()=>new Promise(resolve=>window.__releaseClipboard=resolve)},configurable:true}));await page.goto('/LATTICE/');await expect(page.locator('#export')).toBeEnabled();await page.locator('#share').click();await expect.poll(()=>page.evaluate(()=>typeof window.__releaseClipboard)).toBe('function');await budget(page,2);await expect(page.locator('#status')).toContainText('required closure grains exceed');await page.evaluate(()=>window.__releaseClipboard());await expect(page.locator('#status')).toContainText('required closure grains exceed');await blank(page);
});

test('an injected runtime trap retires the engine and Retry recovers current inputs',async({page})=>{
 await observe(page);await page.route('**/worker.js',async route=>{const response=await route.fetch(),body=(await response.text()).replace('const raw=evaluate_json(data.left,data.right,data.budget,data.operation);',"const raw=(()=>{if(data.budget===4)throw new WebAssembly.RuntimeError('injected trap before native call');return evaluate_json(data.left,data.right,data.budget,data.operation);})();");await route.fulfill({response,body});});await page.goto('/LATTICE/');await expect(page.locator('#export')).toBeEnabled();await budget(page,4);await expect(page.locator('#status')).toContainText('engine stopped');await blank(page);await page.unroute('**/worker.js');await page.locator('#retry').click();await expect(page.locator('#grains')).toHaveText('4');await bounded(page);
});

test('actual native budget errors keep the warm engine and recover without Retry',async({page})=>{
 await observe(page);await page.goto('/LATTICE/');await expect(page.locator('#export')).toBeEnabled();await budget(page,2);await expect(page.locator('#status')).toHaveText('required closure grains exceed grain budget');await blank(page);expect(await page.evaluate(()=>window.__workers[0].retired)).toBe(false);await expect(page.locator('#retry')).toBeHidden();await budget(page,8);await expect(page.locator('#grains')).toHaveText('6');expect(await page.evaluate(()=>window.__workers.length)).toBe(1);
});

test('worker crash and unreadable transport retire without automatically restarting',async({page})=>{
 await observe(page);await page.clock.install();await page.goto('/LATTICE/');await expect(page.locator('#export')).toBeEnabled();await page.evaluate(()=>window.__workers[0].onerror({preventDefault(){}}));await blank(page);await expect(page.locator('#retry')).toBeVisible();await page.clock.fastForward(5000);expect(await page.evaluate(()=>window.__workers.length)).toBe(1);await late(page);await expect(page.locator('#status')).toContainText('could not load or run');await page.locator('#retry').click();await expect(page.locator('#export')).toBeEnabled();await page.evaluate(()=>window.__workers.at(-1).onmessageerror());await blank(page);await expect(page.locator('#status')).toContainText('could not be read');await bounded(page);
});

test('Stop during initialization rejects late readiness before Retry',async({page})=>{
 await observe(page);await stall(page,'init');await page.goto('/LATTICE/');await blocked(page);await page.locator('#stop-engine').click();await blank(page);await late(page);await expect(page.locator('#status')).toContainText('engine stopped');await page.unroute('**/worker.js');await page.locator('#retry').click();await expect(page.locator('#grains')).toHaveText('6');await bounded(page);
});

test('Reset replaces pending work with the original sample and ignores its old result',async({page})=>{
 await observe(page);await stall(page,'promise');await page.goto('/LATTICE/?left=31&right=0&budget=8&operation=meet');await expect(page.locator('#export')).toBeEnabled();await budget(page,4);await blocked(page);await page.locator('#reset').click();await expect(page.locator('#grains')).toHaveText('6');await expect(page.locator('#budget')).toHaveValue('8');await expect(page.locator('#operation')).toHaveValue('join');await late(page);await expect(page.locator('#grains')).toHaveText('6');expect((await download(page,'raw-export')).bytes.equals(Buffer.from(rawFor(cases[0])))).toBe(true);await bounded(page);
});

test('runtime files belong to one exact source revision',async({page})=>{
 const urls=[];page.on('response',r=>urls.push(r.url()));await page.goto('/LATTICE/');await expect(page.locator('#export')).toBeEnabled();const manifest=await(await page.request.get('/LATTICE/BUILD-MANIFEST.json')).json();expect(manifest.runtime).toBe('assets/'+manifest.commit);const runtime=urls.filter(u=>/\/(app\.js|algebra-session\.js|worker\.js|style\.css|lattice_web\.js|lattice_web_bg\.wasm)$/.test(u));expect(new Set(runtime).size).toBe(6);for(const u of runtime)expect(new URL(u).pathname).toBe('/LATTICE/'+manifest.runtime+'/'+u.split('/').slice(u.includes('/pkg/')?-2:-1).join('/'));
});

test('complete source and original native examples remain usable without JavaScript',async({browser,baseURL})=>{
 const context=await browser.newContext({baseURL,javaScriptEnabled:false,acceptDownloads:true});try{
  const p=await context.newPage();await p.goto('/LATTICE/');await p.getByRole('link',{name:'Build guide & downloads'}).click();await expect(p.locator('h1')).toContainText('explainable');const manifest=await(await p.request.get('/LATTICE/BUILD-MANIFEST.json')).json();await expect(p.locator('body')).toContainText(manifest.commit);
  const examples=(await(await p.request.get('/LATTICE/native-examples.ndjson')).text()).trimEnd().split('\n').map(JSON.parse);expect(examples).toHaveLength(14);for(let i=0;i<cases.length;i++){expect(examples[i].inputs).toEqual(cases[i]);expect(examples[i].native).toEqual({Ok:rawFor(cases[i])});}expect(examples[12].native).toEqual({Err:'required closure grains exceed grain budget'});expect(examples[13].native).toEqual({Err:'Choose five sample items, a 1–8 grain budget, and meet or join'});
  const source=await(await p.request.get('/LATTICE/source.tar.xz')).body();const {createHash}=await import('node:crypto');expect(createHash('sha256').update(source).digest('hex')).toBe(manifest.source_archive_sha256);const pending=p.waitForEvent('download');await p.getByRole('link',{name:'Python receiver and licenses'}).click();const receiver=await pending;expect(receiver.suggestedFilename()).toBe('receiver.zip');expect((await fs.readFile(await receiver.path())).subarray(0,2).toString()).toBe('PK');expect(await p.evaluate(()=>document.documentElement.scrollWidth>innerWidth)).toBe(false);
 }finally{await context.close();}
});
