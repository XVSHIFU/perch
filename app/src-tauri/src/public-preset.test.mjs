import test from 'node:test';
import assert from 'node:assert/strict';
import {mkdtemp,readFile,writeFile,rm} from 'node:fs/promises';
import {join} from 'node:path';
import {tmpdir} from 'node:os';
import {mergeThinkingPreset,applyDshThinkingPreset} from './public-preset.mjs';
import {preparePi} from './pi-config.mjs';
import {createRequire} from 'node:module';

test('fresh DSH YAML without a model section accepts an absent preset',()=>{
 const require=createRequire(process.env.PERCH_DSH_TEST_INSTALL??new URL('../../verification/p1/repro/dsh/package.json',import.meta.url));
 const {parseDocument}=require('yaml');
 for(const source of ['', 'other:\n  keep: true\n']){
  const doc=parseDocument(source);
  applyDshThinkingPreset(doc,undefined);
  doc.setIn(['agent-default-model','provider'],'fixture');
  doc.setIn(['agent-default-model','model'],'fixture');
  assert.equal(doc.getIn(['agent-default-model','provider']),'fixture');
  if(source)assert.equal(doc.getIn(['other','keep']),true);
  assert.equal(doc.hasIn(['agent-default-model','reasoningEffort']),false);
 }
});

test('public preset ownership preserves manual settings and restores previous values',()=>{
 const conflict=fn=>assert.throws(fn,{code:'PERCH_PRESET_CONFLICT'});
 assert.deepEqual(mergeThinkingPreset('medium',undefined,undefined),{value:'medium'});
 conflict(()=>mergeThinkingPreset('medium',undefined,'high'));
 const adopted=mergeThinkingPreset('low',undefined,'low');
 const changed=mergeThinkingPreset('low',adopted.owned,'high');
 assert.deepEqual(mergeThinkingPreset('high',changed.owned,undefined),{value:'low'});
 conflict(()=>mergeThinkingPreset('max',changed.owned,'high'));
 assert.deepEqual(mergeThinkingPreset('max',changed.owned,undefined),{value:'max'});
 const added=mergeThinkingPreset(undefined,undefined,'high');
 assert.deepEqual(mergeThinkingPreset('high',added.owned,undefined),{value:undefined});
 conflict(()=>mergeThinkingPreset(undefined,added.owned,'high'));
 conflict(()=>mergeThinkingPreset(undefined,undefined,'arbitrary secret'));
});

test('Pi saves public preset without changing unrelated settings; conflict writes nothing',async()=>{
 const home=await mkdtemp(join(tmpdir(),'perch-preset-'));
 try{
  const settingsPath=join(home,'settings.json');
  await writeFile(settingsPath,JSON.stringify({theme:'custom',defaultThinkingLevel:'low'}));
  const connection={protocol:'openai-chat',models:[],defaultModel:'fixture',name:'fixture',baseUrl:'http://127.0.0.1:1'};
  await preparePi(home,connection,[],[],[],'low');
  await preparePi(home,connection,[],[],[],'high');
  const settings=JSON.parse(await readFile(settingsPath,'utf8'));
  assert.equal(settings.defaultThinkingLevel,'high');assert.equal(settings.theme,'custom');
  settings.defaultThinkingLevel='max';await writeFile(settingsPath,JSON.stringify(settings));
  const before=await readFile(settingsPath,'utf8'),models=await readFile(join(home,'models.json'),'utf8');
  await assert.rejects(preparePi(home,connection,[],[],[],'high'),{code:'PERCH_PRESET_CONFLICT'});
  assert.equal(await readFile(settingsPath,'utf8'),before);assert.equal(await readFile(join(home,'models.json'),'utf8'),models);
  await preparePi(home,connection);
  const after=JSON.parse(await readFile(settingsPath,'utf8'));
  assert.equal(after.defaultThinkingLevel,'max');assert.equal(after.perchThinkingPreset,undefined);
 }finally{await rm(home,{recursive:true,force:true});}
});

test('DSH YAML keeps model selection and unrelated sections; supports restoration',()=>{
 const require=createRequire(new URL('../../verification/p1/repro/dsh/package.json',import.meta.url));
 const {parseDocument}=require('yaml');
 const doc=parseDocument('agent-default-model:\n  provider: fixture\n  model: fixture\nother:\n  keep: true\n');
 applyDshThinkingPreset(doc,'high');
 const persisted=parseDocument(String(doc));
 assert.equal(persisted.getIn(['agent-default-model','reasoningEffort']),'high');
 assert.equal(persisted.getIn(['agent-default-model','provider']),'fixture');
 assert.equal(persisted.getIn(['other','keep']),true);
 applyDshThinkingPreset(persisted,undefined);
 assert.equal(persisted.hasIn(['agent-default-model','reasoningEffort']),false);
 assert.equal(persisted.hasIn(['perch-thinking-preset']),false);
});
