export type Engine = 'Pi' | 'DSH';
export type Protocol = 'deepseek' | 'openai-chat' | 'openai-responses' | 'anthropic';
export type ModelSpec = {id:string; name:string; contextWindow:number|null; maxTokens:number|null};
export type ModelConnection = {id:string; name:string; provider:string; protocol:Protocol; baseUrl:string; defaultModel:string; models:ModelSpec[]; hasKey:boolean; revision:number};
export type Instance = {schemaVersion:1; id:string; name:string; engine:Engine; projectPath:string; connectionId:string|null; profileId:string; revision:number; createdAt:number; updatedAt:number; status:'not_installed'; projectExists:boolean};
export type RuntimeView = {id:string;status:'not_installed'|'installing'|'starting'|'running'|'stopping'|'stopped'|'failed';phase:string;error:AppError|null;logs:string[];connectionRevision:number|null;instanceRevision:number|null};
export type Task = {id:string; kind:string; entityId:string; state:'pending'|'completed'|'failed'|'interrupted'; createdAt:number;failureCode?:string|null};
export type Snapshot = {mode:'desktop'|'demo'; instances:Instance[]; connections:ModelConnection[]; tasks:Task[]; dataRoot:string;runtime:RuntimeView[]};
export const runtimeLabels:Record<RuntimeView['status'],string>={not_installed:'未安装',installing:'安装中',starting:'启动中',running:'运行中',stopping:'停止中',stopped:'已停止',failed:'需要处理'};
export type InstanceInput = Pick<Instance,'name'|'engine'|'projectPath'|'connectionId'> & {profileId?:string; operationId:string; id:string|null; expectedRevision:number|null};
export type ConnectionInput = Pick<ModelConnection,'name'|'provider'|'protocol'|'baseUrl'|'defaultModel'|'models'> & {operationId:string; id:string|null; expectedRevision:number|null; apiKey:string|null};
export type CatalogInput = {connectionId:string|null; baseUrl:string; protocol:Protocol; apiKey:string|null};
export type CatalogResult = {models:ModelSpec[]; truncated:boolean};
export type AppError = {code:string; message:string};
export function errorText(error:unknown):string {return typeof error==='string' ? error : typeof error==='object' && error!==null && 'message' in error ? String(error.message) : '操作失败，请重试。';}
export function compatibility(engine:Engine, connection:ModelConnection):string|null {
  return engine==='DSH' && !['deepseek','openai-chat'].includes(connection.protocol) ? 'DSH 当前固定组合尚未完成此接口协议的适配' : null;
}
export type RemovedInstance={instance:Omit<Instance,'status'|'projectExists'>;removedAt:number;connectionAvailable:boolean;dataExists:boolean};
export interface Gateway {
  removedInstances():Promise<RemovedInstance[]>;
  restoreRemovedInstance(id:string,operationId:string):Promise<Snapshot>;
  readonly mode:'desktop'|'demo';
  cloneInstance(id:string,name:string,projectPath:string):Promise<Snapshot>;
  cancelEngine(id:string):Promise<void>;
  artifactCache(clean:boolean):Promise<{entries:{profileId:string;inUse:boolean;installed:boolean}[];removed:string[]}>;
  packageCatalog():Promise<PackageCatalog>;
  instancePackage(id:string):Promise<InstancePackage>;
  installPackage(id:string,recipe:Recipe,offline:boolean,expectedRecipe?:Recipe):Promise<void>;
  changePackage(id:string,recipe:Recipe,offline:boolean,expectedRecipe?:Recipe):Promise<unknown>;
  restorePackage(id:string,pointId:string|null):Promise<unknown>;

  snapshot():Promise<Snapshot>;
  saveInstance(input:InstanceInput):Promise<Snapshot>;
  saveConnection(input:ConnectionInput):Promise<Snapshot>;
  deleteConnection(id:string,operationId:string):Promise<Snapshot>;
  deleteInstance(id:string,expectedRevision:number,operationId:string):Promise<Snapshot>;
  pickProject():Promise<string|null>;
  fetchModels(input:CatalogInput):Promise<CatalogResult>;
  engineAction(id:string,action:'install'|'start'|'stop',operationId:string):Promise<void>;
  openEngine(id:string):Promise<void>;
}

export type ExtensionRef={id:string;version:string;enabled:boolean;disabledResources?:string[];resourceRules?:Record<string,string[]>};
export type WorkshopRef={requires?:unknown;kind:'skins'|'pets'|'presets';id:string;version:string;repository:string;commit:string;contentPath:string;files:Record<string,string>};
export type Recipe={thinkingLevel?:'off'|'low'|'high'|'max';workshop?:WorkshopRef[];cover?:''|'developer'|'researcher'|'minimal'|'explorer';origin?:{id:string;revision:number};schemaVersion:1;name:string;description?:string;sourceNote?:string;engine:Engine;profileId:string;extensions:ExtensionRef[];modelProtocols:Protocol[]};
export type RestorePoint={id:string;createdAt:number;reason:string;recipe:Recipe;dataVersion:number};
export type PackageCatalog={extensions:{id:string;name:string;engine:Engine;version:string;description:string;source:string;webSupport:string;scripts:string}[];packs:Recipe[]};
export type InstancePackage={recipe:Recipe;restorePoints:RestorePoint[];workshopResources?:{directory:string;name:string;version?:string;commit?:string;managed:boolean;status:string}[]|null;workshopError?:string|null};
