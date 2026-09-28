import {mkdir,readFile,writeFile,rename} from 'node:fs/promises';
import {join,resolve,dirname} from 'node:path';
import {mergeThinkingPreset} from './public-preset.mjs';

async function readObject(path) {
  try {
    const value=JSON.parse(await readFile(path,'utf8'));
    if(!value || Array.isArray(value) || typeof value!=='object')throw Error('Invalid configuration');
    return value;
  } catch(error) { if(error.code==='ENOENT')return {}; throw error; }
}

// Object key order has no meaning in Pi's package selection. Array order does:
// ordered include/exclude patterns must not be sorted when comparing selections.
function sameSelection(a,b) {
  if(a===b)return true;
  if(!a||!b||typeof a!=='object'||typeof b!=='object')return false;
  if(Array.isArray(a)||Array.isArray(b))return Array.isArray(a)&&Array.isArray(b)&&a.length===b.length&&a.every((item,index)=>sameSelection(item,b[index]));
  const keys=Object.keys(a);
  return keys.length===Object.keys(b).length&&keys.every(key=>Object.hasOwn(b,key)&&sameSelection(a[key],b[key]));
}

export function mergeManagedPackages(previous,owned,next) {
  const source=item=>typeof item==='string'?item:item?.source;
  const conflict=()=>{const error=Error('Managed package selection changed in workbench');error.code='PERCH_PACKAGE_CONFLICT';throw error;};
  // Never claim an existing manual declaration, including one identical to next.
  // Its ownership must first be resolved explicitly by the user.
  for(const item of owned){
    const matches=previous.filter(entry=>source(entry)===source(item));
    if(matches.length!==1||!sameSelection(matches[0],item))conflict();
  }
  const remaining=previous.filter(item=>!owned.some(entry=>source(entry)===source(item)));
  if(next.some(item=>remaining.some(entry=>source(entry)===source(item))))conflict();
  return [...remaining,...next];
}

// Preserve unrelated providers and settings. The only persisted credential is
// an explicit environment reference, resolved by the pinned Pi SDK.
export async function preparePi(home,connection,extensions=[],skills=[],packages=[],thinkingLevel) {
  const api={deepseek:'openai-completions','openai-chat':'openai-completions','openai-responses':'openai-responses',anthropic:'anthropic-messages'}[connection.protocol];
  if(!api)throw Error('Unsupported Pi protocol');
  await mkdir(home,{recursive:true});
  const modelsPath=join(home,'models.json'),settingsPath=join(home,'settings.json');
  const models=await readObject(modelsPath),settings=await readObject(settingsPath);
  const preset=mergeThinkingPreset(settings.defaultThinkingLevel,settings.perchThinkingPreset,thinkingLevel);
  if(preset.value===undefined)delete settings.defaultThinkingLevel;else settings.defaultThinkingLevel=preset.value;
  if(preset.owned===undefined)delete settings.perchThinkingPreset;else settings.perchThinkingPreset=preset.owned;
  if(models.providers && (typeof models.providers!=='object'||Array.isArray(models.providers)))throw Error('Invalid providers');
  models.providers??={};
  const catalog=connection.models?.length?connection.models:[{id:connection.defaultModel,name:connection.defaultModel}];
  models.providers.perch={name:connection.name,baseUrl:connection.baseUrl,api,apiKey:'${PERCH_MODEL_KEY}',models:catalog.map(model=>Object.fromEntries(Object.entries(model).filter(([,value])=>value!=null)))};
  const managed=resolve(home,'../config/managed-extensions').toLowerCase();
  const previous=Array.isArray(settings.extensions)?settings.extensions:[];
  settings.extensions=[...previous.filter(item=>typeof item!=='string'||dirname(resolve(item)).toLowerCase()!==managed),...extensions];
  const skillsRoot=resolve(home,'../config/managed-skills').toLowerCase();
  const previousSkills=Array.isArray(settings.skills)?settings.skills:[];
  if(previousSkills.length||skills.length)settings.skills=[...previousSkills.filter(item=>typeof item!=='string'||dirname(resolve(item)).toLowerCase()!==skillsRoot),...skills];
  if(packages.length||settings.perchManagedPackages?.length){
  const previousPackages=Array.isArray(settings.packages)?settings.packages:[];
  const owned=Array.isArray(settings.perchManagedPackages)?settings.perchManagedPackages:[];
  settings.packages=mergeManagedPackages(previousPackages,owned,packages);
  settings.perchManagedPackages=packages;
  }
  settings.defaultProvider='perch';settings.defaultModel=connection.defaultModel;
  for(const [path,value] of [[modelsPath,models],[settingsPath,settings]]) {
    await writeFile(path+'.pending',JSON.stringify(value,null,2),{mode:0o600});
    await rename(path+'.pending',path);
  }
}
