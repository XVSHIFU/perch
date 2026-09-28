import {useReportBusy} from './operationStatus';
import Section from './Section';
import {useRef,useState,type FormEvent} from 'react';
import {errorText,type ConnectionInput,type Gateway,type ModelConnection,type ModelSpec,type Protocol,type Snapshot} from './domain';
import {Button,I} from './ui';
type Row=ModelSpec & {rowKey:string};
const row=(model:ModelSpec):Row=>({...model,rowKey:crypto.randomUUID()});
const emptyModel=():Row=>row({id:'',name:'',contextWindow:null,maxTokens:null});
export type ConnectionPreset={name:string;provider:string;protocol:Protocol;baseUrl:string};
export default function ModelConnectionEditor({connection,preset,gateway,onSaved,onCancel}:{connection?:ModelConnection;preset?:ConnectionPreset;gateway:Gateway;onSaved:(s:Snapshot)=>void;onCancel:()=>void}){
 const [name,setName]=useState(connection?.name??preset?.name??'');
 const [provider,setProvider]=useState(connection?.provider??preset?.provider??'');
 const [protocol,setProtocol]=useState<Protocol>(connection?.protocol??preset?.protocol??'openai-chat');
 const [baseUrl,setBaseUrl]=useState(connection?.baseUrl??preset?.baseUrl??'');
 const original=useRef<ModelSpec[]>(connection?.models?.length?connection.models:connection?.defaultModel?[{id:connection.defaultModel,name:connection.defaultModel,contextWindow:null,maxTokens:null}]:[]);
 const [models,setModels]=useState<Row[]>(()=>original.current.map(row));
 const [defaultModel,setDefaultModel]=useState(connection?.defaultModel??'');
 const [key,setKey]=useState(''),[busy,setBusy]=useState(false),[fetching,setFetching]=useState(false),[error,setError]=useState(''),[message,setMessage]=useState(''),[filter,setFilter]=useState('');
 const [defaultFilter,setDefaultFilter]=useState(''),[catalogOpen,setCatalogOpen]=useState(!connection);
 useReportBusy(busy);
 const last=useRef({signature:'',operationId:''});const working=busy||fetching;
 function update(rowKey:string,patch:Partial<ModelSpec>){const current=models.find(m=>m.rowKey===rowKey);if(patch.id!==undefined&&current?.id===defaultModel)setDefaultModel(patch.id);setModels(items=>items.map(m=>m.rowKey===rowKey?{...m,...patch}:m));}
 function restore(){setModels(original.current.map(row));setDefaultModel(connection?.defaultModel??'');setMessage('已恢复打开编辑时的模型目录，保存后生效');setError('');}
 async function fetchModels(){
  if(working)return;setFetching(true);setError('');setMessage('');
  try{const result=await gateway.fetchModels({connectionId:connection?.id??null,baseUrl,protocol,apiKey:key||null});
   if(!result.models.length){setMessage('服务商返回的模型目录为空，已有模型保留。也可以手动添加。');return;}
   const existing=new Set(models.map(m=>m.id.trim()));const missing=result.models.filter(m=>!existing.has(m.id));const additions=missing.slice(0,Math.max(0,1000-models.length));
   setModels(items=>[...items,...additions.map(row)]);if(!defaultModel)setDefaultModel(models.find(m=>m.id.trim())?.id??result.models[0].id);
   setMessage(`获取到 ${result.models.length} 个模型，新增 ${additions.length} 个；已保留手动配置。${missing.length>additions.length?'目录已达到 1000 个模型上限。':result.truncated?'服务端目录未完整返回，可继续手动补充。':''}保存后生效。`);
  }catch(e){setError(errorText(e));}finally{setFetching(false);}
 }
 async function submit(event:FormEvent){
  event.preventDefault();if(working)return;setError('');setMessage('');
  const clean=models.map(({rowKey:_,...m})=>({...m,id:m.id.trim(),name:m.name.trim()||m.id.trim()}));
  if(!clean.length||clean.some(m=>!m.id)){setCatalogOpen(true);setError('请至少添加一个模型，并填写每个模型的 ID');return;}
  if(new Set(clean.map(m=>m.id)).size!==clean.length){setCatalogOpen(true);setError('模型 ID 重复，请合并或删除重复项');return;}
  if(!clean.some(m=>m.id===defaultModel.trim())){setError('请选择目录中的默认模型');return;}
  if(clean.some(m=>(m.contextWindow!==null&&(!Number.isInteger(m.contextWindow)||m.contextWindow<=0))||(m.maxTokens!==null&&(!Number.isInteger(m.maxTokens)||m.maxTokens<=0))||(m.contextWindow!==null&&m.maxTokens!==null&&m.maxTokens>m.contextWindow))){setCatalogOpen(true);setError('长度需为正整数，最大输出不能超过上下文窗口');return;}
  setBusy(true);const fields={id:connection?.id??null,expectedRevision:connection?.revision??null,name,provider:provider.trim()||name,protocol,baseUrl,defaultModel:defaultModel.trim(),models:clean,apiKey:key||null};
  const signature=JSON.stringify(fields);if(signature!==last.current.signature)last.current={signature,operationId:crypto.randomUUID()};
  try{const data=await gateway.saveConnection({...fields,operationId:last.current.operationId} as ConnectionInput);setKey('');last.current={signature:'',operationId:''};onSaved(data);}catch(e){setError(errorText(e));}finally{setBusy(false);}
 }
 return <form className="connection-editor" onSubmit={submit}><fieldset disabled={working}>
  <section aria-label="选择默认模型"><h3>默认模型</h3><p className="sub">使用此连接的实例下次启动时采用所选模型。</p>{models.length>8&&<label className="search-box"><I name="search"/><input aria-label="搜索默认模型" value={defaultFilter} onChange={e=>setDefaultFilter(e.target.value)} placeholder="按模型 ID 或名称搜索"/></label>}<label className="field"><span>当前选择</span><select className="input" value={defaultModel} onChange={e=>setDefaultModel(e.target.value)} required><option value="">选择默认模型</option>{models.filter(m=>m.id.trim()&&(m.id===defaultModel||(m.id+' '+m.name).toLowerCase().includes(defaultFilter.trim().toLowerCase()))).map(m=><option value={m.id} key={m.rowKey}>{m.name||m.id}{m.id===defaultModel?' · 当前选择':''}</option>)}</select></label>{defaultFilter.trim()&&!models.some(m=>(m.id+' '+m.name).toLowerCase().includes(defaultFilter.trim().toLowerCase()))&&<p role="status">没有匹配模型，当前选择保留。可清空搜索，或展开模型目录添加。</p>}</section>
  <label className="field"><span>API Key {connection?'（留空保留现有密钥）':''}</span><input type="password" autoComplete="new-password" spellCheck={false} className="input" disabled={gateway.mode==='demo'} required={!connection&&gateway.mode==='desktop'} maxLength={2400} value={key} onChange={e=>setKey(e.target.value)} placeholder={gateway.mode==='demo'?'演示不接收真实密钥':connection?'已配置，输入新 Key 可替换':'输入服务商提供的 Key'}/></label>
  <div className="connection-fields"><label className="field"><span>显示名称</span><input required maxLength={60} className="input" value={name} onChange={e=>setName(e.target.value)} placeholder="例如：我的中转站"/></label><label className="field"><span>服务商 <small>可选</small></span><input maxLength={60} className="input" value={provider} onChange={e=>setProvider(e.target.value)} placeholder="默认与显示名称相同"/></label><label className="field full"><span>API 地址</span><input type="url" required className="input" value={baseUrl} onChange={e=>setBaseUrl(e.target.value)} placeholder="https://api.example.com/v1"/></label><label className="field full"><span>API 协议</span><select className="input" value={protocol} onChange={e=>setProtocol(e.target.value as Protocol)}><option value="deepseek">DeepSeek</option><option value="openai-chat">OpenAI Chat Completions（openai-completions）</option><option value="openai-responses">OpenAI Responses</option><option value="anthropic">Anthropic Messages</option></select></label></div>
  <Section description="获取服务商模型，或手动维护模型 ID 与显示名称。" open={catalogOpen} onOpenChange={setCatalogOpen} title={<>编辑模型目录 · {models.length} 个模型</>}><section className="model-directory" aria-label="模型目录"><div className="model-directory-heading"><div><h3>模型目录 <span className="small muted">{models.length}</span></h3><p className="sub">自动获取，或手动添加服务商提供的模型 ID。</p></div><div className="catalog-actions"><button type="button" className="text-btn" onClick={restore} disabled={!models.length}>{connection?'恢复已保存目录':'清空目录'}</button><button type="button" className="btn" onClick={()=>void fetchModels()} disabled={gateway.mode==='demo'}>{fetching?'正在获取…':'获取可用模型'}</button></div></div>
   {gateway.mode==='demo'&&<p className="sub">自动获取在桌面端可用，浏览器可预览手动编辑。</p>}
   {models.length>8&&<label className="search-box"><I name="search"/><input aria-label="筛选模型" value={filter} onChange={e=>setFilter(e.target.value)} placeholder="搜索模型 ID 或名称"/></label>}
   <div className="model-rows">{models.filter(m=>(m.id+' '+m.name).toLowerCase().includes(filter.toLowerCase())).map(m=><div className="model-entry" key={m.rowKey}><div className="model-entry-main"><input className="input" aria-label="模型 ID" maxLength={512} value={m.id} onChange={e=>update(m.rowKey,{id:e.target.value})} placeholder="模型 ID"/><input className="input" aria-label="模型显示名称" maxLength={512} value={m.name} onChange={e=>update(m.rowKey,{name:e.target.value})} placeholder="显示名称（可选）"/><button className="icon-btn" type="button" title="删除模型" onClick={()=>{setModels(items=>items.filter(x=>x.rowKey!==m.rowKey));if(defaultModel===m.id)setDefaultModel('');}}><I name="trash"/></button></div><Section className="model-details" title={<>模型参数 <span>{m.contextWindow||m.maxTokens?'已自定义':'可选'}</span></>}><div className="connection-fields"><label className="field"><span>上下文窗口（tokens）</span><input className="input" type="number" min={1} max={4294967295} value={m.contextWindow??''} onChange={e=>update(m.rowKey,{contextWindow:e.target.value===''?null:Number(e.target.value)})} placeholder="使用上游默认值"/></label><label className="field"><span>最大输出（tokens）</span><input className="input" type="number" min={1} max={4294967295} value={m.maxTokens??''} onChange={e=>update(m.rowKey,{maxTokens:e.target.value===''?null:Number(e.target.value)})} placeholder="使用上游默认值"/></label></div></Section></div>)}</div>
   {!models.length&&<p className="catalog-empty">还没有模型。点击“获取可用模型”或手动添加。</p>}
   <button className="btn" type="button" disabled={models.length>=1000} onClick={()=>{setModels(items=>[...items,emptyModel()]);setFilter('');}}><I name="plus"/>添加模型</button>
  </section></Section></fieldset>
  {error&&<p className="form-error" role="alert">{error}</p>}{message&&<p className="catalog-feedback" role="status">{message}</p>}
  <div className="connection-save"><p className="sub">Key 仅保存在当前设备。保存后，使用此连接的实例下次启动生效。</p><div className="form-actions"><Button disabled={working} onClick={()=>{setKey('');onCancel();}}>取消</Button><button type="submit" className="btn primary" disabled={working}>{busy?'正在保存…':'保存'}</button></div></div>
 </form>;
}
