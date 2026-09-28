import type {ReactNode} from 'react';
import {I} from './ui';
export default function SettingsFrame({section,onSection,children}:{section:string;onSection:(v:string)=>void;children:ReactNode}){
 return <div className="settings-layout"><header className="settings-title"><h1>设置</h1></header><div className="settings-body"><nav className="settings-nav" aria-label="设置分类"><button className={section==='settings'?'active':''} aria-current={section==='settings'?'page':undefined} onClick={()=>onSection('settings')}><I name="settings"/>通用设置</button><button className={section==='connections'?'active':''} aria-current={section==='connections'?'page':undefined} onClick={()=>onSection('connections')}><I name="globe"/>模型连接</button></nav><div className="settings-main">{children}</div></div></div>;
}
