import {connectionIssue,runtimeActionBlockReason,startBlockReason} from './runtimeReadiness';
import {useReportBusy} from './operationStatus';
import Section from './Section';
import {useState} from 'react';
import {errorText,runtimeLabels,type Gateway,type Instance,type ModelConnection,type RuntimeView} from './domain';
import {Button,IconButton} from './ui';
export default function InstanceRuntime({instance,connection,runtime,gateway,onRefresh,onConnections,compact=false,iconOnly=false}:{instance:Instance;connection?:ModelConnection;runtime?:RuntimeView;gateway:Gateway;onRefresh:()=>Promise<void>;onConnections:()=>void;compact?:boolean;iconOnly?:boolean}){
 const [sending,setSending]=useState(false),[error,setError]=useState('');
 useReportBusy(sending);
 const startReason=startBlockReason(gateway.mode,instance,connection);const modelIssue=connectionIssue(instance,connection);
 const status=runtime?.status??'not_installed';const working=['installing','starting','stopping'].includes(status);
 async function action(action:'install'|'start'|'stop'|'open'){
  setSending(true);setError('');try{if(action==='open')await gateway.openEngine(instance.id);else await gateway.engineAction(instance.id,action,crypto.randomUUID());await onRefresh();}catch(e){setError(errorText(e));}finally{setSending(false);}
 }
 if(compact){const label=status==='running'?'打开工作台':working?runtimeLabels[status]:status==='not_installed'?'安装并启动':'启动实例';const reason=runtimeActionBlockReason(status,gateway.mode,instance,connection);const disabled=sending||working||!!reason;return <span className="runtime-compact">{iconOnly?<IconButton icon={status==='running'?'external':'play'} label={(reason||label)+' · '+instance.name} disabled={disabled} onClick={()=>void action(status==='running'?'open':'start')}/>:<Button title={reason||label} primary icon={status==='running'?'external':'play'} disabled={disabled} onClick={()=>void action(status==='running'?'open':'start')}>{working?label:'继续「'+instance.name+'」'}</Button>}{(status!=='running'&&!!modelIssue)&&<Button small onClick={onConnections}>{connection?'配置模型连接':'选择模型连接'}</Button>}{!iconOnly&&reason&&<span className="sub">{reason}</span>}{error&&<span className="form-error" role="alert">{error}</span>}</span>;}
 return <section className="instance-runtime" aria-label="工作台运行状态"><div className="section-title"><h2>{runtimeLabels[status]}</h2>{status==='running'&&<span className="pill">{instance.engine} 工作台</span>}</div>
 <p className="sub" role="status">{runtime?.phase||(status==='not_installed'?'首次启动会安装固定版本的引擎与工作台。':status==='stopped'?'工作台已停止。启动时将检查固定版本，并沿用此实例的配置与会话。':'正在读取运行状态。')}</p>
 {status==='running'&&runtime?.connectionRevision!==connection?.revision&&<p className="notice">模型连接已更新，停止后重新启动将应用新配置。</p>}
 {(error||runtime?.error)&&<p className="form-error" role="alert">{error||runtime?.error?.message}</p>}
 <Section description="接口协议、扩展支持和版本边界" className="runtime-capabilities" title={<>引擎能力与兼容范围</>}><p className="path-text">固定组合：{instance.profileId}</p><p className="sub">{instance.engine==='Pi'?'Pi 实例独立保存设置与会话；栖点适配 OpenAI Chat、Responses、Anthropic 和 DeepSeek 兼容接口。Pi 扩展中的终端界面不一定能在 Web 工作台显示。':'DSH 实例独立保存设置与会话；栖点当前适配 DeepSeek 和 OpenAI Chat 接口。DSH 插件与 Pi 扩展不可互装，插件有无 Web 界面需单独确认。'}历史版本的实际能力可能不同，请结合所选版本的声明确认。模型是否可用仍取决于服务商和所选模型。停止实例会结束其中正在进行的任务。</p></Section>
 {status!=='running'&&startReason&&<p className="sub" role="status">{startReason}</p>}
 <div className="form-actions">
 {status==='running'?<><Button primary disabled={sending} onClick={()=>void action('open')}>打开工作台</Button><Button disabled={sending} onClick={()=>void action('stop')}>停止实例</Button></>:<><Button primary title={startReason||undefined} disabled={sending||working||!!startReason} onClick={()=>void action('start')}>{working?runtimeLabels[status]:status==='not_installed'?'安装并启动':'启动实例'}</Button>{status==='not_installed'&&<Button disabled={sending||gateway.mode!=='desktop'} onClick={()=>void action('install')}>仅安装</Button>}</>}
 {working&&status!=='stopping'&&<Button onClick={()=>void gateway.cancelEngine(instance.id).catch(e=>setError(errorText(e)))}>取消操作</Button>}
 <Button onClick={onConnections}>{!connection?'选择模型连接':modelIssue?'配置模型连接':'管理模型连接'}</Button></div>
 {runtime?.logs.length? <Section description="查看引擎启动与运行信息，可选择复制。" className="runtime-log" title={<>运行日志 · 最近 {runtime.logs.length} 行</>}><pre className="selectable">{runtime.logs.join('\n')}</pre></Section>:null}
 </section>;
}
