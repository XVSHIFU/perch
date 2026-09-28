import type {Instance,Snapshot} from './domain';
import {Button,I} from './ui';

const names:Record<string,string>={engine_install:'安装引擎',engine_start:'启动工作台',engine_stop:'停止工作台',create_instance:'创建实例',update_instance:'修改实例',save_connection:'保存模型连接',delete_connection:'删除模型连接',delete_instance:'删除实例（保留数据）',restore_removed_instance:'恢复已删除实例'};
const states={completed:'已完成',pending:'执行中',failed:'失败',interrupted:'已中断'};
const failures:Record<string,string>={CANCELLED:'操作已取消。',INSTANCE_BUSY:'实例当时正在执行其他操作。',ALREADY_RUNNING:'实例当时已在运行。',PROJECT_MISSING:'项目目录不存在。',INSTALL_TIMEOUT:'安装超时，请检查网络后重试。',INSTALL_INTEGRITY:'安装完整性校验未通过，暂存内容未启用。',START_FAILED:'工作台启动未完成。',PROCESS_EXIT:'引擎意外退出。',PI_WORKSPACE:'Pi 工作区准备失败。',REVISION_CONFLICT:'操作期间实例配置已变化。',WORKSHOP_PREPARE:'工坊资源准备失败。',ENGINE_TASK:'引擎任务异常中断。',OFFLINE_ARTIFACT_MISSING:'离线安装缺少固定工件。',ENGINE_FAILED:'任务未完成，详细原因未保存。'};

export default function ActivityPage({data,loading,onRefresh,onInstance,onConnections}:{data:Snapshot;loading:boolean;onRefresh:()=>void;onInstance:(instance:Instance)=>void;onConnections:()=>void}){
 return <>
  <div className="page-heading"><div><h1>任务与活动</h1><p>查看安装、启动和配置记录，进入对应实例处理问题。</p></div><Button disabled={loading} onClick={onRefresh}>刷新</Button></div>
  {!data.tasks.length?<div className="empty"><h3>还没有任务记录</h3><p>创建实例、保存连接或启动工作台后，会在这里留下记录。</p></div>:data.tasks.map(task=>{
   const instance=data.instances.find(item=>item.id===task.entityId);
   const connection=data.connections.find(item=>item.id===task.entityId);
   const failed=task.state==='failed'||task.state==='interrupted';
   return <div className="row" key={task.id}>
    <I name={task.state==='completed'?'check':failed?'alert':'clock'}/>
    <div className="grow"><h3>{names[task.kind]??task.kind} · {instance?.name??connection?.name??`已移除对象 (${task.entityId.slice(0,8)})`}</h3>
     <p className="sub">{new Date(task.createdAt*1000).toLocaleString()}</p>
     {failed&&<p>{task.state==='interrupted'?'任务未完成，应用重启后标记为中断。':failures[task.failureCode??'']??'此记录未保存详细错误原因。'}{instance?'进入实例查看当前状态和日志后重试。':connection?'进入模型连接检查配置后重试。':'对应对象已移除，此记录仅供查看。'}</p>}
    </div>
    <span className={'pill '+(failed?'warn':'')}>{states[task.state]}</span>
    {instance?<Button onClick={()=>onInstance(instance)}>{failed?'进入实例处理':'查看实例'}</Button>:connection?<Button onClick={onConnections}>{failed?'检查模型连接':'查看模型连接'}</Button>:null}
   </div>;
  })}
 </>;
}
