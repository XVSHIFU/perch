import {mkdtemp,mkdir,writeFile} from 'node:fs/promises';
import {tmpdir} from 'node:os';
import {join,resolve} from 'node:path';
import {spawn} from 'node:child_process';
import assert from 'node:assert/strict';
const npm=process.argv[2];assert.ok(npm,'Pass the pinned npm CLI path');
const root=await mkdtemp(join(tmpdir(),'perch-engine-constraint-'));
const host=resolve('app/src-tauri/src/engine-host.mjs');
for(const [label,range,accepted] of [['compatible','>=1',true],['incompatible','>=1000',false]]){
  const cwd=join(root,label),dependency=join(cwd,'dependency');await mkdir(dependency,{recursive:true});
  await writeFile(join(dependency,'package.json'),JSON.stringify({name:'constraint-fixture',version:'1.0.0',engines:{node:range}}));
  await writeFile(join(cwd,'package.json'),JSON.stringify({name:'perch-constraint-test',version:'1.0.0',private:true,dependencies:{'constraint-fixture':'file:./dependency'}}));
  const env=Object.fromEntries(Object.entries(process.env).filter(([key])=>['SYSTEMROOT','WINDIR','PATH','TEMP','TMP','COMSPEC','PATHEXT'].includes(key.toUpperCase())));
  Object.assign(env,{USERPROFILE:root,APPDATA:root,LOCALAPPDATA:root,npm_config_cache:join(root,'cache')});
  const child=spawn(process.execPath,[host],{cwd,env,windowsHide:true,stdio:['pipe','pipe','pipe']});
  let diagnostic='';child.stdout.resume();child.stderr.on('data',data=>{diagnostic=(diagnostic+data).slice(-8192);});
  const timer=setTimeout(()=>child.kill(),30000);
  const code=await new Promise((resolve,reject)=>{child.once('error',reject);child.once('exit',resolve);child.stdin.write(JSON.stringify({mode:'resolve',npm})+'\n');});
  clearTimeout(timer);child.stdin.destroy();
  assert.equal(code===0,accepted,`${label}: ${diagnostic}`);
  if(!accepted)assert.match(diagnostic,/EBADENGINE/);
}
console.log(JSON.stringify({root,result:'Actual npm resolver accepts matching engines and rejects incompatible engines',modelCalls:0}));
