import {useEffect,useRef,useState} from 'react';
import {invoke} from '@tauri-apps/api/core';
import {listen} from '@tauri-apps/api/event';
import DetailDrawer,{DrawerHeaderActions} from './DetailDrawer';
import {IconButton} from './ui';
import {errorText} from './domain';
export type SourceLink={name:string;url:string;preview?:{name:string;description:string}[];count?:number};
type PageEvent={token:string;kind:string;value:string};
function NativePage({source,onClose}:{source:SourceLink;onClose:()=>void}){
 const host=useRef<HTMLDivElement>(null),close=useRef(onClose),token=useRef(crypto.randomUUID());close.current=onClose;
 const [address,setAddress]=useState(source.url),[title,setTitle]=useState(source.name),[loading,setLoading]=useState(true),[error,setError]=useState('');
 const control=useRef<(action:string)=>void>(()=>{});
 useEffect(()=>{
  const node=host.current;if(!node)return;
  let disposed=false,opened=false,frame=0,last='',unlisten=()=>{},timeout=0,failed=false,queue=Promise.resolve();
  const id=token.current;
  const run=(action:string,bounds?:{x:number;y:number;width:number;height:number})=>{
   queue=queue.then(()=>invoke<void>('source_browser_control',{token:id,action,bounds:bounds??null})).catch(e=>{if(!disposed)setError(errorText(e));});return queue;
  };
  const measure=()=>{const r=node.getBoundingClientRect();return {x:r.x,y:r.y,width:Math.max(1,r.width),height:Math.max(1,r.height)};};
  const sync=()=>{
   if(!opened||disposed)return;
   const dialog=node.closest('dialog');
   const covered=failed||!dialog?.open||dialog.classList.contains('drawer-covered')||!!document.querySelector('.managed-close[open]')||document.visibilityState==='hidden';
   const bounds=measure(),signature=JSON.stringify([covered,bounds]);if(signature===last)return;last=signature;
   if(covered)void run('hide');else{void run('bounds',bounds);void run('show');}
  };
  const schedule=()=>{cancelAnimationFrame(frame);frame=requestAnimationFrame(sync);};
  const resize=new ResizeObserver(schedule);resize.observe(node);
  const mutation=new MutationObserver(schedule);mutation.observe(document.body,{subtree:true,attributes:true,attributeFilter:['open','class','style']});
  document.addEventListener('visibilitychange',schedule);window.addEventListener('resize',schedule);
  const start=async()=>{
   unlisten=await listen<PageEvent>('source-browser',({payload:p})=>{
    if(disposed||p.token!==id)return;
    if(p.kind==='close'){close.current();return;}
    if(p.kind==='error'){failed=true;setLoading(false);setError(p.value);sync();}
    if(p.kind==='title')setTitle(p.value);
    if(p.kind==='navigate'){failed=false;setAddress(p.value);setError('');setLoading(true);sync();}
    if(p.kind==='loading'){setLoading(true);clearTimeout(timeout);timeout=window.setTimeout(()=>{if(!disposed){setLoading(false);setError('页面加载较慢，可刷新或在浏览器中打开。');}},30000);}
    if(p.kind==='loaded'){setLoading(false);clearTimeout(timeout);}
   });
   if(disposed){unlisten();return;}
   await invoke('source_browser_open',{token:id,url:source.url,bounds:measure()});opened=true;
   if(disposed){await run('close');return;}sync();
  };
  control.current=action=>{if(opened){if(action==='reload'){failed=false;last='';setError('');setLoading(true);sync();}void run(action);}};
  void start().catch(e=>{if(!disposed){setLoading(false);setError(errorText(e));}});
  return()=>{disposed=true;unlisten();clearTimeout(timeout);cancelAnimationFrame(frame);resize.disconnect();mutation.disconnect();document.removeEventListener('visibilitychange',schedule);window.removeEventListener('resize',schedule);if(opened)void run('close');};
 },[source.url]);
 return <><DrawerHeaderActions><div className="source-address" title={title+' · '+address}><span>{address}</span></div><IconButton icon="refresh" label="刷新网页" onClick={()=>control.current('reload')}/><IconButton icon="external" label="在浏览器中打开" onClick={()=>void invoke('open_resource_source',{address}).catch(e=>setError(errorText(e)))}/></DrawerHeaderActions>
  {loading&&<div className="source-loading" role="status" aria-label="正在加载网页"/>}
  {error&&<div className="source-error" role="alert">{error}</div>}
  <div className="native-page-host" ref={host}/>
 </>;
}
export default function SourcePage({source,onClose}:{source?:SourceLink;onClose:()=>void}){
 return <DetailDrawer open={!!source} title={source?.name??'来源页面'} onClose={onClose} className="source-drawer">{source&&<NativePage key={source.url} source={source} onClose={onClose}/>}</DetailDrawer>;
}
