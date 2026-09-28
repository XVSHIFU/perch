import {mkdtemp,mkdir,cp} from 'node:fs/promises';
import {tmpdir} from 'node:os';
import {join} from 'node:path';
import {pathToFileURL} from 'node:url';
import assert from 'node:assert/strict';
const [artifact,source]=process.argv.slice(2);
assert.ok(artifact&&source,'Provide installed DSH artifact and imported Skill directory');
const root=await mkdtemp(join(tmpdir(),'perch-dsh-skill-load-'));
for(const key of Object.keys(process.env)){if(!['PATH','SYSTEMROOT','WINDIR','COMSPEC','TEMP','TMP','PATHEXT'].includes(key.toUpperCase()))delete process.env[key];}
Object.assign(process.env,{HOME:root,USERPROFILE:root,DSH_HOME:join(root,'agent-data'),DSH_AGENTS_HOME:join(root,'agents')});
globalThis.fetch=async()=>{throw Error('Network disabled during Skill loading verification');};
const cwd=join(root,'project'),skillRoot=join(root,'config/managed-skills/local-skill-fixture');
await mkdir(cwd);await mkdir(skillRoot,{recursive:true});await cp(source,join(skillRoot,'skill'),{recursive:true});
const {FileSystemSkillProvider}=await import(pathToFileURL(join(artifact,'node_modules/@deepseek-ai/dsh-skill-filesystem/lib/index.js')).href);
const warnings=[],controller=new AbortController();
// Exercise the actual provider's native filesystem path; no model or application service is supplied.
const context={get:()=>undefined,logger:{warn:message=>warnings.push(message)}};
const control={signal:controller.signal,invalidate:()=>{}};
let provider=new FileSystemSkillProvider(context,control,{customSkillDirs:[skillRoot],includeDefaultRoots:false,watch:false});
try{
 const candidates=await provider.list({cwd,signal:controller.signal});
 assert.ok(Array.isArray(candidates));assert.equal(candidates.length,1);assert.equal(candidates[0].name,'brand-guidelines');
 const skill=await provider.get(candidates[0],{cwd,signal:controller.signal});
 assert.ok(skill.content.length>0);assert.equal(skill.resourceBase.path,join(skillRoot,'skill'));assert.equal(warnings.length,0);
 await provider.dispose();
 provider=new FileSystemSkillProvider(context,control,{customSkillDirs:[],includeDefaultRoots:false,watch:false});
 assert.deepEqual(await provider.list({cwd,signal:controller.signal}),[]);
 console.log(JSON.stringify({root,name:skill.name,resourceBase:skill.resourceBase,contentLength:skill.content.length,warnings,disabledSkillCount:0},null,2));
}finally{await provider.dispose();controller.abort();}
