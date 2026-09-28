import {useEffect,useState} from 'react';
import {errorText,type Gateway,type Instance,type RuntimeView} from './domain';
import PackagePanel from './PackagePanel';
import RemovedInstances from './RemovedInstances';
import {Button} from './ui';

export default function RecoveryPage({gateway,instances,runtime,onOpen,onRestored}:{gateway:Gateway;instances:Instance[];runtime:RuntimeView[];onOpen:(instance:Instance)=>void;onRestored:(snapshot:import('./domain').Snapshot)=>void}){
 const [id,setId]=useState(instances[0]?.id??''),[cache,setCache]=useState<Awaited<ReturnType<Gateway['artifactCache']>>>(),[error,setError]=useState(''),[busy,setBusy]=useState(false);
 useEffect(()=>{void gateway.artifactCache(false).then(setCache).catch(e=>setError(errorText(e)));},[]);
 useEffect(()=>{if(!instances.some(item=>item.id===id))setId(instances[0]?.id??'');},[instances,id]);
 const selected=instances.find(i=>i.id===id);
 async function clean(){setBusy(true);setError('');try{setCache(await gateway.artifactCache(true));}catch(e){setError(errorText(e));}finally{setBusy(false);}}
 return <><div className="page-heading"><div><h1>恢复中心</h1><p>恢复实例内的数据与固定组合，项目文件始终保留在原处。</p></div></div>
 {error&&<p className="form-error" role="alert">{error}</p>}
 {instances.length?<><label className="field"><span>查看实例</span><select className="input" value={id} onChange={e=>setId(e.target.value)}>{instances.map(i=><option key={i.id} value={i.id}>{i.name} · {i.engine}</option>)}</select></label>{selected&&<Button onClick={()=>onOpen(selected)}>管理此实例 / 停止工作台</Button>}{selected&&<PackagePanel recoveryOnly key={id} gateway={gateway} instance={selected} disabled={runtime.some(r=>r.id===id&&['running','installing','starting','stopping'].includes(r.status))}/>}</>:<p className="sub">创建实例后，可以在这里管理它的恢复点。</p>}
 <RemovedInstances gateway={gateway} onRestored={next=>{onRestored(next);void gateway.artifactCache(false).then(setCache).catch(e=>setError(errorText(e)));}}/>
 <section className="instance-runtime"><h2>引擎缓存</h2><p className="sub">实例、恢复点和已保存整合包引用的固定版本会保留。清理仅移除未被引用的受管引擎缓存；再次使用时需要重新下载。</p>{cache?.entries.map(entry=><div className="fact" key={entry.profileId}><span>{entry.profileId}</span><span>{entry.inUse?'被实例、恢复点或整合包引用':'可清理'} · {entry.installed?'完整':'不完整'}</span></div>)}{cache?.entries.length===0&&<p className="sub">暂无引擎缓存。</p>}<Button disabled={busy||gateway.mode!=='desktop'||!cache?.entries.some(e=>!e.inUse)} onClick={()=>void clean()}>{busy?'正在清理…':'清理未引用的缓存'}</Button>{!!cache?.removed.length&&<p role="status">已清理 {cache.removed.length} 个未引用组合。</p>}</section>
 </>;
}
