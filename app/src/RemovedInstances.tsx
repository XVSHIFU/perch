import {useEffect,useState} from 'react';
import {errorText,type Gateway,type RemovedInstance,type Snapshot} from './domain';
import {useReportBusy} from './operationStatus';
import {Button,Logo} from './ui';
export default function RemovedInstances({gateway,onRestored}:{gateway:Gateway;onRestored:(snapshot:Snapshot)=>void}){
 const [items,setItems]=useState<RemovedInstance[]>([]),[loading,setLoading]=useState(true),[busy,setBusy]=useState(''),[error,setError]=useState(''),[notice,setNotice]=useState('');
 useReportBusy(!!busy);
 async function load(){setLoading(true);setError('');try{setItems(await gateway.removedInstances());}catch(e){setError(errorText(e));}finally{setLoading(false);}}
 useEffect(()=>{void load();},[gateway]);
 async function restore(item:RemovedInstance){
  setBusy(item.instance.id);setError('');setNotice('');
  try{const next=await gateway.restoreRemovedInstance(item.instance.id,crypto.randomUUID());setItems(rows=>rows.filter(row=>row.instance.id!==item.instance.id));onRestored(next);setNotice(`已恢复「${item.instance.name}」到实例列表，尚未启动。${item.connectionAvailable?'':'请进入实例选择模型连接。'}`);}
  catch(e){setError(errorText(e));}finally{setBusy('');}
 }
 return <section className="instance-runtime"><div className="section-title"><h2>已删除实例</h2><Button disabled={loading||!!busy} onClick={()=>void load()}>刷新</Button></div>
  <p className="sub">恢复保留的实例记录与原数据引用，不启动引擎、不修改项目文件。</p>
  {error&&<p className="form-error" role="alert">{error}</p>}{notice&&<p role="status">{notice}</p>}
  {loading?<p role="status">正在读取…</p>:!items.length?<p className="sub">没有保留数据的已删除实例。</p>:items.map(item=><div className="row" key={item.instance.id}><Logo engine={item.instance.engine}/><div className="grow"><h3>{item.instance.name}</h3><p className="sub path-text">{item.instance.projectPath}</p><p className="sub">{new Date(item.removedAt*1000).toLocaleString()} 移除 · {item.connectionAvailable?'沿用原模型连接':'恢复后需选择模型连接'}</p>{!item.dataExists&&<p className="form-error">保留的数据目录不存在，需先找回原目录。</p>}</div><Button disabled={!!busy||!item.dataExists} onClick={()=>void restore(item)}>{busy===item.instance.id?'正在恢复…':'恢复到实例列表'}</Button></div>)}
 </section>;
}
