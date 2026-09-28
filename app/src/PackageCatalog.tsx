import Section from './Section';
import {useReportBusy} from './operationStatus';
import DetailDrawer from './DetailDrawer';
import {invoke} from '@tauri-apps/api/core';
import type {Draft} from './packEditing';
import {useEffect,useState} from 'react';
import {errorText,type Gateway,type Instance,type PackageCatalog as Catalog,type Recipe} from './domain';
import {Art,Button,IconButton,I} from './ui';
import PackEditor from './PackEditor';
import ExternalResources from './ExternalResources';
import InstalledExtensions from './InstalledExtensions';
import {recipeChanges} from './PackagePanel';

export default function PackageCatalog({gateway,instances,extensionsOnly=false,favoritesOnly=false,favorites,onFavorite,onNew,onOpen}:{gateway:Gateway;instances:Instance[];extensionsOnly?:boolean;favoritesOnly?:boolean;favorites:string[];onFavorite:(id:string)=>void;onNew:(recipe?:Recipe)=>void;onOpen:(instance:Instance)=>void}){
 const [editor,setEditor]=useState<{seed?:Recipe;draft?:Draft}>();
 const [drafts,setDrafts]=useState<Draft[]>([]),[notice,setNotice]=useState(''),[search,setSearch]=useState('');
 const [detail,setDetail]=useState<Recipe>();
 useEffect(()=>{if(detail){setNotice('');setError('');}},[detail]);
 useEffect(()=>{if(gateway.mode==='desktop'&&!extensionsOnly)void invoke<Draft[]>('pack_drafts').then(setDrafts).catch(e=>setError(errorText(e)));},[]);
 function edit(seed?:Recipe,draft?:Draft){if(seed&&sessionStorage.getItem('perch-pack-editing')){setDetail(undefined);setError('请先继续并保存或放弃当前编辑，再开始另一个组合。');return;}setNotice('');setError('');setDetail(undefined);setEditor({seed,draft});}
 async function exportPack(recipe:Recipe){setNotice('');setError('');try{const path=await invoke<string|null>('export_pack',{recipe});if(path)setNotice('整合包已导出。');}catch(e){setError(errorText(e));}}
 async function importPack(){setBusy(true);setError('');try{const recipe=await invoke<Recipe|null>('import_pack');if(recipe)edit(recipe);}catch(e){setError(errorText(e));}finally{setBusy(false);}}
 const [extensionView,setExtensionView]=useState<'discover'|'installed'|'sources'>('discover');
 const [existing,setExisting]=useState<Recipe>();
 const [loadedId,setLoadedId]=useState('');
 const [additive,setAdditive]=useState(false);
 function selectedPlan():Recipe|undefined {
  if(!selected)return undefined;
  if(!additive)return selected;
  if(!existing||loadedId!==id)return undefined;
  return {...existing,extensions:[...existing.extensions.filter(e=>!selected.extensions.some(add=>add.id===e.id)),...selected.extensions]};
 }
 const [catalog,setCatalog]=useState<Catalog>(),[selected,setSelected]=useState<Recipe>(),[id,setId]=useState(''),[offline,setOffline]=useState(false),[busy,setBusy]=useState(false),[error,setError]=useState(''),[done,setDone]=useState(false);
 useReportBusy(busy);
 const planned=selectedPlan();
 const extensionName=(id:string)=>catalog?.extensions.find(item=>item.id===id)?.name??id;
 useEffect(()=>{void gateway.packageCatalog().then(setCatalog).catch(e=>setError(errorText(e)));},[]);
 useEffect(()=>{setExisting(undefined);if(!id)return;let active=true;void gateway.instancePackage(id).then(result=>{if(active){setExisting(result.recipe);setLoadedId(id);}}).catch(e=>{if(active)setError(errorText(e));});return()=>{active=false;};},[id]);
 function choose(recipe:Recipe,add=false){setAdditive(add);setExisting(undefined);setSelected(recipe);setId('');setDone(false);setError('');}
 async function install(){if(!planned||!existing||loadedId!==id)return;setBusy(true);setError('');try{await gateway.installPackage(id,planned,offline,existing);setDone(true);setExisting(planned);}catch(e){setError(errorText(e));}finally{setBusy(false);}}

 const cards=[...(catalog?.packs??[]).map(recipe=>({key:'pack:'+recipe.profileId,recipe,draft:undefined as Draft|undefined})),...drafts.map(draft=>({key:'draft:'+draft.id,recipe:draft.recipe,draft}))].filter(item=>(!favoritesOnly||favorites.includes(item.key))&&(item.recipe.name+' '+(item.recipe.description??'')).toLowerCase().includes(search.toLowerCase()));
 function packCard(item:typeof cards[number]){const pack=item.recipe;return <article className="pack-card" key={item.key}><button className="pack-cover" aria-label={'查看 '+pack.name} onClick={()=>setDetail(pack)}><Art id={pack.cover||(pack.engine==='Pi'?'minimal':'developer')}/></button><div className="managed-pack-body"><h2>{pack.name}</h2><p className="resource-excerpt">{!item.draft?`${pack.engine==='Pi'?'Pi Web':'DSH'} 工作台 · 本地时间工具与代码审阅清单。`:pack.description||'独立工作台，搭配本地工具与审阅能力。'}</p><p className="sub">{pack.engine} · {pack.extensions.length} 项能力{item.draft?(item.draft.ready?' · 已保存':' · 草稿'):' · 精选组合'}</p><div className="pack-card-actions"><IconButton icon="heart" label={favorites.includes(item.key)?'取消收藏':'收藏'} aria-pressed={favorites.includes(item.key)} onClick={()=>onFavorite(item.key)}/><Button onClick={()=>item.draft&&!item.draft.ready?edit(pack,item.draft):setDetail(pack)}>{item.draft&&!item.draft.ready?'继续制作':'查看详情'}</Button>{(!item.draft||item.draft.ready)&&<Button primary onClick={()=>onNew(pack)}>使用组合</Button>}</div></div></article>;}
 if(editor&&catalog)return <PackEditor catalog={catalog} seed={editor.seed} savedDraft={editor.draft} onClose={()=>setEditor(undefined)} onSaved={draft=>{setDrafts(items=>[...items.filter(item=>item.id!==draft.id),draft]);setEditor(undefined);setNotice('组合已保存，可以直接使用或继续编辑。');}}/>;
 return <><div className="page-heading"><div><h1>{favoritesOnly?'我的收藏':extensionsOnly?'扩展市场':'发现整合包'}</h1><p>{extensionsOnly?'找到适合当前工作环境的能力。':'挑选现成组合，或在它的基础上搭配自己的工具。'}</p></div>{!extensionsOnly&&!favoritesOnly&&<div className="form-actions"><Button disabled={busy} onClick={()=>void importPack()}>导入整合包</Button><Button primary disabled={!catalog||busy} onClick={()=>edit({...catalog!.packs[0],name:'我的整合包'})}>新建组合</Button></div>}</div>
 {notice&&<p role="status">{notice}</p>}{error&&<p className="form-error" role="alert">{error}</p>}
 {!extensionsOnly&&!favoritesOnly&&sessionStorage.getItem('perch-pack-editing')&&<div className="resume-edit"><span>有尚未完成的组合编辑</span><Button onClick={()=>edit()}>继续编辑</Button></div>}
 {extensionsOnly&&<div className="resource-views" role="group" aria-label="扩展目录视图">{(['discover','installed','sources'] as const).map((view,index)=><Button key={view} aria-pressed={extensionView===view} onClick={()=>setExtensionView(view)}>{['发现','已安装','来源'][index]}</Button>)}</div>}
 {extensionsOnly&&extensionView==='installed'?<InstalledExtensions gateway={gateway} instances={instances} onOpen={onOpen}/>:extensionsOnly&&extensionView==='sources'?<ExternalResources gateway={gateway} instances={instances} onNew={onNew} sourcesOnly/>:<>
 {extensionsOnly&&gateway.mode==='desktop'&&<ExternalResources gateway={gateway} instances={instances} onNew={onNew}/>}
 {gateway.mode!=='desktop'?<p>请在桌面端读取真实目录。</p>:<div className="package-layout browse-only"><div>
 {!catalog&&!error&&<p role="status">正在读取目录…</p>}
 {!extensionsOnly&&<><label className="field catalog-search"><span>查找组合</span><input className="input" placeholder="按名称或用途查找" value={search} onChange={e=>setSearch(e.target.value)}/></label><div className="pack-grid">{cards.map(packCard)}</div>{!cards.length&&<p>{favoritesOnly?'还没有收藏的组合。':'没有匹配的组合。'}</p>}</>}
 {(extensionsOnly||favoritesOnly)&&catalog?.extensions.filter(e=>!favoritesOnly||favorites.includes('extension:'+e.id)).map(extension=><article className="row" key={extension.id}><div className="grow"><h2>{extension.name}</h2><p>{extension.description}</p><p className="sub">{extension.engine} · {extension.version} · {extension.source}</p><p className="sub">{extension.webSupport}。{extension.scripts}。</p><section className="builtin-compatibility"><h3>版本与兼容依据</h3><p className="sub">固定组合：{catalog.packs.find(p=>p.engine===extension.engine)?.profileId}。工具已通过该组合的实际加载与本地执行检查；Skill 已通过对应引擎的目录加载器检查。</p><p className="sub">{extension.id.endsWith('-local-clock')?'1.0.0 返回 UTC 时间；1.1.0 同时报告系统时区。依赖随固定引擎提供，无额外依赖。':'1.0.0：纯 Markdown 审阅步骤，无额外依赖。'}</p></section></div><div className="form-actions"><IconButton icon="heart" label={favorites.includes("extension:"+extension.id)?"取消收藏":"收藏"} aria-pressed={favorites.includes("extension:"+extension.id)} onClick={()=>onFavorite("extension:"+extension.id)}/><Button disabled={busy} onClick={()=>{const pack=catalog.packs.find(p=>p.engine===extension.engine);if(pack)choose({...pack,name:extension.name,extensions:pack.extensions.filter(e=>e.id===extension.id)},true);}}>查看安装计划</Button></div></article>)}
 </div></div>}
 </>}
 <DetailDrawer open={!!detail} title={detail?.name??'组合详情'} onClose={()=>setDetail(undefined)}>{detail&&<><div className="pack-cover"><Art id={detail.cover||(detail.engine==='Pi'?'minimal':'developer')}/></div><p>{detail.description||'独立的工作台与实例配置，模型连接可沿用已有配置。'}</p><h3>组合清单 · {detail.engine}</h3><ul className="composition-list">{detail.extensions.map(item=><li key={item.id}><I name="puzzle"/><span>{extensionName(item.id)}</span><small>{item.version}{!item.enabled?' · 已停用':''}</small></li>)}{detail.workshop?.map(item=><li key={item.id}>{item.id} · {item.kind}</li>)}</ul><div className="pack-detail-facts"><div><small>固定版本</small><span>{detail.profileId}</span></div><div><small>模型接口</small><span>{detail.modelProtocols.join(' / ')}</span></div></div><p className="sub">{detail.sourceNote||'来源信息随固定组合保留。'}</p><div className="form-actions"><Button onClick={()=>edit({...detail,name:detail.name+' · 自定义',origin:undefined})}>基于此组合自定义</Button><Button primary disabled={drafts.some(d=>d.recipe===detail&&!d.ready)} onClick={()=>onNew(detail)}>使用组合</Button></div>{drafts.find(d=>d.recipe===detail)&&<div className="form-actions">{drafts.find(d=>d.recipe===detail)?.lastReady&&!drafts.find(d=>d.recipe===detail)?.ready&&<Button onClick={()=>onNew(drafts.find(d=>d.recipe===detail)!.lastReady!)}>使用上次已校验修订</Button>}{drafts.find(d=>d.recipe===detail)?.lastReady&&!drafts.find(d=>d.recipe===detail)?.ready&&<Button onClick={()=>void exportPack(drafts.find(d=>d.recipe===detail)!.lastReady!)}>导出上次已校验修订</Button>}<Button onClick={()=>edit(detail,drafts.find(d=>d.recipe===detail))}>编辑此组合</Button><Button disabled={drafts.some(d=>d.recipe===detail&&!d.ready)} onClick={()=>void exportPack(detail)}>导出分享</Button></div>}{notice&&<p role="status">{notice}</p>}{error&&<p className="form-error" role="alert">{error}</p>}<button className="drawer-action-row" onClick={()=>choose(detail)}><I name="layers"/><span><strong>应用到已有实例</strong><small>选择目标，查看新增、保留与移除项后再确认。</small></span><I name="chevron"/></button></>}</DetailDrawer>
 <DetailDrawer open={!!selected} title="变更计划" closeDisabled={busy} onClose={()=>setSelected(undefined)}>{selected&&<>
 {error&&<p className="form-error" role="alert">{error}</p>}
 <h2>{additive?'添加扩展':'替换扩展组合'}</h2><p>{additive?'保留已有扩展，仅添加或更新所选扩展。':'使用所选整合包替换当前扩展集合，请确认移除项。'}</p><h3>{selected.name}</h3>
 <label className="field"><span>安装到实例</span><select className="input" value={id} disabled={busy} onChange={e=>{setId(e.target.value);setDone(false);}}><option value="">请选择同引擎实例</option>{instances.filter(i=>i.engine===selected.engine).map(i=><option key={i.id} value={i.id}>{i.name}</option>)}</select></label>
 <div className="package-change-summary"><h3>变更摘要</h3><p>所选组合包含 {selected.extensions.length} 项能力。</p>{id&&loadedId!==id&&<p role="status">正在读取实例组合…</p>}{existing&&planned&&loadedId===id&&<>
 <p>新增 {planned.extensions.filter(e=>!existing.extensions.some(old=>old.id===e.id)).length} 项 · 移除 {existing.extensions.filter(e=>!planned.extensions.some(next=>next.id===e.id)).length} 项</p>
 {existing.extensions.some(e=>!planned.extensions.some(next=>next.id===e.id))&&<p className="notice">将移除：{existing.extensions.filter(e=>!planned.extensions.some(next=>next.id===e.id)).map(e=>extensionName(e.id)).join('、')}</p>}
 </>}</div>
 <label><input type="checkbox" checked={offline} disabled={busy} onChange={e=>setOffline(e.target.checked)}/> 仅使用离线缓存</label>
 <div className="resource-confirm-actions">{done?<Button primary onClick={()=>{const instance=instances.find(i=>i.id===id);if(instance)onOpen(instance);}}>打开实例</Button>:<><Button onClick={()=>busy?void gateway.cancelEngine(id).catch(e=>setError(errorText(e))):setSelected(undefined)}>{busy?'取消安装':'取消'}</Button>{!id?<Button primary onClick={()=>onNew(selected)}>用此组合新建实例</Button>:<Button primary disabled={busy||!existing||!planned||loadedId!==id} onClick={()=>void install()}>{busy?'正在安装…':additive?'添加到实例':'应用整合包'}</Button>}</>}</div>
 {done&&<p role="status">已安装。进入实例后可启动工作台。</p>}
 <Section title="完整组合与变更明细" description="查看能力名称、精确版本及变更内容"><ul className="composition-list">{(planned??selected).extensions.map(e=><li key={e.id}><I name="puzzle"/><span>{extensionName(e.id)}</span><small>{e.version}</small></li>)}</ul>{existing&&planned&&loadedId===id&&<div>{recipeChanges(existing,planned,extensionName).length?recipeChanges(existing,planned,extensionName).map(line=><p className="sub" key={line}>{line}</p>):<p className="sub">组合未变化，将检查固定引擎安装。</p>}</div>}</Section>
 <Section title="版本与安装说明"><p className="path-text">{selected.profileId}</p><p className="sub">固定引擎与依赖；已有缓存时复用，不追踪最新版。安装前自动备份实例；依赖安装失败时保留原组合。扩展与依赖安装脚本禁用。</p><ul>{(planned??selected).extensions.map(e=><li key={e.id}>{extensionName(e.id)} · {e.id} · {e.version}</li>)}</ul></Section>
 </>}</DetailDrawer>
 </>;
}
