import {useState} from 'react';
import {errorText,type Gateway,type Instance,type Snapshot} from './domain';
import DetailDrawer from './DetailDrawer';
import {useReportBusy} from './operationStatus';
import {Button} from './ui';

export default function DeleteInstance({instance,gateway,disabled,dataRoot,onDeleted}:{instance:Instance;gateway:Gateway;disabled:boolean;dataRoot:string;onDeleted:(next:Snapshot)=>void}){
 const [open,setOpen]=useState(false),[busy,setBusy]=useState(false),[error,setError]=useState('');
 useReportBusy(busy);
 async function remove(){
  setBusy(true);setError('');
  try{const next=await gateway.deleteInstance(instance.id,instance.revision,crypto.randomUUID());setOpen(false);onDeleted(next);}
  catch(e){setError(errorText(e));}finally{setBusy(false);}
 }
 return <div className="instance-remove">
  <Button disabled={disabled||gateway.mode!=='desktop'} onClick={()=>{setError('');setOpen(true);}}>删除实例</Button>
  {disabled&&<p className="muted small">先停止实例并等待当前操作结束，即可删除。</p>}
  <DetailDrawer open={open} title={`删除「${instance.name}」`} closeDisabled={busy} onClose={()=>setOpen(false)}>
   <p>删除后，该实例将从启动台和侧栏移除。</p>
   <div className="detail-summary"><div><h3>保留你的文件</h3><p>项目文件夹和共享模型连接不变。实例配置、会话与恢复点保留在原数据目录，此操作不会释放这些文件占用的空间。</p></div></div>
   <div className="fact"><span>项目目录 · 保留</span><span className="path-text">{instance.projectPath}</span></div>
   <div className="fact"><span>实例数据 · 保留</span><span className="path-text">{dataRoot}\instances\{instance.id}</span></div>
   <p className="muted">之后可在“恢复中心 → 已删除实例”恢复到列表。</p>
   {error&&<p className="form-error" role="alert">{error}</p>}
   <div className="form-actions"><Button disabled={busy} onClick={()=>setOpen(false)}>取消</Button><Button primary disabled={busy||disabled} onClick={()=>void remove()}>{busy?'正在删除…':'确认删除实例'}</Button></div>
  </DetailDrawer>
 </div>;
}
