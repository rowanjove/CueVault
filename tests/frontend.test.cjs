const test = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const vm = require('node:vm');
const path = require('node:path');
const html = fs.readFileSync(path.join(__dirname, '../ui/index.html'), 'utf8');
const script = html.split('<script>')[1].split('</script>')[0];
function section(start, end) {
  const a = script.indexOf(start), b = script.indexOf(end, a);
  assert.ok(a >= 0 && b > a, `Missing source section: ${start}`);
  return script.slice(a, b);
}
const prompt = () => ({id:'p', title:'标题', categoryId:'old', variants:[{id:'v', name:'默认', content:'正文'}], createdAt:1, updatedAt:2, useCount:0});
const library = () => ({categories:[{id:'all',name:'全部',system:true},{id:'old',name:'旧分类'}],prompts:[prompt()]});
function context() {
  const store = new Map(), nodes = new Map();
  const ctx = {
    crypto:require('node:crypto').webcrypto, structuredClone, console,
    STORAGE_KEY:'prompt_library_v7', seedData:library(), state:library(),
    store, nodes, toasts:[], showToast(text){ctx.toasts.push(text);},
    localStorage:{getItem:k=>store.has(k)?store.get(k):null,setItem:(k,v)=>store.set(k,v)},
    isDesktop:()=>true, persistEnabled:true, saveQueue:Promise.resolve(),
    isEditingCategories:false, categorySnapshot:null, detachedPromptCategories:new Map(),
    categoryEditDirty:false, currentCategory:'all', temporaryPrompt:null,
    document:{getElementById(id){if(!nodes.has(id)) nodes.set(id,{});return nodes.get(id);}},
    renderAll(){}, closeModal(){ctx.temporaryPrompt=null;ctx.currentPromptId=null;},
    askConfirm:async()=>true,
    els:{editCategoryBtn:{classList:{remove(){}},title:''},titleInput:{value:'标题'},categorySelect:{value:'old'},promptEditor:{value:'正文'},saveBtn:{}},
    invoke:async(cmd,args)=>{ctx.saved=structuredClone(args.state);}
  };
  vm.createContext(ctx);
  vm.runInContext(section('function makeId()', 'function debounce(')
    +section('function readStoredLibrary()', 'function libraryFromRemote(')
    +section('function stateForSave(', 'function getCategory('),ctx);
  return ctx;
}
test('HTML copies match and JavaScript parses',()=>{
  assert.equal(html,fs.readFileSync(path.join(__dirname,'../提示词管理.html'),'utf8'));
  new vm.Script(script);
});
test('pagination returns only the current page',()=>{
  const ctx={};
  vm.createContext(ctx);
  vm.runInContext(section('function paginateItems(', 'function renderPager(')+'\nthis.paginateItems=paginateItems;', ctx);
  const items=Array.from({length:60}, (_,i)=>i);
  const page=ctx.paginateItems(items, 2, 25);
  assert.deepEqual(page.items, items.slice(25, 50));
  assert.equal(page.pages, 3);
  assert.equal(page.page, 2);
  const clamped=ctx.paginateItems(items, 9, 25);
  assert.equal(clamped.page, 3);
  assert.equal(clamped.items.length, 10);
  const exact=ctx.paginateItems(items.slice(0, 25), 3, 25);
  assert.equal(exact.pages, 1);
  assert.equal(exact.page, 1);
  assert.equal(exact.items.length, 25);
  assert.equal(ctx.paginateItems([], 4, 25).items.length, 0);
  assert.match(script, /view\.items\.forEach/);
  assert.doesNotMatch(script, /prompt-card[^\{]*display:\s*none/);
});
test('preset library meets counts, validates structure and has no private prompts',()=>{
  const start=script.indexOf('const seedData = ');
  const end=script.indexOf('\nlet state = ', start);
  const ctx={};
  vm.createContext(ctx);
  vm.runInContext(script.slice(start, end)+'\nthis.seedData=seedData;', ctx);
  const data=ctx.seedData;
  const exported=JSON.parse(fs.readFileSync(path.join(__dirname,'../library.json'),'utf8'));
  assert.equal(JSON.stringify(data),JSON.stringify(exported));
  const categories=data.categories.filter(c=>c.id!=='all');
  assert.equal(categories.length,13);
  assert.equal(new Set(categories.map(c=>c.name)).size,13);
  assert.equal(data.prompts.length,52);
  assert.equal(new Set(data.prompts.map(p=>p.title)).size,52);
  assert.equal(new Set(data.prompts.map(p=>p.variants[0].content)).size,52);
  for(const cat of categories){
    const count=data.prompts.filter(p=>p.categoryId===cat.id).length;
    assert.equal(count,4,cat.name);
  }
  for(const p of data.prompts){
    assert.ok(categories.some(c=>c.id===p.categoryId));
    assert.ok(p.tags.length>=3&&p.tags.length<=5,p.title);
    assert.equal(new Set(p.tags).size,p.tags.length);
    assert.ok(p.tags.every(t=>typeof t==='string'&&t.length<=6));
    assert.ok(p.variants[0].content.length>=60&&p.variants[0].content.length<=240,p.title);
    assert.match(p.variants[0].content,/\[.+\]/);
  }
  assert.equal(data.categories.some(c=>c.id==='common'||c.id==='routing'), false);
  assert.equal(data.prompts.some(p=>p.title==='常用'||p.title==='模型路由'), false);
});

test('tags survive normalization and import, including legacy prompts without tags',()=>{
  const c=context();
  const p={...prompt(),tags:[' 调试 ','调试','复现','修复']};
  assert.equal(c.normalizePrompt(p).tags.join(','),'调试,复现,修复');
  assert.equal(c.normalizePrompt(prompt()).tags.length,0);
  const data=c.normalizeImport({categories:library().categories,prompts:[p]});
  assert.equal(data.prompts[0].tags.join(','),'调试,复现,修复');
});

test('20-item pagination renders filtered page data and keeps controls on page change',()=>{
  function node(){return {children:[],hidden:false,classList:{},setAttribute(){},replaceChildren(){this.children=[];},appendChild(n){this.children.push(n);},querySelector(){return {};},set innerHTML(v){this.children=[];this.html=v;},get innerHTML(){return this.html;}};}
  const c={state:{categories:[{id:'a',name:'甲'},{id:'b',name:'乙'}],prompts:Array.from({length:58},(_,i)=>({id:String(i),title:'条目'+i,categoryId:i<30?'a':'b',tags:[i<25?'检索':'其他','用途','场景'],variants:[{content:'正文'}],favorite:i<23,updatedAt:100-i}))},
    currentCategory:'all',favoriteOnly:false,sortMode:'updatedAt',currentPage:1,listQueryKey:'',PAGE_SIZE:20,
    els:{searchInput:{value:''},promptGrid:node(),promptPager:node(),pageTitle:node(),pageSubtitle:node()},
    document:{createElement:node},window:{scrollTo(){}},escapeHtml:s=>s,getAllContent:p=>p.variants[0].content,getPrimaryContent:p=>p.variants[0].content,COPY_ICON:'',STAR_ICON:''};
  c.getCategory=id=>c.state.categories.find(cat=>cat.id===id);
  vm.createContext(c);vm.runInContext(section('function visibleQueryKey()', 'function renderAll()'),c);
  c.renderPrompts();assert.equal(c.els.promptGrid.children.length,20);assert.equal(c.els.promptPager.hidden,false);
  c.els.promptPager.children.find(n=>n.textContent==='下一页').onclick();
  assert.equal(c.currentPage,2);assert.equal(c.els.promptGrid.children.length,20);
  c.currentPage=3;c.renderPrompts();assert.equal(c.els.promptGrid.children.length,18);
  c.els.searchInput.value='检索';c.renderPrompts();assert.equal(c.currentPage,1);assert.equal(c.els.promptGrid.children.length,20);
  c.els.promptPager.children.find(n=>n.textContent==='下一页').onclick();
  assert.equal(c.els.searchInput.value,'检索');assert.equal(c.currentPage,2);assert.equal(c.els.promptGrid.children.length,5);
  c.favoriteOnly=true;c.renderPrompts();assert.equal(c.currentPage,1);
  c.els.promptPager.children.find(n=>n.textContent==='下一页').onclick();
  assert.equal(c.favoriteOnly,true);assert.equal(c.els.promptGrid.children.length,3);
  c.currentCategory='b';c.renderPrompts();assert.equal(c.els.promptGrid.children.length,0);assert.equal(c.els.promptPager.hidden,true);
  c.favoriteOnly=false;c.els.searchInput.value='';c.currentCategory='a';c.renderPrompts();
  c.els.promptPager.children.find(n=>n.textContent==='下一页').onclick();
  assert.equal(c.currentCategory,'a');assert.equal(c.sortMode,'updatedAt');assert.equal(c.els.promptGrid.children.length,10);
  c.state.prompts=c.state.prompts.slice(0,20);c.renderPrompts();
  assert.equal(c.currentPage,1);assert.equal(c.els.promptPager.hidden,true);assert.equal(c.els.promptPager.children.length,0);
  assert.match(script,/const PAGE_SIZE = 20/);
});
test('corrupt current storage recovers old data without overwriting either key',()=>{
  const c=context();c.store.set(c.STORAGE_KEY,'{broken');c.store.set('prompt_library_v6',JSON.stringify(library()));
  const before=new Map(c.store), loaded=c.loadBrowserData();
  assert.equal(loaded.prompts[0].id,'p');assert.deepEqual(c.store,before);
});
test('unrecoverable corrupt storage fails closed',()=>{
  for(const raw of ['{broken','{}','{"categories":[],"prompts":[{"id":"p","variants":[null]}]}']){
    const c=context();c.store.set(c.STORAGE_KEY,raw);
    assert.throws(()=>c.loadBrowserData());assert.equal(c.store.get(c.STORAGE_KEY),raw);
  }
});
test('first launch seeds data and a valid empty library stays empty',()=>{
  const c=context();assert.equal(c.loadBrowserData().prompts.length,1);assert.ok(c.store.has(c.STORAGE_KEY));
  c.store.set(c.STORAGE_KEY,JSON.stringify({categories:[{id:'all',name:'全部'}],prompts:[]}));
  assert.equal(c.loadBrowserData().prompts.length,0);
});
function installImport(c){
  vm.runInContext(section('function finishCategoryEdit()', 'els.editCategoryBtn.onclick=')
    +section("document.getElementById('importFile').onchange=", "document.addEventListener('keydown'"),c);
  c.isEditingCategories=true;c.categoryEditDirty=true;c.categorySnapshot=structuredClone(c.state.categories);
  c.currentCategory='old';
}
const imported={categories:[{id:'all',name:'全部'},{id:'new',name:'新分类'}],prompts:[{...prompt(),categoryId:'new'}]};
test('import during category edit persists new categories and ends editing',async()=>{
  const c=context();installImport(c);
  const target={files:[{text:async()=>JSON.stringify(imported)}],value:'file'};
  await c.nodes.get('importFile').onchange({target});
  assert.equal(c.saved.categories[1].id,'new');assert.equal(c.saved.prompts[0].categoryId,'new');
  assert.equal(c.isEditingCategories,false);assert.equal(c.categorySnapshot,null);assert.equal(target.value,'');
});
test('failed import restores data, view and active category draft',async()=>{
  const c=context();installImport(c);c.state.categories[1].name='未保存改名';
  const before=JSON.stringify(c.state);c.invoke=async()=>{throw Error('disk full');};
  await c.nodes.get('importFile').onchange({target:{files:[{text:async()=>JSON.stringify(imported)}],value:'file'}});
  assert.equal(JSON.stringify(c.state),before);assert.equal(c.currentCategory,'old');assert.equal(c.isEditingCategories,true);
});
function installClose(c){
  let handler;
  c.destroyed=0;
  c.window={__TAURI__:{window:{getCurrentWindow:()=>({onCloseRequested:async h=>{handler=h;return ()=>{};},destroy:async()=>{c.destroyed++;}})}}};
  vm.runInContext(section('async function bindCloseFlush()', 'async function boot()'),c);
  return async()=>{await c.bindCloseFlush();return handler;};
}
test('close keeps window after save failure and allows retry',async()=>{
  const c=context();const open=await installClose(c);const handler=await open();
  c.isEditingCategories=true;c.categoryEditDirty=true;c.saveData=async()=>{throw Error('disk full');};
  await handler({preventDefault(){}});assert.equal(c.destroyed,0);
  c.saveData=async()=>{};await handler({preventDefault(){}});assert.equal(c.destroyed,1);
});
test('close flushes a normal save and stays open when that save fails',async()=>{
  const c=context();const open=await installClose(c);const handler=await open();
  let calls=0;
  c.saveData=async()=>{calls++;throw Error('disk full');};
  await handler({preventDefault(){}});
  assert.equal(calls,1);assert.equal(c.destroyed,0);
  c.saveData=async()=>{calls++;};
  await handler({preventDefault(){}});
  assert.equal(calls,2);assert.equal(c.destroyed,1);
});
test('close after a failed read does not write the library',async()=>{
  const c=context();const open=await installClose(c);const handler=await open();
  let calls=0;c.persistEnabled=false;c.saveData=async()=>{calls++;};
  await handler({preventDefault(){}});
  assert.equal(calls,0);assert.equal(c.destroyed,1);
});
function installEditor(c){
  c.temporaryPrompt=structuredClone(c.state.prompts[0]);c.currentPromptId='p';c.editMode=true;c.activeVariantId='v';c.syncModal=()=>{};
  c.commitEditor=()=>{c.temporaryPrompt.variants.find(v=>v.id===c.activeVariantId).content=c.els.promptEditor.value;};
  vm.runInContext(section('function comparablePrompt(', 'function restorePrompt()'),c);
}
test('typing while save is pending keeps subsequent draft and blocks duplicate saves',async()=>{
  const c=context();installEditor(c);let resolve,calls=0;
  c.saveData=()=>{calls++;return new Promise(r=>{resolve=r;});};
  const saving=c.savePrompt();c.els.promptEditor.value='保存中继续输入';await c.savePrompt();resolve();await saving;
  assert.equal(calls,1);assert.equal(c.temporaryPrompt.variants[0].content,'保存中继续输入');assert.equal(c.editMode,true);
  assert.equal(c.state.prompts[0].variants[0].content,'正文');assert.equal(c.els.saveBtn.hidden,false);
  c.saveData=async()=>{};await c.savePrompt();assert.equal(c.state.prompts[0].variants[0].content,'保存中继续输入');assert.equal(c.editMode,false);
});
test('closing detail while save is pending does not reopen it or crash',async()=>{
  const c=context();installEditor(c);let resolve;c.saveData=()=>new Promise(r=>{resolve=r;});
  const saving=c.savePrompt();c.closeModal();resolve();await saving;assert.equal(c.temporaryPrompt,null);
});
test('failed prompt save restores saved state and retains draft for retry',async()=>{
  const c=context();installEditor(c);c.els.promptEditor.value='未保存正文';c.saveData=async()=>{throw Error('disk full');};
  await c.savePrompt();assert.equal(c.state.prompts[0].variants[0].content,'正文');assert.equal(c.temporaryPrompt.variants[0].content,'未保存正文');
  c.saveData=async()=>{};await c.savePrompt();assert.equal(c.state.prompts[0].variants[0].content,'未保存正文');
});

test('new prompt preserves edits made while first save is pending',async()=>{
  const c=context();installEditor(c);c.state.prompts=[];c.currentPromptId='__new__';
  c.newPromptBaseline={title:'标题',categoryId:'old',variantId:'v'};
  let resolve;c.saveData=()=>new Promise(r=>{resolve=r;});
  const saving=c.savePrompt();c.els.titleInput.value='后续标题';resolve();await saving;
  assert.equal(c.state.prompts.length,1);assert.equal(c.currentPromptId,'p');
  assert.equal(c.temporaryPrompt.title,'后续标题');assert.equal(c.editMode,true);
  c.saveData=async()=>{};await c.savePrompt();assert.equal(c.state.prompts.length,1);assert.equal(c.state.prompts[0].title,'后续标题');
});
test('failed first save leaves a new draft that can be retried',async()=>{
  const c=context();installEditor(c);c.state.prompts=[];c.currentPromptId='__new__';
  c.newPromptBaseline={title:'标题',categoryId:'old',variantId:'v'};
  c.saveData=async()=>{throw Error('disk full');};await c.savePrompt();
  assert.equal(c.state.prompts.length,0);assert.equal(c.currentPromptId,'__new__');assert.ok(c.temporaryPrompt);
  c.saveData=async()=>{};await c.savePrompt();assert.equal(c.state.prompts.length,1);assert.equal(c.editMode,false);
});

function installTemporaryEditing(c){
  installEditor(c);
  const classList={add(){},remove(){},toggle(){}};
  c.setTimeout=()=>0;c.document.activeElement=null;
  c.document.querySelector=()=>({classList});
  c.els.editModeBtn={classList};c.els.deleteBtn={};c.els.detailOverlay={classList};
  c.els.promptEditor.setAttribute=()=>{};
  c.renderCategoryOptions=()=>{};c.renderVariantTabs=()=>{};
  vm.runInContext(section('function getActiveVariant()', 'function renderVariantTabs()')
    +section('function switchVariant(', 'function addVariant()')
    +section('function openPrompt(', 'function comparablePrompt(')
    +section('function restorePrompt()', "document.getElementById('newPromptBtn')"),c);
  c.openPrompt('p');
}
test('default editor is temporary: copy uses draft and persisted content stays original',async()=>{
  const c=context();installTemporaryEditing(c);let copied;c.copyText=async text=>{copied=text;};
  assert.equal(c.editMode,false);assert.equal(c.els.promptEditor.readOnly,false);
  assert.equal(c.els.titleInput.readOnly,true);assert.equal(c.els.categorySelect.disabled,true);
  c.els.promptEditor.value='本次使用的临时正文';c.updateDirtyState();
  assert.equal(c.els.saveBtn.hidden,true);await c.savePrompt();assert.equal(c.saved,undefined);
  await c.copyCurrent();assert.equal(copied,'本次使用的临时正文');
  assert.equal(c.saved.prompts[0].variants[0].content,'正文');assert.equal(c.saved.prompts[0].useCount,1);
  let confirmations=0;c.askConfirm=async()=>{confirmations++;return true;};await c.safeCloseModal();
  assert.equal(confirmations,0);c.openPrompt('p');assert.equal(c.els.promptEditor.value,'正文');
});
test('temporary text survives scene switches without updating source',()=>{
  const c=context();c.state.prompts[0].variants.push({id:'v2',name:'其他',content:'其他正文'});
  installTemporaryEditing(c);c.els.promptEditor.value='临时第一场景';c.switchVariant('v2');
  assert.equal(c.els.promptEditor.value,'其他正文');c.els.promptEditor.value='临时第二场景';c.switchVariant('v');
  assert.equal(c.els.promptEditor.value,'临时第一场景');assert.equal(c.editMode,false);
  assert.equal(c.state.prompts[0].variants[0].content,'正文');assert.equal(c.state.prompts[0].variants[1].content,'其他正文');
});
test('explicit edit and save persist temporary text; cancelling returns to temporary mode',async()=>{
  const c=context();installTemporaryEditing(c);c.els.promptEditor.value='准备保存的修改';
  c.enterEditMode();assert.equal(c.editMode,true);assert.equal(c.els.saveBtn.hidden,false);
  await c.savePrompt();assert.equal(c.saved.prompts[0].variants[0].content,'准备保存的修改');
  assert.equal(c.editMode,false);assert.equal(c.els.promptEditor.readOnly,false);
  c.enterEditMode();c.els.promptEditor.value='放弃的修改';c.exitEditMode();
  assert.equal(c.editMode,false);assert.equal(c.els.promptEditor.readOnly,false);assert.equal(c.els.promptEditor.value,'准备保存的修改');
});
test('double clicking body cannot implicitly enter persistent editing',()=>{
  assert.ok(!script.includes("els.promptEditor.addEventListener('dblclick'"));
  assert.ok(!script.includes('if(!editMode) enterEditMode();'));
});
