import {useEffect,useId,useRef,useState} from 'react';
import {createPortal} from 'react-dom';
import type {Instance} from './domain';
import {I,Logo} from './ui';

export default function QuickSearch({instances,onOpen}:{instances:Instance[];onOpen:(instance:Instance)=>void}){
 const [query,setQuery]=useState(''),[open,setOpen]=useState(false),[active,setActive]=useState(0);
 const [position,setPosition]=useState({left:12,top:120});
 const input=useRef<HTMLInputElement>(null),anchor=useRef<HTMLDivElement>(null),panel=useRef<HTMLDivElement>(null);
 const id=useId();
 const term=query.trim().toLowerCase();
 const results=instances.filter(i=>[i.name,i.engine,i.projectPath].some(value=>value.toLowerCase().includes(term))).slice(0,20);
 const selected=Math.min(active,Math.max(0,results.length-1));
 function locate(){const box=anchor.current?.getBoundingClientRect();if(box)setPosition({left:box.left,top:box.bottom+4});}
 function show(){locate();setOpen(true);}
 function choose(instance:Instance){setOpen(false);setQuery('');setActive(0);input.current?.blur();onOpen(instance);}
 useEffect(()=>{const shortcut=(e:KeyboardEvent)=>{if((e.ctrlKey||e.metaKey)&&e.key.toLowerCase()==='k'){e.preventDefault();if(document.querySelector('dialog[open]'))return;input.current?.focus();input.current?.select();locate();setOpen(true);}};window.addEventListener('keydown',shortcut);return()=>window.removeEventListener('keydown',shortcut);},[]);
 useEffect(()=>{if(!open)return;const outside=(e:PointerEvent)=>{if(!anchor.current?.contains(e.target as Node)&&!panel.current?.contains(e.target as Node))setOpen(false);};document.addEventListener('pointerdown',outside);window.addEventListener('resize',locate);return()=>{document.removeEventListener('pointerdown',outside);window.removeEventListener('resize',locate);};},[open]);
 useEffect(()=>{panel.current?.querySelector('[aria-selected="true"]')?.scrollIntoView({block:'nearest'});},[selected,query]);
 return <><div ref={anchor} className="side-search quick-search"><I name="search"/><input ref={input} role="combobox" aria-label="快速查找实例" aria-expanded={open} aria-controls={id} aria-autocomplete="list" aria-activedescendant={open&&results.length?`${id}-${selected}`:undefined} placeholder="快速查找" value={query} onFocus={show} onClick={show} onChange={e=>{setQuery(e.target.value);setActive(0);show();}} onBlur={e=>{if(!anchor.current?.contains(e.relatedTarget)&&!panel.current?.contains(e.relatedTarget))setOpen(false);}} onKeyDown={e=>{if(e.nativeEvent.isComposing)return;if(e.key==='Escape'){e.preventDefault();setOpen(false);}else if(e.key==='ArrowDown'||e.key==='ArrowUp'){e.preventDefault();show();setActive((selected+(e.key==='ArrowDown'?1:-1)+results.length)%Math.max(1,results.length));}else if(e.key==='Enter'&&open&&results[selected]){e.preventDefault();choose(results[selected]);}}}/>{query?<button type="button" aria-label="清空搜索" className="quick-clear" onClick={()=>{setQuery('');setActive(0);input.current?.focus();show();}}><I name="close"/></button>:<kbd>Ctrl K</kbd>}</div>
 {open&&createPortal(<div ref={panel} className="quick-results" style={{left:position.left,top:position.top,maxHeight:`calc(100vh - ${position.top+16}px)`}}><div id={id} role="listbox" aria-label="实例搜索结果">{results.map((instance,index)=><div key={instance.id} id={`${id}-${index}`} role="option" aria-selected={selected===index} className="quick-result" onMouseDown={e=>e.preventDefault()} onMouseEnter={()=>setActive(index)} onClick={()=>choose(instance)}><Logo engine={instance.engine}/><span className="grow"><strong>{instance.name}</strong><span className="sub" title={instance.projectPath}>{instance.engine} · {instance.projectPath}</span></span>{selected===index&&<kbd>Enter</kbd>}</div>)}</div>{!results.length&&<p className="quick-empty" role="status">{term?'没有匹配的实例，试试名称、引擎或项目路径。':'还没有实例，创建后可在这里快速打开。'}</p>}<div className="quick-hint">↑ ↓ 选择 · Enter 打开 · Esc 收起</div></div>,document.body)}</>;
}
