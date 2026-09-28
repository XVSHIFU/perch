import type {Instance,ModelConnection} from './domain';
export function connectionIssue(instance:Instance,connection?:ModelConnection){
 return !connection?'尚未绑定模型连接':!connection.hasKey?'模型连接尚未配置密钥':instance.engine==='DSH'&&!['deepseek','openai-chat'].includes(connection.protocol)?'模型连接协议不适用于 DSH':'';
}
export function startBlockReason(mode:string,instance:Instance,connection?:ModelConnection){
 return mode!=='desktop'?'请在桌面端启动':!instance.projectExists?'项目目录不存在，请编辑实例':connectionIssue(instance,connection);
}
export function runtimeActionBlockReason(status:string,mode:string,instance:Instance,connection?:ModelConnection){
 return status==='running'?(mode!=='desktop'?'请在桌面端打开工作台':''):startBlockReason(mode,instance,connection);
}
