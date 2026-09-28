import type {Recipe} from './domain';
export type Draft={id:string;revision:number;recipe:Recipe;ready:boolean;lastReady?:Recipe};
const object=(value:unknown):value is Record<string,unknown>=>typeof value==='object'&&value!==null&&!Array.isArray(value);
function recipe(value:unknown):value is Recipe{
 if(!object(value)||value.schemaVersion!==1||!['DSH','Pi'].includes(String(value.engine))||typeof value.name!=='string'||typeof value.profileId!=='string')return false;
 if(value.thinkingLevel!==undefined&&!['off','low','high','max'].includes(String(value.thinkingLevel)))return false;
 if(value.workshop!==undefined&&(!Array.isArray(value.workshop)||!value.workshop.every(item=>object(item)&&['skins','pets','presets'].includes(String(item.kind))&&['id','version','repository','commit','contentPath'].every(key=>typeof item[key]==='string')&&object(item.files)&&Object.values(item.files).every(hash=>typeof hash==='string'))))return false;
 return Array.isArray(value.modelProtocols)&&value.modelProtocols.every(item=>['deepseek','openai-chat','openai-responses','anthropic'].includes(item))&&Array.isArray(value.extensions)&&value.extensions.every(item=>object(item)&&typeof item.id==='string'&&typeof item.version==='string'&&typeof item.enabled==='boolean'&&(item.resourceRules===undefined||(object(item.resourceRules)&&Object.entries(item.resourceRules).every(([kind,rules])=>['extensions','skills','prompts','themes'].includes(kind)&&Array.isArray(rules)&&rules.every(rule=>typeof rule==='string'))))&&(item.disabledResources===undefined||(Array.isArray(item.disabledResources)&&item.disabledResources.every(kind=>typeof kind==='string'))))&&['description','sourceNote','cover'].every(key=>value[key]===undefined||typeof value[key]==='string');
}
export function parseEditing(raw:string|null):{recipe?:Recipe;editing?:Draft;error?:string}{
 if(raw===null)return {};
 try{
  const value:unknown=JSON.parse(raw);
  if(!object(value)||!recipe(value.recipe))throw new Error();
  const saved=value.editing;
  if(saved!==undefined&&(!object(saved)||typeof saved.id!=='string'||!Number.isSafeInteger(saved.revision)||Number(saved.revision)<1||typeof saved.ready!=='boolean'||!recipe(saved.recipe)))throw new Error();
  return {recipe:value.recipe,editing:saved as Draft|undefined};
 }catch{return {error:'编辑缓存格式无法恢复。原始缓存暂时保留；重新开始编辑时会先备份，已保存的整合包不受影响。'};}
}
