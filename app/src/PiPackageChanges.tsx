import {useEffect,useState} from 'react';
import {invoke} from '@tauri-apps/api/core';
import {errorText,type Recipe} from './domain';
import {Button} from './ui';

type Drift={token:string;changes:{package:string;expected:string;actual:string}[];adoptionError:string|null};
export default function PiPackageChanges({id,running,recipe,onChange}:{id:string;running:boolean;recipe:Recipe;onChange:()=>Promise<void>}) {
  const [drift,setDrift]=useState<Drift>(),[error,setError]=useState(''),[busy,setBusy]=useState(false),[confirm,setConfirm]=useState<'adopt'|'restore'|null>(null),[message,setMessage]=useState('');
  async function refresh(){
    setBusy(true);setError('');setConfirm(null);
    try{setDrift(await invoke<Drift>('pi_package_drift',{id}));}
    catch(e){setError(errorText(e));setDrift(undefined);}
    finally{setBusy(false);}
  }
  useEffect(()=>{void refresh();},[id]);
  async function apply(){
    if(!drift||!confirm)return;
    setBusy(true);setError('');
    try{
      await invoke(confirm==='adopt'?'adopt_pi_package_selection':'restore_pi_package_selection',{id,token:drift.token,expectedRecipe:recipe});
      setMessage(confirm==='adopt'?'已备份并采纳工作台选择，实例组合已同步。':'已备份并恢复包选择，下次启动将应用当前组合。');
      await onChange();
      await refresh();
    }catch(e){setError(errorText(e));setConfirm(null);}
    finally{setBusy(false);}
  }
  return <section aria-label="Pi 工作台包配置">
    <div className="section-title"><h3>工作台中的包选择</h3><Button disabled={busy} onClick={()=>void refresh()}>{busy?'正在处理…':'检查差异'}</Button></div>
    {error&&<p role="alert" className="form-error">{error}</p>}
    {message&&<p role="status">{message}</p>}
    {drift?.changes.length===0&&<p className="sub">已记录的受管包没有发现工作台改动。</p>}
    {!!drift?.changes.length&&<>
      <p>工作台里的选择已变化。启动前需要处理这些差异，当前文件仍保留。</p>
      {drift.changes.map((change,index)=><div className="fact" key={index}><div><strong>{change.package}</strong><p>栖点记录：{change.expected}</p><p>工作台当前：{change.actual}</p></div></div>)}
      <p className="sub">采纳会将上述选择写入实例组合；恢复会还原原选择。两种操作都会先创建恢复点，其他手动包保持原样。</p>
      {drift.adoptionError&&<p>{drift.adoptionError}</p>}
      {running&&<p>请先停止实例，再处理包选择。</p>}
      {confirm?<div className="form-actions"><Button disabled={busy} onClick={()=>setConfirm(null)}>取消</Button><Button primary disabled={busy||running} onClick={()=>void apply()}>{confirm==='adopt'?'备份并确认采纳':'备份并确认恢复'}</Button></div>:<div className="form-actions"><Button disabled={busy||running||!!drift.adoptionError} onClick={()=>setConfirm('adopt')}>采纳工作台选择</Button><Button disabled={busy||running} onClick={()=>setConfirm('restore')}>恢复栖点记录</Button></div>}
    </>}
  </section>;
}
