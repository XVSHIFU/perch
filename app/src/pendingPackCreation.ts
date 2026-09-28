import type {InstanceDraft} from './InstanceForm';
import type {Recipe} from './domain';

const key='perch.pending-pack-creations.v1';
export type PendingCreation={recipe:string;draft:InstanceDraft;signature:string;operationId:string};
function entries(storage:Storage):PendingCreation[]{
 const raw=storage.getItem(key);
 if(!raw)return [];
 const value=JSON.parse(raw);
 if(!Array.isArray(value)||!value.every(item=>item&&typeof item.recipe==='string'&&typeof item.signature==='string'&&typeof item.operationId==='string'&&item.draft&&['name','projectPath','connectionId'].every(field=>typeof item.draft[field]==='string')&&['DSH','Pi'].includes(item.draft.engine)&&(item.draft.profileId===undefined||typeof item.draft.profileId==='string')))throw new Error('未完成创建的缓存无法读取，请保留当前表单重试。');
 return value;
}
export function pendingCreation(storage:Storage,recipe:Recipe){return entries(storage).find(item=>item.recipe===JSON.stringify(recipe));}
export function rememberCreation(storage:Storage,entry:PendingCreation){storage.setItem(key,JSON.stringify([...entries(storage).filter(item=>item.recipe!==entry.recipe),entry]));}
export function forgetCreation(storage:Storage,operationId:string){storage.setItem(key,JSON.stringify(entries(storage).filter(item=>item.operationId!==operationId)));}
