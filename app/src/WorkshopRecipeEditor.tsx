import Section from './Section';
import {useEffect,useState} from 'react';
import {invoke} from '@tauri-apps/api/core';
import {errorText,type Recipe,type WorkshopRef} from './domain';
import {Button} from './ui';
import WorkshopAssetPanel from './WorkshopAssetPanel';

const labels={skins:'主题',pets:'宠物',presets:'预设'};
export function workshopConflicts(recipe:Recipe):string[]{
 const hosts={skins:'@linxin666/dsh-client-ui-skin-center',pets:'@linxin666/dsh-pet',presets:'@linxin666/dsh-client-ui-preset-center'};
 return (recipe.workshop??[]).flatMap(item=>recipe.engine!=='DSH'?[`${item.id} 需要 DSH 引擎`]:
  recipe.extensions.some(extension=>extension.enabled&&[hosts[item.kind],'@linxin666/dsh-web-all'].some(host=>extension.id===`dsh-npm:${host}`))?[]:[`${item.id} 缺少已启用的宿主 ${hosts[item.kind]}`]);
}
type Entry={id:string;name:string;version:string;kinds:string[]};
export default function WorkshopRecipeEditor({recipe,onChange,busy,onBusyChange}:{recipe:Recipe;onChange:(recipe:Recipe)=>void;busy:boolean;onBusyChange:(busy:boolean)=>void}){
 const [entries,setEntries]=useState<Entry[]>([]),[query,setQuery]=useState(''),[selected,setSelected]=useState(''),[error,setError]=useState(''),[loaded,setLoaded]=useState(false);
 useEffect(()=>{let active=true;void invoke<{entries:Entry[]}>('workshop_catalog',{refresh:false}).then(value=>{if(active){setEntries(value.entries.filter(item=>item.id.startsWith('workshop:')&&item.kinds.some(kind=>kind in labels)));setLoaded(true);}}).catch(e=>{if(active)setError(errorText(e));});return()=>{active=false;};},[]);
 function add(resource:WorkshopRef){
  if((recipe.workshop??[]).some(item=>`${item.kind}/${item.id}`.toLowerCase()===`${resource.kind}/${resource.id}`.toLowerCase()))throw new Error('组合中已有同名资源。请先移除旧选择，再加入所需版本；已有实例不会改变。');
  onChange({...recipe,workshop:[...(recipe.workshop??[]),resource]});setSelected('');setError('');onBusyChange(false);
 }
 const matches=entries.filter(item=>`${item.name} ${item.id}`.toLowerCase().includes(query.trim().toLowerCase()));
 const conflicts=workshopConflicts(recipe);
 return <section aria-label="组合工坊资源"><h3>工坊主题、宠物与预设</h3><p>加入组合前下载并校验固定版本。资源所需宿主仍需在版本组合中安装并启用。</p>
  {(recipe.workshop??[]).map(item=><div className="fact" key={`${item.kind}/${item.id}`}><div className="grow"><strong>{item.id} · {labels[item.kind]}</strong><p>{item.version} · {item.repository} · {item.commit.slice(0,12)}</p></div><Button disabled={busy} aria-label={`从组合移除 ${item.id}`} onClick={()=>onChange({...recipe,workshop:recipe.workshop?.filter(resource=>resource!==item)})}>移除</Button></div>)}
  {conflicts.length>0&&<div role="alert"><p>这些选择已保留，可以先保存草稿。校验前请处理：</p><ul>{conflicts.map(message=><li key={message}>{message}</li>)}</ul></div>}
  {error&&<p className="form-error" role="alert">{error}</p>}
  {recipe.engine==='DSH'&&<Section collapsible={false} title={<>从工坊目录添加</>}><label className="field"><span>查找资源</span><input className="input" value={query} onChange={e=>setQuery(e.target.value)} placeholder="资源名称或标识"/></label>
   {loaded&&!entries.length&&<p>还没有工坊目录缓存。请到扩展市场的来源管理刷新工坊，再返回此页继续编辑。</p>}
   {loaded&&entries.length>0&&!matches.length&&<p>没有匹配资源，请换一个关键词。</p>}
   {matches.slice(0,20).map(item=><div className="fact" key={item.id}><span>{item.name} · {item.version}</span><Button disabled={busy||!!selected} onClick={()=>{setSelected(item.id);setError('');}}>预览并加入</Button></div>)}
   {matches.length>20&&<p>显示前 20 项，请输入关键词缩小范围。</p>}
   {selected&&<WorkshopAssetPanel key={selected} resourceId={selected} instances={[]} onClose={()=>setSelected('')} onAdd={add} onBusyChange={onBusyChange}/>}
  </Section>}
 </section>;
}
