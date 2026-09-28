import {mkdtemp,mkdir,writeFile} from 'node:fs/promises';
import {tmpdir} from 'node:os';
import {join} from 'node:path';
import {pathToFileURL} from 'node:url';
import assert from 'node:assert/strict';
const artifact=process.argv[2];
const root=await mkdtemp(join(tmpdir(),'perch-pi-resource-load-'));
for(const key of Object.keys(process.env)){if(!['PATH','SYSTEMROOT','WINDIR','COMSPEC','TEMP','TMP','PATHEXT'].includes(key.toUpperCase()))delete process.env[key];}
Object.assign(process.env,{HOME:root,USERPROFILE:root,PI_CODING_AGENT_DIR:join(root,'agent'),DSH_HOME:join(root,'dsh')});
globalThis.fetch=async()=>{throw Error('Network disabled during resource loading verification');};
const cwd=join(root,'project'),agentDir=join(root,'agent');await mkdir(cwd);await mkdir(agentDir);
await writeFile(join(agentDir,'settings.json'),JSON.stringify({packages:[join(artifact,'node_modules/pi-web-access')]}));
const {DefaultResourceLoader}=await import(pathToFileURL(join(artifact,'node_modules/@earendil-works/pi-coding-agent/dist/core/resource-loader.js')).href);
const loader=new DefaultResourceLoader({cwd,agentDir,noContextFiles:true});
await loader.reload();
const result=loader.getExtensions();
console.log(JSON.stringify({root,extensions:result.extensions.map(extension=>({path:extension.path,tools:[...extension.tools.keys()]})),errors:result.errors},null,2));
assert.equal(result.errors.length,0);assert.ok(result.extensions.length>0);assert.ok(result.extensions.some(extension=>extension.tools.size>0));
process.exit(0);
