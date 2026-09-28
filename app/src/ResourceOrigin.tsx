import {useState} from 'react';
import {invoke} from '@tauri-apps/api/core';
import {I} from './ui';
export function repositoryLink(value?:string|null){
 if(!value)return null;
 try{const url=new URL(value.replace(/^git\+/,''));if(url.protocol!=='https:'||url.hostname!=='github.com'||url.username||url.password)return null;
 const parts=url.pathname.split('/').filter(Boolean);if(parts.length<2||!parts.slice(0,2).every(v=>/^[\w.-]+$/.test(v)))return null;
 const name=parts.slice(0,2).join('/').replace(/\.git$/,'');return {url:'https://github.com/'+name,name};}catch{return null;}
}
export function resourceRepository(resource:{homepage:string;requirements?:unknown}){
 const direct=repositoryLink(resource.homepage);if(direct)return direct;
 const metadata=resource.requirements as {contentSource?:{repository?:string}}|undefined;return repositoryLink(metadata?.contentSource?.repository);
}
export default function ResourceOrigin({address,label,onOpen}:{address?:string;label?:string;onOpen?:(url:string,name:string)=>void}){
 const [error,setError]=useState('');const repo=repositoryLink(address);
 let href=repo?.url;try{if(!href&&address){const u=new URL(address);if(u.protocol==='https:'&&!u.username&&!u.password)href=u.href;}}catch{/* source may be a local path */}
 const text=repo?.name||label?.replace(/\s*[·:]\s*[a-f0-9]{32,64}\b/gi,'')||address||'未记录来源';
 return <>{href?<a className="resource-origin-link" href={href} title={href} onClick={event=>{event.preventDefault();if(onOpen&&href){onOpen(href,text);return;}setError('');void invoke('open_resource_source',{address:href}).catch(e=>setError(String(e)));}}><I name={repo?'github':'external'}/><span>{text}</span></a>:<span>{text}</span>}{error&&<small role="alert">{error}</small>}</>;
}
