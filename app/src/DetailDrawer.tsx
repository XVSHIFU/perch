import {useEffect,useId,useRef,useState,type ReactNode,type CSSProperties} from 'react';
import {createPortal} from 'react-dom';
import {createContext,useContext} from 'react';
import {IconButton} from './ui';

const HeaderContext=createContext<HTMLElement|null>(null);
export function DrawerHeaderActions({children}:{children:ReactNode}){const node=useContext(HeaderContext);return node?createPortal(children,node):null;}
// Modal layers keep parent content mounted while the browser makes it inert.
const layers:HTMLDialogElement[]=[];
export default function DetailDrawer({open,title,onClose,children,closeDisabled=false,headerIcon,className=''}:{open:boolean;title:string;onClose:()=>void;children:ReactNode;closeDisabled?:boolean;headerIcon?:ReactNode;className?:string}){
 const id=useId(),dialog=useRef<HTMLDialogElement>(null),[depth,setDepth]=useState(0),[parentTitle,setParentTitle]=useState(''),[header,setHeader]=useState<HTMLDivElement|null>(null);
 const nested=depth>0;
 const portal=useRef<Element|null>(null);if(open&&!portal.current)portal.current=document.querySelector('.managed-shell')??document.body;
 const close=useRef(onClose);close.current=onClose;
 useEffect(()=>{
  const node=dialog.current;if(!open||!node)return;
  const previous=document.activeElement as HTMLElement|null;
  const parent=layers.at(-1);setDepth(layers.length);setParentTitle(parent?.querySelector('h2')?.textContent??'');parent?.classList.add('drawer-covered');layers.push(node);node.showModal();
  return()=>{const index=layers.indexOf(node);if(index>=0)layers.splice(index,1);parent?.classList.remove('drawer-covered');node.close();if(previous?.isConnected)previous.focus({preventScroll:true});};
 },[open]);
 if(!open)return null;
 const dismiss=()=>{if(!closeDisabled)close.current();};
 return createPortal(<dialog ref={dialog} aria-labelledby={id} className={'drawer-layer'+(nested?' drawer-nested':'')+' '+className} style={{'--drawer-depth':Math.min(depth,3)} as CSSProperties} onCancel={e=>{e.preventDefault();e.stopPropagation();dismiss();}} onClick={e=>{if(e.target===e.currentTarget)dismiss();}}>
  <section className="detail-drawer" aria-label={title}>
   <header className="detail-drawer-heading">{nested&&<IconButton icon="back" label="返回上一级" disabled={closeDisabled} onClick={dismiss}/>}{headerIcon}<div className="drawer-title" title={nested?parentTitle:undefined}><h2 id={id}>{title}</h2></div><div className="drawer-header-actions" ref={setHeader}/><IconButton icon="close" label={nested?'关闭当前层':'关闭详情'} disabled={closeDisabled} onClick={dismiss}/></header>
   <div className="detail-drawer-content"><HeaderContext.Provider value={header}>{children}</HeaderContext.Provider>{closeDisabled&&<p className="drawer-busy-note" role="status">当前操作完成后可关闭此面板。</p>}</div>
  </section>
 </dialog>,portal.current!);
}
