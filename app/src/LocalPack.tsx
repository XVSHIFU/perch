import {useState} from 'react';
import {invoke} from '@tauri-apps/api/core';
import {errorText} from './domain';
import DetailDrawer from './DetailDrawer';
import {Button} from './ui';
import {useReportBusy} from './operationStatus';

type Preview={token:string;skills:{name:string;path:string;digest:string;license?:string}[]};
export default function LocalPack({id,disabled}:{id:string;disabled:boolean}){
 const [open,setOpen]=useState(false),[busy,setBusy]=useState(false),[preview,setPreview]=useState<Preview>(),[error,setError]=useState(''),[confirmed,setConfirmed]=useState(false),[done,setDone]=useState(false);
 useReportBusy(busy);
 async function read(){setBusy(true);setError('');setPreview(undefined);setConfirmed(false);setDone(false);try{setPreview(await invoke<Preview>('preview_local_pack',{id}));}catch(e){setError(errorText(e));}finally{setBusy(false);}}
 async function save(){if(!preview)return;setBusy(true);setError('');try{await invoke('save_local_pack',{id,token:preview.token});setDone(true);}catch(e){setError(errorText(e));setPreview(undefined);setConfirmed(false);}finally{setBusy(false);}}
 return <><Button disabled={disabled||busy} onClick={()=>{setOpen(true);void read();}}>整理本地 Skill</Button>
  <DetailDrawer open={open} title="本地 Skill → 新整合包" onClose={()=>setOpen(false)} closeDisabled={busy}>
   <p className="sub">检查此实例已知的自动发现目录，将完整 Skill 连同脚本和参考文件复制进新组合。当前实例、原目录和共享模型连接保持不变。</p>
   {busy&&<p role="status">正在处理本地资源，请稍候…</p>}
   {error&&<p className="form-error" role="alert">{error}</p>}
   {done?<><p role="status">新整合包已保存。到“发现整合包”可编辑、创建独立实例或导出 ZIP；新实例可继续选择已有模型连接。</p><Button primary onClick={()=>setOpen(false)}>完成</Button></>:<>
    {preview&&<><h3>{preview.skills.length?`将纳入 ${preview.skills.length} 个完整 Skill`:'没有待整理的 Skill'}</h3>
     {preview.skills.length>0&&<><p>以下目录的全部文件都会进入组合。请确认内容可分享且不含私密信息；此操作不会自动删除原目录中的重复资源。</p>
      <label className="field"><span><input type="checkbox" checked={confirmed} disabled={busy} onChange={e=>setConfirmed(e.target.checked)}/> 我确认这些完整目录可纳入新组合</span></label>
      <div className="form-actions"><Button primary disabled={busy||!confirmed} onClick={()=>void save()}>复制并保存新整合包</Button></div>
      {preview.skills.map(skill=><div className="fact" key={skill.path}><div><strong>{skill.name}</strong><p className="sub path-text selectable">{skill.path}</p><p className="sub">许可声明：{skill.license??'未声明，请自行确认分享权限'}</p></div></div>)}
     </>}
     {!preview.skills.length&&<p>可使用实例页的“保存为整合包”，保存已受管的组件。</p>}
    </>}
    <Button disabled={busy} onClick={()=>void read()}>重新检查</Button>
   </>}
  </DetailDrawer>
 </>;
}
