import {useId,useState,type ReactNode} from 'react';
import {I} from './ui';
import './section.css';

/** Optional settings stay mounted so closing a group never discards edits. */
export default function Section({title,description,children,className='',open,onOpenChange,defaultOpen=false,collapsible=true}:{
 title:ReactNode;description?:ReactNode;children:ReactNode;className?:string;
 open?:boolean;onOpenChange?:(open:boolean)=>void;defaultOpen?:boolean;collapsible?:boolean;
}){
 const id=useId(),[expanded,setExpanded]=useState(defaultOpen);
 const visible=!collapsible||(open??expanded);
 const heading=<span className="settings-section-label"><span className="settings-section-title">{title}</span>{description&&<span className="settings-section-description">{description}</span>}</span>;
 return <section className={`settings-section ${className}`}>
  {collapsible?<button type="button" className="settings-section-trigger" aria-expanded={visible} aria-controls={id} onClick={()=>{setExpanded(!visible);onOpenChange?.(!visible);}}>{heading}<I name="chevron"/></button>:<div className="settings-section-heading">{heading}</div>}
  <div id={id} className="settings-section-body" hidden={!visible}>{children}</div>
 </section>;
}
