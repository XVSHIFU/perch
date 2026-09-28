import {useReportBusy} from './operationStatus';
import {useState} from 'react';
import {type Gateway,type Snapshot,errorText} from './domain';
import ModelConnectionEditor,{type ConnectionPreset} from './ModelConnectionEditor';
import {Button,I} from './ui';
export default function ConnectionsPage({data,gateway,onSaved}:{data:Snapshot;gateway:Gateway;onSaved:(s:Snapshot)=>void}){
 const [editing,setEditing]=useState<string|null>(null),[adding,setAdding]=useState(false),[preset,setPreset]=useState<ConnectionPreset|undefined>();
 const [busy,setBusy]=useState(false),[error,setError]=useState(''),[deleting,setDeleting]=useState<string|null>(null);
 useReportBusy(busy);
 function saved(s:Snapshot){setEditing(null);setAdding(false);onSaved(s);}
 async function remove(id:string){setBusy(true);setError('');try{saved(await gateway.deleteConnection(id,crypto.randomUUID()));setDeleting(null);}catch(e){setError(errorText(e));}finally{setBusy(false);}}
 function add(p?:ConnectionPreset){setPreset(p);setAdding(true);setEditing(null);setError('');}
 return <section className="connections-section"><header className="settings-section-heading"><h2>模型连接</h2><p>配置一次，让实例共用；也可以为不同项目选择独立连接。</p></header>{error&&<p className="form-error" role="alert">{error}</p>}
 <div className="provider-list">{data.connections.map(c=>{const used=data.instances.filter(i=>i.connectionId===c.id).length;return <article className={'provider-item '+(editing===c.id?'expanded':'')} key={c.id}><div className="provider-heading"><span className={'connection-indicator '+(c.hasKey?'configured':'')} title={c.hasKey?'密钥已保存，未验证模型调用':'未配置密钥'}/><div className="grow"><h3>{c.name}</h3>{editing!==c.id&&<p className="sub">{c.hasKey?'密钥已保存 · 尚未验证模型调用':'尚未配置密钥'} · 默认模型：{c.defaultModel} · {c.models?.length||1} 个模型 · {used} 个实例使用</p>}</div><button className="btn" onClick={()=>{setEditing(editing===c.id?null:c.id);setAdding(false);setDeleting(null);}}>{editing===c.id?'收起':'编辑'}</button><button className="text-btn danger" disabled={busy||used>0} title={used?'请先为使用此连接的实例更换连接':'删除连接'} onClick={()=>setDeleting(c.id)}>删除</button></div>{deleting===c.id&&<div className="inline-confirm"><p>删除“{c.name}”及本机保存的密钥？</p><Button onClick={()=>setDeleting(null)}>取消</Button><Button disabled={busy} onClick={()=>void remove(c.id)}>确认删除</Button></div>}{editing===c.id&&<ModelConnectionEditor connection={c} gateway={gateway} onSaved={saved} onCancel={()=>setEditing(null)}/>}</article>;})}</div>
 {!data.connections.length&&!adding&&<div className="connections-empty"><I name="globe"/><h3>添加你的第一个模型连接</h3><p>选择服务商，或添加兼容 API 的自定义连接。</p></div>}
 {adding?<article className="provider-item expanded"><div className="provider-heading"><h3 className="grow">{preset?'添加 '+preset.name:'添加自定义连接'}</h3><button className="icon-btn" title="取消添加" onClick={()=>setAdding(false)}><I name="close"/></button></div><ModelConnectionEditor key={preset?.name??'custom'} preset={preset} gateway={gateway} onSaved={saved} onCancel={()=>setAdding(false)}/></article>:<div className="provider-add"><Button icon="plus" onClick={()=>add({name:'DeepSeek 官方',provider:'DeepSeek',protocol:'deepseek',baseUrl:'https://api.deepseek.com'})}>添加 DeepSeek</Button><Button icon="plus" onClick={()=>add()}>添加自定义连接</Button></div>}
 <p className="settings-footnote">模型目录获取不会发送聊天消息。部分服务商不提供目录接口，可手动添加；获取成功也不代表每个模型都已开通权限。</p></section>;
}
