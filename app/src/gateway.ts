import {invoke} from '@tauri-apps/api/core';
import {compatibility, type Gateway, type Snapshot, type InstanceInput, type ConnectionInput, type CatalogInput, type CatalogResult, type Recipe, type PackageCatalog, type InstancePackage} from './domain';

export class DesktopGateway implements Gateway {
  removedInstances(){return invoke<import('./domain').RemovedInstance[]>('removed_instances');}
  restoreRemovedInstance(id:string,operationId:string){return invoke<Snapshot>('restore_removed_instance',{id,operationId});}
  readonly mode='desktop' as const;
  cloneInstance(id:string,name:string,projectPath:string){return invoke<Snapshot>('clone_instance',{id,name,projectPath});}
  cancelEngine(id:string){return invoke<void>('cancel_engine',{id});}
  artifactCache(clean:boolean){return invoke<{entries:{profileId:string;inUse:boolean;installed:boolean}[];removed:string[]}>('artifact_cache',{clean});}
  packageCatalog(){return invoke<PackageCatalog>('package_catalog');}
  instancePackage(id:string){return invoke<InstancePackage>('instance_package',{id});}
  installPackage(id:string,recipe:Recipe,offline:boolean,expectedRecipe?:Recipe){return invoke<void>('install_package',{id,recipe,offline,expectedRecipe});}
  changePackage(id:string,recipe:Recipe,offline:boolean,expectedRecipe?:Recipe){return invoke('change_package',{id,recipe,offline,expectedRecipe});}
  restorePackage(id:string,pointId:string|null){return invoke('restore_package',{id,pointId});}

  engineAction(id:string,action:'install'|'start'|'stop',operationId:string){return invoke<void>('engine_action',{id,action,operationId});}
  openEngine(id:string){return invoke<void>('engine_open',{id});}
  snapshot(){return invoke<Snapshot>('data_snapshot');}
  saveInstance(input:InstanceInput){return invoke<Snapshot>('save_instance',{input});}
  saveConnection(input:ConnectionInput){return invoke<Snapshot>('save_connection',{input});}
  deleteConnection(id:string,operationId:string){return invoke<Snapshot>('delete_connection',{id,operationId});}
  deleteInstance(id:string,expectedRevision:number,operationId:string){return invoke<Snapshot>('delete_instance',{id,expectedRevision,operationId});}
  pickProject(){return invoke<string|null>('pick_project');}
  fetchModels(input:CatalogInput){return invoke<CatalogResult>('fetch_models',{input});}
}
// Browser previews never access desktop state or keep credentials, even on failure.
export class DemoGateway implements Gateway {
  async removedInstances(){return [];}
  async restoreRemovedInstance(_id:string,_operationId:string):Promise<Snapshot>{throw new Error('恢复已删除实例需要桌面端');}
  readonly mode='demo' as const;
  async cloneInstance(_id:string,_name:string,_projectPath:string):Promise<Snapshot>{throw {message:'克隆需要桌面端'};}
  async cancelEngine(_id:string):Promise<void>{throw {message:'没有桌面任务'};}
  async artifactCache(_clean:boolean){return {entries:[],removed:[]};}
  async packageCatalog():Promise<PackageCatalog>{return {extensions:[],packs:[]};}
  async instancePackage(_id:string):Promise<InstancePackage>{throw {message:'扩展与恢复点需要桌面端'};}
  async installPackage(_id:string,_recipe:Recipe,_offline:boolean):Promise<void>{throw {message:'整合包安装需要桌面端'};}
  async changePackage(_id:string,_recipe:Recipe,_offline:boolean):Promise<unknown>{throw {message:'组合更改需要桌面端'};}
  async restorePackage(_id:string,_pointId:string|null):Promise<unknown>{throw {message:'恢复点需要桌面端'};}

  private data:Snapshot={mode:'demo',instances:[],connections:[],tasks:[],runtime:[],dataRoot:'浏览器内存预览，刷新后重置'};
  async engineAction(_id:string,_action:'install'|'start'|'stop',_operationId:string):Promise<void>{throw {message:'真实安装与启动仅在桌面端提供'};}
  async openEngine(_id:string):Promise<void>{throw {message:'浏览器预览没有真实工作台进程'};}
  private operations=new Set<string>();
  async snapshot(){return structuredClone(this.data);}
  private record(operationId:string,kind:string,entityId:string){this.operations.add(operationId);this.data.tasks.unshift({id:operationId,kind,entityId,state:'completed',createdAt:Date.now()/1000});}
  async saveInstance(input:InstanceInput){
    if(this.operations.has(input.operationId))return this.snapshot();
    if(!input.name.trim()||!input.projectPath.trim())throw {message:'请填写名称和项目目录'};
    const connection=this.data.connections.find(c=>c.id===input.connectionId);
    if(input.connectionId&&!connection)throw {message:'连接已不存在，请重新选择'};
    if(connection&&compatibility(input.engine,connection))throw {message:compatibility(input.engine,connection)};
    const old=this.data.instances.find(i=>i.id===input.id);
    if(old&&old.revision!==input.expectedRevision)throw {message:'实例已更新，请刷新'};
    const instance={schemaVersion:1 as const,id:old?.id??crypto.randomUUID(),name:input.name.trim(),engine:input.engine,projectPath:input.projectPath,connectionId:input.connectionId,profileId:input.engine==='Pi'?'pi-web-0.9.3-pi-0.87.1':'dsh-0.1.5-rc.3',revision:(old?.revision??0)+1,createdAt:old?.createdAt??Date.now()/1000,updatedAt:Date.now()/1000,status:'not_installed' as const,projectExists:true};
    this.data.instances=old?this.data.instances.map(i=>i.id===old.id?instance:i):[...this.data.instances,instance];
    this.record(input.operationId,old?'update_instance':'create_instance',instance.id);return this.snapshot();
  }
  async saveConnection(input:ConnectionInput){
    if(input.apiKey)throw {message:'浏览器演示不接收真实密钥，请在桌面端添加'};
    if(this.operations.has(input.operationId))return this.snapshot();
    const old=this.data.connections.find(c=>c.id===input.id);
    const connection={id:old?.id??crypto.randomUUID(),name:input.name,provider:input.provider,protocol:input.protocol,baseUrl:input.baseUrl,defaultModel:input.defaultModel,models:input.models,hasKey:false,revision:(old?.revision??0)+1};
    this.data.connections=old?this.data.connections.map(c=>c.id===old.id?connection:c):[...this.data.connections,connection];
    this.record(input.operationId,'save_connection',connection.id);return this.snapshot();
  }
  async deleteInstance(_id:string,_expectedRevision:number,_operationId:string):Promise<Snapshot>{throw new Error('删除实例仅在桌面端可用');}
  async deleteConnection(id:string,operationId:string){
    if(this.data.instances.some(i=>i.connectionId===id))throw {message:'仍有实例使用此连接，请先更换绑定'};
    this.data.connections=this.data.connections.filter(c=>c.id!==id);this.record(operationId,'delete_connection',id);return this.snapshot();
  }
  async pickProject(){return null;}
  async fetchModels(_input:CatalogInput):Promise<CatalogResult>{throw {message:'请在桌面端获取模型；浏览器演示不会发送 Key 或请求模型服务。'};}
}
