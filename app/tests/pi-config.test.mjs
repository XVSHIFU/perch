import {test} from 'node:test';
import assert from 'node:assert/strict';
import {mkdtemp,readFile,writeFile,rm} from 'node:fs/promises';
import {tmpdir} from 'node:os';
import {join} from 'node:path';
import {preparePi} from '../src-tauri/src/pi-config.mjs';

test('Pi connection updates preserve unrelated providers and settings without persisting a key',async()=>{
 const home=await mkdtemp(join(tmpdir(),'perch-pi-config-'));
 try{
  await writeFile(join(home,'models.json'),JSON.stringify({providers:{personal:{api:'openai-completions'}}}));
  await writeFile(join(home,'settings.json'),JSON.stringify({theme:'dark',extensions:['my-extension']}));
  const connection={protocol:'openai-chat',name:'Shared',baseUrl:'https://example.com/v1',defaultModel:'model-a',models:[{id:'model-a',name:'Model A',contextWindow:null,maxTokens:4096}]};
  await preparePi(home,connection);
  const models=JSON.parse(await readFile(join(home,'models.json'),'utf8'));
  assert.deepEqual(models.providers.personal,{api:'openai-completions'});
  assert.equal(models.providers.perch.apiKey,'${PERCH_MODEL_KEY}');
  assert.equal(models.providers.perch.models[0].contextWindow,undefined);
  await preparePi(home,{...connection,protocol:'anthropic'});
  assert.deepEqual(JSON.parse(await readFile(join(home,'settings.json'),'utf8')),{theme:'dark',extensions:['my-extension'],defaultProvider:'perch',defaultModel:'model-a'});
  const before=await readFile(join(home,'models.json'),'utf8');
  await writeFile(join(home,'settings.json'),'invalid JSON');
  await assert.rejects(preparePi(home,connection));
  assert.equal(await readFile(join(home,'models.json'),'utf8'),before);
 }finally{await rm(home,{recursive:true,force:true});}
});

test('managed packages preserve user entries, remove disabled packages and detect workbench edits',async()=>{
 const home=await mkdtemp(join(tmpdir(),'perch-pi-packages-'));
 const connection={protocol:'openai-chat',name:'Shared',baseUrl:'https://example.com/v1',defaultModel:'model-a',models:[]};
 try{
  await writeFile(join(home,'settings.json'),JSON.stringify({packages:['user-package']}));
  await preparePi(home,connection,[],[],['managed-package']);
  let settings=JSON.parse(await readFile(join(home,'settings.json'),'utf8'));
  assert.deepEqual(settings.packages,['user-package','managed-package']);
  await preparePi(home,connection,[],[],[]);
  settings=JSON.parse(await readFile(join(home,'settings.json'),'utf8'));assert.deepEqual(settings.packages,['user-package']);
  await preparePi(home,connection,[],[],['managed-package']);
  settings=JSON.parse(await readFile(join(home,'settings.json'),'utf8'));settings.packages=['user-package'];
  const edited=JSON.stringify(settings);await writeFile(join(home,'settings.json'),edited);
  await assert.rejects(preparePi(home,connection,[],[],['managed-package']),/Managed package selection changed/);
  assert.equal(await readFile(join(home,'settings.json'),'utf8'),edited);
 }finally{await rm(home,{recursive:true,force:true});}
});
