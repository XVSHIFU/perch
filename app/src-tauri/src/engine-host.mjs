// The child waits for its parent to attach the Windows Job before doing any work.
// Credentials travel over an anonymous pipe, never argv or a generated file.
import {createInterface} from 'node:readline';
import {spawn} from 'node:child_process';
import {createRequire,registerHooks} from 'node:module';
import {pathToFileURL} from 'node:url';
import {join,resolve,dirname} from 'node:path';
import {readFile,writeFile,rename,mkdir,readdir,lstat,realpath} from 'node:fs/promises';
// Real profile-local ESM proxies also satisfy DSH preset discovery's filesystem
// checks. A resolver hook alone cannot satisfy those checks on broken junctions.
async function prepareProfileModules(install,home,require){
 const entries=new Set();
 const {resolve}=require('resolve.exports');
 const source=join(install,'node_modules','@deepseek-ai');
 for(const name of await readdir(source)){
  const packageDir=join(source,name);
  const manifest=JSON.parse(await readFile(join(packageDir,'package.json'),'utf8'));
  const destination=join(home,'profiles','web','node_modules','@deepseek-ai',name);
  try{
   const stat=await lstat(destination);
   if(stat.isSymbolicLink())continue;
   let existing;
   try{existing=JSON.parse(await readFile(join(destination,'package.json'),'utf8'));}
   catch{continue;} // An unmarked local directory belongs to the user.
   if(existing.perchModuleProxy!==true)continue;
   if(existing.version===manifest.version && existing.perchProxyInstall===install){
    const files=Object.values(existing.exports??{});
    const valid=await Promise.all(files.map(file=>lstat(join(destination,file)).then(s=>s.isFile(),()=>false)));
    if(valid.every(Boolean)){
     for(const file of files)entries.add(pathToFileURL(join(destination,file)).href);
     continue;
    }
   }
  }catch(error){if(error.code!=='ENOENT')throw error;}
  const declared=manifest.exports;
  const subpaths=declared && typeof declared==='object' && Object.keys(declared).some(k=>k.startsWith('.'))
   ?Object.keys(declared).filter(k=>(k==='.'||k.startsWith('./'))&&!k.includes('*')&&k!=='./package.json') : ['.'];
  const targets=[];
  for(const subpath of subpaths){
   let candidates;
   try{candidates=declared?resolve(manifest,subpath):[manifest.main??'index.js'];}
   catch(error){if(error.message.startsWith('No known conditions for '))continue;throw error;}
   for(const candidate of candidates??[]){
    const target=join(packageDir,candidate);
    try{if((await lstat(target)).isFile()){targets.push([subpath,pathToFileURL(target).href]);break;}}
    catch(error){if(error.code!=='ENOENT')throw error;}
   }
  }
  await mkdir(destination,{recursive:true});
  for(const [index,[,target]] of targets.entries()){
   const specifier=JSON.stringify(target);
   await writeFile(join(destination,`entry-${index}.js`),`export * from ${specifier};\nimport * as source from ${specifier};\nexport default source.default;\n`);
   entries.add(pathToFileURL(join(destination,`entry-${index}.js`)).href);
  }
  await writeFile(join(destination,'package.json'),JSON.stringify({name:manifest.name,version:manifest.version,private:true,type:'module',perchModuleProxy:true,perchProxyInstall:install,exports:Object.fromEntries(targets.map(([subpath],index)=>[subpath,`./entry-${index}.js`]))}));
 }
 return entries;
}
const input=createInterface({input:process.stdin});
let started=false;
function stop(){process.emit('SIGINT');setTimeout(()=>process.exit(0),4500).unref();}
input.on('close',stop);
input.on('line',async line=>{
 if(started){if(line==='stop'){input.close();}return;}
 started=true;
 let stage='读取启动参数';
 try {
  const options=JSON.parse(line);line='';
  if(options.mode==='install'||options.mode==='resolve'){
   await writeFile(join(process.cwd(),'.npmrc'),'');await writeFile(join(process.cwd(),'.npm-global'),'');
   process.argv=[process.execPath,options.npm,...(options.mode==='resolve'?['install','--package-lock-only','--engine-strict']:['ci']),'--ignore-scripts','--no-audit','--no-fund','--registry=https://registry.npmjs.org','--userconfig='+join(process.cwd(),'.npmrc'),'--globalconfig='+join(process.cwd(),'.npm-global')];
   await import(pathToFileURL(options.npm).href);return;
  }
  if(options.engine==='Pi'){
   process.env.PI_CODING_AGENT_DIR=options.home;
   process.env.PERCH_MODEL_KEY=options.key;delete options.key;
   process.env.NEXT_TELEMETRY_DISABLED='1';
   const {preparePi}=await import('./pi-config.mjs');
   stage='保存 Pi 配置';await preparePi(options.home,options.connection,options.extensions??[],options.skills??[],options.packages??[],options.thinkingLevel);
   process.argv=[process.execPath,options.entry,'--hostname','127.0.0.1','--port',String(options.port),'--no-open'];
   stage='启动 Pi';
   const child=spawn(process.execPath,process.argv.slice(1),{env:process.env,stdio:['ignore','inherit','inherit'],windowsHide:true});
   process.on('SIGINT',()=>child.kill('SIGINT'));
   child.on('error',()=>{console.error('PERCH_BOOT_FAILED: Pi 进程无法启动');process.exit(1);});
   child.on('exit',code=>process.exit(code??1));return;
  }
  process.env.DSH_HOME=options.home;
  process.env.DSH_TELEMETRY_DISABLED='1';
  process.env.PERCH_MODEL_KEY=options.key;delete options.key;
  const require=createRequire(join(options.install,'package.json'));
  stage='读取实例配置';
  const {parseDocument}=require('yaml');
  await mkdir(options.home,{recursive:true});
  const settings=join(options.home,'settings.yaml');
  let source='';try{source=await readFile(settings,'utf8');}catch(e){if(e.code!=='ENOENT')throw e;}
  const doc=parseDocument(source);if(doc.errors.length)throw Error('Invalid settings');
  const c=options.connection;
  const models=(c.models.length?c.models:[{id:c.defaultModel,name:c.defaultModel}]).map(m=>Object.fromEntries(Object.entries(m).filter(([,v])=>v!==null)));
  let provider;
  stage='生成模型配置';
  if(c.protocol==='deepseek'){
   if(doc.hasIn(['llm-pi-ai','providers','perch']))doc.deleteIn(['llm-pi-ai','providers','perch']);
   provider='deepseek-official';doc.setIn(['llm-deepseek','apiKeyEnv'],'PERCH_MODEL_KEY');
   doc.setIn(['llm-deepseek','baseURL'],c.baseUrl);doc.setIn(['llm-deepseek','models'],models);
  }else{
   if(doc.getIn(['llm-deepseek','apiKeyEnv'])==='PERCH_MODEL_KEY')doc.deleteIn(['llm-deepseek','apiKeyEnv']);
   provider='perch';doc.setIn(['llm-pi-ai','providers','perch'],{displayName:c.name,api:'openai-completions',apiKeyEnv:'PERCH_MODEL_KEY',baseURL:c.baseUrl,models});
  }
  const skillRoot=resolve(options.home,'../config/managed-skills').toLowerCase();
  const previousSkills=doc.getIn(['skill-filesystem','customSkillDirs'])?.toJSON?.()??[];
  if(!Array.isArray(previousSkills))throw Error('Invalid skill roots');
  doc.setIn(['skill-filesystem','customSkillDirs'],[...previousSkills.filter(path=>typeof path==='string'&&dirname(resolve(path)).toLowerCase()!==skillRoot),...(options.skills??[])]);
  const {applyDshThinkingPreset}=await import('./public-preset.mjs');
  applyDshThinkingPreset(doc,options.thinkingLevel);
  doc.setIn(['agent-default-model','provider'],provider);doc.setIn(['agent-default-model','model'],c.defaultModel);
  stage='保存实例配置';
  await writeFile(settings+'.pending',String(doc),{mode:0o600});await rename(settings+'.pending',settings);
  const bundleArgs=[],externalNames=[],externalRoots=[];
  for(const directory of options.packages??[]){
   const manifest=JSON.parse(await readFile(join(directory,'package.json'),'utf8'));
   const patch=manifest.dsh?.bundle?.patch;
   if(typeof patch!=='string'||patch.includes('\\')||patch.includes(':')||patch.split('/').includes('..'))throw Error('Invalid bundle patch');
   const root=await realpath(directory),file=await realpath(resolve(directory,patch));
   if(!file.toLowerCase().startsWith(root.toLowerCase()+(process.platform==='win32'?'\\':'/'))||!(await lstat(file)).isFile())throw Error('Bundle patch outside package');
   bundleArgs.push('--patch',file);externalNames.push(manifest.name);externalRoots.push([pathToFileURL(directory+'/').href,manifest.name]);
  }
  process.argv=[process.execPath,options.entry,'--profile','web',...bundleArgs,'--patch',options.patch,'--host','127.0.0.1','--port',String(options.port),'--no-open'];
  stage='加载引擎';
  const proxyEntries=await prepareProfileModules(options.install,options.home,require);
  // Some Windows data directories cannot traverse DSH's generated junctions.
  // Keep profile-local resolution first; resolve missing built-in packages from
  // this pinned installation only, without changing or deleting profile files.
  const profileUrl=pathToFileURL(join(options.home,'profiles')+'/').href;
  const installUrl=pathToFileURL(join(options.install,'package.json')).href;
  const loadedBundles=new Set();
  registerHooks({load(url,context,nextLoad){
   const result=nextLoad(url,context);
   for(const [root,name] of externalRoots){if(url.startsWith(root)&&!loadedBundles.has(name)){loadedBundles.add(name);console.log(`PERCH_BUNDLE_MODULE_LOADED: ${name}`);}}
   return result;
  },resolve(specifier,context,nextResolve){
   try{
    const resolved=nextResolve(specifier,context);
    // Client asset discovery must see the original package's manifest/assets.
    if(proxyEntries.has(resolved.url) && specifier.startsWith('@deepseek-ai/'))return nextResolve(specifier,{...context,parentURL:installUrl});
    return resolved;
   }
   catch(error){
    if(error.code!=='ERR_MODULE_NOT_FOUND' || !context.parentURL?.startsWith(profileUrl) || (!/^@deepseek-ai\/[a-z0-9-]+(?:\/|$)/.test(specifier)&&!externalNames.some(name=>specifier===name||specifier.startsWith(name+'/'))))throw error;
    return nextResolve(specifier,{...context,parentURL:installUrl});
   }
  }});
  const {runCli}=await import(pathToFileURL(options.entry).href);
  stage='启动引擎';await runCli();
 }catch(error){
  if(error?.code==='PERCH_PRESET_CONFLICT')console.error('PERCH_PRESET_CONFLICT: 默认思考强度与工作台设置冲突，原配置未覆盖。请在组合编辑中选择与工作台一致的值，或取消此预设后重试。');
  if(error?.code==='PERCH_PACKAGE_CONFLICT')console.error('PERCH_PACKAGE_CONFLICT: Pi 工作台中的包选择与栖点记录不一致；已保留原配置，未覆盖。需要核对包的启用状态及资源选择。');
  if(error?.code==='EADDRINUSE'||error?.cause?.code==='EADDRINUSE')console.error('EADDRINUSE');
  const code=['ENOENT','EACCES','EPERM','MODULE_NOT_FOUND','ERR_MODULE_NOT_FOUND','EADDRINUSE'].includes(error?.code)?` (${error.code})`:'';
  console.error(`PERCH_BOOT_FAILED: ${stage}失败${code}，请检查实例设置文件和安装完整性`);
  // Only source locations, never exception messages (which may contain secrets).
  const pending=[error],seen=new Set();
  while(pending.length && seen.size<20){
   const cause=pending.pop();if(!cause || seen.has(cause))continue;seen.add(cause);
   const frames=String(cause.stack??'').split('\n').slice(1).map(line=>line.match(/([\w.-]+[\\/]lib[\\/][\w.-]+\.(?:m?js|cjs):\d+:\d+)\)?$/)?.[1]).filter(Boolean).slice(0,3);
   if(frames.length)console.error(`PERCH_BOOT_TRACE_${seen.size}: ${frames.join(' -> ')}`);
   if(Array.isArray(cause.errors))pending.push(...cause.errors);
   if(cause.cause)pending.push(cause.cause);
  }
  process.exit(1);
 }
});
