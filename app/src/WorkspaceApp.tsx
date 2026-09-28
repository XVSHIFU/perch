import {useOperationBusy} from './operationStatus';
import DetailDrawer from './DetailDrawer';
import LaunchPage from './LaunchPage';
import ActivityPage from './ActivityPage';
import {useCallback,useEffect,useRef,useState} from 'react';
import {isNative,desktop} from './native';
import {DesktopGateway,DemoGateway} from './gateway';
import {errorText,runtimeLabels,type Instance,type Snapshot,type Recipe} from './domain';
import AppShell from './AppShell';
import InstanceRuntime from './InstanceRuntime';
import DeleteInstance from './DeleteInstance';
import PackagePanel from './PackagePanel';
import PackageCatalog from './PackageCatalog';
import RecoveryPage from './RecoveryPage';
import VersionCatalog from './VersionCatalog';
import ConnectionsPage from './ConnectionsPage';
import SettingsFrame from './SettingsFrame';
import InstanceForm,{type InstanceDraft} from './InstanceForm';
import {Button,I,Logo,Mountain} from './ui';


const gateway=isNative?new DesktopGateway():new DemoGateway();
function readPrefs(){try{return JSON.parse(localStorage.getItem('perch-ui-preferences-v1')??'{}');}catch{return {};}}
export default function WorkspaceApp(){
 const [data,setData]=useState<Snapshot|null>(null),[error,setError]=useState(''),[loading,setLoading]=useState(true);
 const [instanceFormBusy,setInstanceFormBusy]=useState(false);
 const [page,setPage]=useState('launch'),[selected,setSelected]=useState<Instance|undefined>(),[editingInstance,setEditingInstance]=useState(false);
 const [instanceTab,setInstanceTab]=useState('environment');
 const [instanceOpen,setInstanceOpen]=useState(false);
 const [creationRecipe,setCreationRecipe]=useState<Recipe>();
 const [instanceDraft,setInstanceDraft]=useState<InstanceDraft>();
 const [returnToInstance,setReturnToInstance]=useState(false);
 const connectionIdsBefore=useRef<string[]>([]);
 const [recentId,setRecentId]=useState<string>(readPrefs().recentId??'');
 const [query,setQuery]=useState(''),[notice,setNotice]=useState('');
 const operationBusy=useOperationBusy();
 const [dark,setDark]=useState(!!readPrefs().dark),[closeMode,setCloseMode]=useState(readPrefs().closeMode==='tray'?'tray':'ask');
 const [favorites,setFavorites]=useState<string[]>(()=>{const value=readPrefs().favorites;return Array.isArray(value)?value.filter((id):id is string=>typeof id==='string'):[];});
 function toggleFavorite(id:string){setFavorites(items=>items.includes(id)?items.filter(item=>item!==id):[...items,id]);}
 const [maximized,setMaximized]=useState(false),[closeOpen,setCloseOpen]=useState(false);
 const closeDialog=useRef<HTMLDialogElement>(null);
 const refresh=useCallback(async()=>{setLoading(true);setError('');try{setData(await gateway.snapshot());}catch(e){setError(errorText(e));}finally{setLoading(false);}},[]);
 useEffect(()=>{void refresh();},[refresh]);
 useEffect(()=>{if(!isNative)return;let disposed=false,running=false;const timer=setInterval(async()=>{if(running)return;running=true;try{const next=await gateway.snapshot();if(!disposed)setData(next);}catch{}finally{running=false;}},1500);return()=>{disposed=true;clearInterval(timer);};},[]);
 useEffect(()=>{document.body.classList.toggle('dark',dark);try{localStorage.setItem('perch-ui-preferences-v1',JSON.stringify({dark,closeMode,favorites,recentId}));}catch{}},[dark,closeMode,favorites,recentId]);
 const windowAction=useCallback(async(action:string)=>{
   if(!isNative){setNotice('窗口按钮仅在 Windows 桌面端生效');return;}
   try{if(action==='close'){if(closeMode==='tray')await desktop.window('hide');else setCloseOpen(true);}else await desktop.window(action);}catch(e){setError(errorText(e));}
 },[closeMode]);
 useEffect(()=>{if(!isNative)return;let disposed=false;const removers:(()=>void)[]=[];const bind=async()=>{for(const promise of [desktop.onClose(()=>void windowAction('close')),desktop.onRunning(()=>{setEditingInstance(false);setPage('library');}),desktop.onStopped(()=>void refresh()),desktop.onResized(()=>void desktop.isMaximized().then(setMaximized))]){const remove=await promise;if(disposed)remove();else removers.push(remove);}setMaximized(await desktop.isMaximized());};void bind().catch(()=>setError('窗口状态读取失败，请重新打开应用'));return()=>{disposed=true;removers.forEach(fn=>fn());};},[windowAction]);
 useEffect(()=>{if(closeOpen)closeDialog.current?.showModal();else closeDialog.current?.close();},[closeOpen]);
 function go(next:string){setInstanceOpen(false);setPage(next);setEditingInstance(false);setNotice('');setQuery('');}
 function saved(next:Snapshot,savedInstanceId?:string){if(editingInstance){setQuery('');setInstanceDraft(undefined);}if(returnToInstance){const added=next.connections.find(c=>!connectionIdsBefore.current.includes(c.id));if(added)setInstanceDraft(d=>d?{...d,connectionId:added.id}:d);}setData(previous=>({...next,runtime:previous?.runtime??next.runtime}));setEditingInstance(false);setNotice('已保存');if(selected)setSelected(next.instances.find(i=>i.id===selected.id));const destination=savedInstanceId?next.instances.find(i=>i.id===savedInstanceId):undefined;if(destination){setReturnToInstance(false);view(destination);}}
 function newInstance(recipe?:Recipe){setInstanceOpen(false);setCreationRecipe(recipe);setInstanceDraft(undefined);setReturnToInstance(false);setQuery('');setNotice('');setSelected(undefined);setEditingInstance(true);setPage('library');}
 function view(instance:Instance){setNotice('');setInstanceTab('environment');setCreationRecipe(undefined);setRecentId(instance.id);setSelected(instance);setEditingInstance(!!instanceDraft&&selected?.id===instance.id);if(selected?.id!==instance.id)setInstanceDraft(undefined);setInstanceOpen(true);}
 function instanceConnections(instance:Instance){if(data?.connections.some(connection=>connection.id===instance.connectionId)){go('connections');return;}setCreationRecipe(undefined);setInstanceDraft(undefined);setReturnToInstance(false);setSelected(instance);setInstanceOpen(true);setEditingInstance(true);setNotice('选择已有连接即可复用；没有合适连接时，可在下方管理模型连接，返回后继续保存。');}
 const list=data?.instances.filter(i=>(i.name+' '+i.projectPath).toLowerCase().includes(query.toLowerCase()))??[];
 function instances(){return <><div className="section-title"><h2>我的工作环境 <span className="small muted">{data?.instances.length??0}</span></h2><label className="search-box"><I name="search"/><input aria-label="查找实例" placeholder="查找实例" value={query} onChange={e=>setQuery(e.target.value)}/></label></div>{!list.length?<div className="empty"><I name="box"/><h3>{query?'没有匹配的实例':'创建你的第一个工作环境'}</h3><p>{query?'试试其他名称或项目路径。':'选择项目和引擎，已有模型连接可以直接沿用。'}</p>{!query&&<Button primary onClick={()=>newInstance()}>新建实例</Button>}</div>:list.map(i=><button className="instance-row" key={i.id} onClick={()=>view(i)}><Logo engine={i.engine}/><span className="grow"><strong>{i.name}</strong><span className="sub path-text">{i.projectPath}</span></span><span className={'pill '+(!i.projectExists?'warn':'')}>{i.projectExists?runtimeLabels[data?.runtime?.find(r=>r.id===i.id)?.status??'not_installed']:'项目目录不存在'}</span><I name="chevron"/></button>)}</>;}
 function content(detail=false){
  if(loading&&!data)return <div className="empty" role="status">正在读取本机工作空间…</div>;
  if(!data)return <div className="empty"><h2>工作空间暂时无法读取</h2><p>没有切换到演示数据。请检查目录权限后重试。</p><Button onClick={()=>void refresh()}>重新读取</Button></div>;
  if(editingInstance&&(detail||!selected))return <InstanceForm key={selected?.id??'new'} instance={selected} draft={instanceDraft} recipe={creationRecipe} data={data} gateway={gateway} onSaved={saved} onDraftChange={setInstanceDraft} onBusyChange={setInstanceFormBusy} onCancel={()=>{setInstanceDraft(undefined);setEditingInstance(false);}} onConnections={draft=>{setInstanceDraft(draft);connectionIdsBefore.current=data.connections.map(c=>c.id);setReturnToInstance(true);go('connections');}}/>;
  if(detail&&selected){const current=data.instances.find(i=>i.id===selected.id);if(!current)return <div className="empty">实例已不存在，请返回列表。</div>;const c=data.connections.find(c=>c.id===current.connectionId);return <><div className="page-heading"><div><p>{current.engine} · 独立实例</p></div><Button disabled={data.runtime?.some(r=>r.id===current.id&&['running','installing','starting','stopping'].includes(r.status))} onClick={()=>{setCreationRecipe(undefined);setInstanceDraft(undefined);setSelected(current);setEditingInstance(true);}}>编辑实例</Button></div><InstanceRuntime instance={current} connection={c} runtime={data.runtime?.find(r=>r.id===current.id)} gateway={gateway} onRefresh={refresh} onConnections={()=>instanceConnections(current)}/><div className="resource-views" role="group" aria-label="实例管理分区">{[['environment','环境'],['extensions','扩展与版本'],['recovery','恢复点'],['activity','活动']].map(([id,label])=><Button key={id} aria-pressed={instanceTab===id} onClick={()=>setInstanceTab(id)}>{label}</Button>)}</div>{instanceTab==='environment'&&<><div className="detail-summary"><Logo engine={current.engine}/><div><h2>独立工作环境</h2><p>实例配置与会话独立保存，模型连接可共用。</p></div></div>{[['项目目录',current.projectPath],['模型连接',c?.name??'尚未绑定'],['默认模型',c?.defaultModel??'尚未配置'],['固定组合',current.profileId],['配置修订',String(current.revision)]].map(([label,value])=><div className="fact" key={label}><span>{label}</span><span className="path-text">{value}</span></div>)}{!current.projectExists&&<p className="form-error" role="alert">项目目录已不存在，请编辑实例重新选择。实例配置仍保留。</p>}<DeleteInstance instance={current} gateway={gateway} dataRoot={data.dataRoot} disabled={operationBusy||data.runtime?.some(r=>r.id===current.id&&['running','installing','starting','stopping'].includes(r.status))} onDeleted={next=>{setData(previous=>({...next,runtime:previous?.runtime?.filter(r=>r.id!==current.id)??[]}));setInstanceOpen(false);setSelected(undefined);setInstanceDraft(undefined);if(recentId===current.id)setRecentId('');setNotice('实例已删除，项目、连接与实例数据均已保留。');}}/></>}{instanceTab==='extensions'&&<PackagePanel gateway={gateway} instance={current} disabled={data.runtime?.some(r=>r.id===current.id&&['running','installing','starting','stopping'].includes(r.status))}/>}{instanceTab==='recovery'&&<PackagePanel recoveryOnly gateway={gateway} instance={current} disabled={data.runtime?.some(r=>r.id===current.id&&['running','installing','starting','stopping'].includes(r.status))}/>}{instanceTab==='activity'&&<ActivityPage data={{...data,tasks:data.tasks.filter(task=>task.entityId===current.id)}} loading={loading} onRefresh={()=>void refresh()} onInstance={view} onConnections={()=>go('connections')}/>}</>;}
  if(page==='connections')return <SettingsFrame section={page} onSection={go}>{returnToInstance&&<Button onClick={()=>{setReturnToInstance(false);setPage('library');setInstanceOpen(!!selected);setEditingInstance(true);setNotice('');}}>返回{selected?'编辑':'创建'}实例 · 草稿已保留</Button>}<ConnectionsPage data={data} gateway={gateway} onSaved={saved}/></SettingsFrame>;
  if(!detail&&page==='launch')return <LaunchPage data={data} gateway={gateway} recentId={recentId} onSelect={setRecentId} onOpen={view} onNew={()=>newInstance()} onRefresh={refresh} onConnections={instanceConnections}/>;
  if(!detail&&page==='library')return <><div className="page-heading"><div><h1>我的实例</h1><p>实例保存独立配置，启动后在工作台中开展工作。</p></div><Button icon="plus" onClick={()=>newInstance()}>新建实例</Button></div>{instances()}</>;
  if(page==='activity')return <ActivityPage data={data} loading={loading} onRefresh={()=>void refresh()} onInstance={view} onConnections={()=>go('connections')}/>;
  if(page==='settings')return <SettingsFrame section={page} onSection={go}><header className="general-settings-heading"><h2>通用设置</h2><p>调整外观与窗口行为。</p></header><div className="settings-group"><div className="setting-row"><div><h3>深色外观</h3><p>在浅色与深色之间切换。</p></div><button className={'preference-switch '+(dark?'on':'')} role="switch" aria-checked={dark} aria-label="深色外观" onClick={()=>setDark(!dark)}><span/></button></div><div className="setting-row"><div><h3>关闭窗口</h3><p>收起到托盘，或在关闭时询问。</p></div><select aria-label="关闭窗口行为" value={closeMode} onChange={e=>setCloseMode(e.target.value)}><option value="ask">每次询问</option><option value="tray">收起到托盘</option></select></div></div><h3 className="settings-subheading">数据与诊断</h3><div className="setting-row"><div><h3>诊断报告</h3><p>导出版本、运行状态与错误标记，不包含密钥、项目路径或对话内容。</p></div><Button disabled={!isNative} onClick={()=>void desktop.exportDiagnostics().then(path=>{if(path)setNotice(`诊断报告已保存：${path}`);}).catch(e=>setError(errorText(e)))}>导出诊断报告</Button></div><div className="settings-group"><div className="setting-row"><div className="grow"><h3>本机数据位置</h3><p className="path-text selectable">{data.dataRoot}</p></div></div><div className="setting-row"><div><h3>演示样机</h3><p>单独体验示例整合包、插件与演示服务。</p></div><a className="btn" href="?demo=legacy">打开演示</a></div></div></SettingsFrame>;
  if(page==='discover'||page==='extensions'||page==='favorites')return <PackageCatalog key={page} favorites={favorites} onFavorite={toggleFavorite} favoritesOnly={page==='favorites'} gateway={gateway} instances={data.instances} extensionsOnly={page==='extensions'} onNew={recipe=>newInstance(recipe)} onOpen={view}/>;
  if(page==='snapshots')return <RecoveryPage gateway={gateway} instances={data.instances} runtime={data.runtime} onOpen={view} onRestored={next=>setData(previous=>({...next,runtime:previous?.runtime??[]}))}/>;
  return <div className="empty"><I name="heart"/><h3>页面不存在</h3><p>请从侧栏选择功能。</p></div>;
 }
 return <><AppShell selectedId={instanceOpen?selected?.id:undefined} page={page} instances={data?.instances??[]} onPage={go} onInstance={view} onNew={()=>newInstance()} mode={gateway.mode} runtime={data?.runtime??[]} busy={instanceFormBusy||operationBusy} maximized={maximized} windowAction={a=>void windowAction(a)}>{error&&<div className="form-error" role="alert">{error} <button className="btn sm" onClick={()=>void refresh()}>重新读取</button></div>}{notice&&<p className="save-notice" role="status">{notice}</p>}{content()}<DetailDrawer key={selected?.id??'instance'} open={instanceOpen} title={editingInstance?'编辑实例':selected?.name??'实例详情'} closeDisabled={instanceFormBusy} onClose={()=>setInstanceOpen(false)}>{instanceOpen&&content(true)}</DetailDrawer></AppShell><dialog aria-labelledby="close-heading" className="managed-close" ref={closeDialog} onCancel={()=>setCloseOpen(false)}><h2 id="close-heading">关闭栖点</h2><p>收起到托盘会保持工作台运行；退出将停止全部受管实例及子进程。</p><div className="form-actions"><Button onClick={()=>setCloseOpen(false)}>取消</Button><Button onClick={()=>{setCloseOpen(false);void windowAction('hide');}}>收起到托盘</Button><Button primary onClick={()=>void windowAction('quit')}>退出</Button></div></dialog></>;
}
