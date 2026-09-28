import React from 'react';
import {createRoot} from 'react-dom/client';
import {lazy,Suspense} from 'react';
import {isNative} from './native';
import WorkspaceApp from './WorkspaceApp';
const LegacyDemo=lazy(()=>import('./App'));
import './styles.css';
import './workspace.css';
const legacy=new URLSearchParams(location.search).get('demo')==='legacy';
// The desktop shell scrolls its own panels; browser zoom/navigation must not pan the shell.
if(isNative){
 document.documentElement.classList.add('desktop-host');
 document.addEventListener('wheel',e=>{if(e.ctrlKey)e.preventDefault();},{passive:false});
 document.addEventListener('keydown',e=>{if((e.ctrlKey||e.metaKey)&&['+','-','=','0'].includes(e.key))e.preventDefault();});
 document.addEventListener('dragstart',e=>{if(!(e.target as HTMLElement).closest('input,textarea,[contenteditable=true]'))e.preventDefault();});
 document.addEventListener('contextmenu',e=>{if(!(e.target as HTMLElement).closest('input,textarea,[contenteditable=true],.selectable,.path-text,pre'))e.preventDefault();});
}
createRoot(document.getElementById('root')!).render(legacy?<Suspense fallback={<p>正在加载演示…</p>}><a className="demo-return" href="?">返回本机工作空间</a><LegacyDemo/></Suspense>:<WorkspaceApp/>);
