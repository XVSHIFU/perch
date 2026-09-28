import {createElement,useEffect,useState,useMemo,useId,useRef,type ReactNode} from 'react';
import {marked} from 'marked';
import {invoke} from '@tauri-apps/api/core';
import {I} from './ui';
import {DrawerHeaderActions} from './DetailDrawer';
import {readmeUrl,headingSlug} from './readmeLinks';
import './readme.css';

type Readme={markdown:string;sourceUrl:string;baseUrl?:string;language:string;languages:string[]};
function SourceLink({href,children,className,title}:{href:string;children:ReactNode;className?:string;title?:string}){
 const [error,setError]=useState('');
 return <><a href={href} className={className} title={title} aria-label={title} onClick={event=>{event.preventDefault();setError('');void invoke('open_resource_source',{address:href}).catch(e=>setError(String(e)));}}>{children}</a>{error&&<small role="alert">{error}</small>}</>;
}
function ReadmeImage({src,alt,width,height}:{src:string;alt:string;width?:number;height?:number}){
 const [failed,setFailed]=useState(false);
 return failed?<span className="readme-image-fallback"><I name="alert"/> {alt||'图片未能加载'}</span>:<img src={src} alt={alt} width={width} height={height} loading="lazy" decoding="async" referrerPolicy="no-referrer" onError={()=>setFailed(true)}/>;
}
function htmlNodes(source:string,base:string,prefix:string):ReactNode[]{
 // Parse in an inert template; rebuild a small presentation-only allowlist.
 // Never copy styles, event handlers, srcdoc or arbitrary source attributes.
 const template=document.createElement('template');template.innerHTML=source;
 const blocked=new Set(['script','style','iframe','object','embed','svg','math','link','meta','base','form','button','textarea','select','video','audio','source']);
 const allowed=new Set(['p','div','span','strong','b','em','i','s','del','br','hr','ul','ol','li','blockquote','pre','code','h1','h2','h3','h4','h5','h6','table','thead','tbody','tr','th','td','sup','sub','dl','dt','dd','details','summary','kbd']);
 const slugs=new Map<string,number>();
 const dimension=(el:Element,key:string)=>{const value=el.getAttribute(key)||'';return /^\d{1,4}$/.test(value)?Math.min(Number(value),1600):undefined;};
 const render=(node:Node,key:string):ReactNode=>{
  if(node.nodeType===Node.TEXT_NODE)return node.textContent;
  if(node.nodeType!==Node.ELEMENT_NODE)return null;
  const el=node as Element,tag=el.tagName.toLowerCase();if(blocked.has(tag))return null;
  if(tag==='img'){const src=readmeUrl(el.getAttribute('src')||'',base,true);return src?<ReadmeImage key={key+src} src={src} alt={el.getAttribute('alt')||''} width={dimension(el,'width')} height={dimension(el,'height')}/>:<span key={key}>{el.getAttribute('alt')||''}</span>;}
  if(tag==='input')return el.getAttribute('type')==='checkbox'?<input key={key} type="checkbox" checked={el.hasAttribute('checked')} disabled aria-label="清单状态"/>:null;
  const children=Array.from(el.childNodes).map((child,i)=>render(child,`${key}-${i}`));
  if(tag==='a'){
   const target=el.getAttribute('href')||'';
   if(target.startsWith('#')){let id=target.slice(1);try{id=decodeURIComponent(id);}catch{/* retain literal fragment */}return <a key={key} href={'#'+prefix+id} onClick={event=>{event.preventDefault();document.getElementById(prefix+id)?.scrollIntoView({block:'start',behavior:'smooth'});}}>{children}</a>;}
   const href=readmeUrl(target,base);return href?<SourceLink key={key} href={href}>{children}</SourceLink>:<span key={key}>{children}</span>;
  }
  if(!allowed.has(tag))return <span key={key}>{children}</span>;
  const props:Record<string,unknown>={key};
  const align=el.getAttribute('align');if(align&&['left','center','right'].includes(align))props.className='readme-align-'+align;
  if(/^h[1-6]$/.test(tag)){const slug=headingSlug(el.textContent||''),count=slugs.get(slug)||0;slugs.set(slug,count+1);props.id=prefix+slug+(count?'-'+count:'');}
  else if(el.id)props.id=prefix+el.id;
  if(tag==='details'&&el.hasAttribute('open'))props.open=true;
  if(tag==='td'||tag==='th'){for(const attr of ['colspan','rowspan']){const n=dimension(el,attr);if(n&&n<=100)props[attr==='colspan'?'colSpan':'rowSpan']=n;}}
  const result=createElement(tag,props,...(tag==='br'||tag==='hr'?[]:children));
  return tag==='table'?<div className="readme-table" key={key}>{result}</div>:result;
 };
 return Array.from(template.content.childNodes).map((node,i)=>render(node,String(i)));
}
export function ReadmeMarkdown({text,base}:{text:string;base:string}){
 const prefix=useId()+'-readme-',article=useRef<HTMLElement>(null);
 const [headings,setHeadings]=useState<{id:string;title:string;level:number}[]>([]),[active,setActive]=useState('');
 const nodes=useMemo(()=>htmlNodes(marked.parse(text,{async:false,gfm:true}),base,prefix),[text,base,prefix]);
 useEffect(()=>{
  const root=article.current;if(!root)return;
  const elements=Array.from(root.querySelectorAll<HTMLElement>('h1,h2,h3,h4,h5,h6'));
  setHeadings(elements.map(el=>({id:el.id,title:el.textContent||'',level:Number(el.tagName[1])})));
  const scroll=root.closest('.detail-drawer-content')??root.parentElement;
  const update=()=>{const top=scroll?.getBoundingClientRect().top??0;let current=elements[0]?.id??'';for(const el of elements){if(el.getBoundingClientRect().top<=top+80)current=el.id;else break;}setActive(current);};
  scroll?.addEventListener('scroll',update,{passive:true});const resize=new ResizeObserver(update);resize.observe(root);update();
  return()=>{scroll?.removeEventListener('scroll',update);resize.disconnect();};
 },[nodes]);
 return <><article className="readme-markdown" ref={article}>{nodes}</article>{headings.length>1&&<nav className="readme-outline" aria-label="README 目录"><div className="readme-outline-ticks" aria-hidden="true">{headings.map(item=><span key={item.id} className={item.id===active?'active':''} style={{width:item.level<=2?14:8}}/>)}</div><button className="readme-outline-handle" type="button" aria-label="查看 README 目录"><I name="layers"/></button><div className="readme-outline-panel"><strong>目录</strong>{headings.map(item=><button key={item.id} type="button" className={item.id===active?'active':''} aria-current={item.id===active?'location':undefined} style={{paddingLeft:12+Math.min(item.level-1,3)*8}} onClick={()=>document.getElementById(item.id)?.scrollIntoView({block:'start',behavior:'smooth'})}>{item.title}</button>)}</div></nav>}</>;
}
export function ReadmeView({packageName,repository}:{packageName?:string|null;repository?:string|null}){
 const [doc,setDoc]=useState<Readme|null>(null),[error,setError]=useState(''),[retry,setRetry]=useState(0),[language,setLanguage]=useState('zh'),[loading,setLoading]=useState(true);
 useEffect(()=>{let active=true;setLoading(true);setError('');void invoke<Readme>('resource_readme',{packageName:packageName||null,repository:repository||null,language}).then(v=>{if(active){setDoc(v);setLoading(false);}}).catch(e=>{if(active){setError(String(e));setLoading(false);}});return()=>{active=false;};},[packageName,repository,retry,language]);
 return <><DrawerHeaderActions>{doc&&<><div className="readme-language" role="group" aria-label="README 语言">{['zh','en'].map(lang=><button key={lang} type="button" aria-pressed={doc.language===lang} disabled={loading||!doc.languages.includes(lang)} title={doc.languages.includes(lang)?undefined:'作者未提供此语言版本'} onClick={()=>setLanguage(lang)}>{lang==='zh'?'中文':'English'}</button>)}</div><SourceLink href={doc.sourceUrl} className="icon-btn" title="在浏览器中查看 README"><I name="external"/></SourceLink></>}</DrawerHeaderActions>
 {error&&<div role="alert" className="readme-feedback"><h3>暂时无法读取 README</h3><p>{error}</p><button type="button" className="btn" onClick={()=>setRetry(v=>v+1)}>重试</button></div>}
 {loading&&<p role="status" className="sub">正在读取 README…</p>}{doc&&<ReadmeMarkdown text={doc.markdown} base={doc.baseUrl||repository||doc.sourceUrl}/>}</>;
}
