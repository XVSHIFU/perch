import {test} from 'node:test';
import assert from 'node:assert/strict';
import {mkdtemp,readFile,writeFile} from 'node:fs/promises';
import {tmpdir} from 'node:os';
import {join} from 'node:path';
import {mergeManagedPackages,preparePi} from '../src-tauri/src/pi-config.mjs';

test('package comparison ignores object order but preserves pattern order and manual ownership',()=>{
  const owned={source:'managed',skills:[],prompts:['a','!b']};
  assert.deepEqual(mergeManagedPackages(['manual',{prompts:['a','!b'],skills:[],source:'managed'}],[owned],['new']),['manual','new']);
  for(const previous of [[],[{...owned,prompts:['!b','a']}],[owned,owned]]){
    assert.throws(()=>mergeManagedPackages(previous,[owned],[]),{code:'PERCH_PACKAGE_CONFLICT'});
  }
  assert.throws(()=>mergeManagedPackages(['manual'],[],['manual']),{code:'PERCH_PACKAGE_CONFLICT'});
  assert.deepEqual(mergeManagedPackages(['manual',owned],[owned],[]),['manual']);
});

test('conflict leaves persisted settings and model configuration unchanged',async()=>{
  const home=await mkdtemp(join(tmpdir(),'perch-pi-conflict-'));
  const settings=JSON.stringify({packages:[{source:'managed',skills:[]}],perchManagedPackages:['managed'],theme:'custom'});
  const models=JSON.stringify({providers:{existing:{name:'Existing'}}});
  await writeFile(join(home,'settings.json'),settings);
  await writeFile(join(home,'models.json'),models);
  await assert.rejects(preparePi(home,{protocol:'openai-chat',name:'Fixture',baseUrl:'http://127.0.0.1:9',defaultModel:'fixture'},[],[],['managed']),{code:'PERCH_PACKAGE_CONFLICT'});
  assert.equal(await readFile(join(home,'settings.json'),'utf8'),settings);
  assert.equal(await readFile(join(home,'models.json'),'utf8'),models);
});
