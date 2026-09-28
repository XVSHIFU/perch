import Section from './Section';
import type {ExtensionRef} from './domain';

const kinds=[['extensions','扩展'],['skills','Skills'],['prompts','提示词'],['themes','主题']] as const;
export default function PiResourceRules({item,onChange}:{item:ExtensionRef;onChange:(item:ExtensionRef)=>void}){
 function update(kind:string,mode:string,rules?:string[]){
  const resourceRules={...item.resourceRules};
  delete resourceRules[kind];
  if(mode==='custom')resourceRules[kind]=rules??[''];
  onChange({...item,resourceRules,disabledResources:[...(item.disabledResources??[]).filter(value=>value!==kind),...(mode==='none'?[kind]:[])]});
 }
 return <Section title={<>{item.id.slice(4)} · 包内资源选择</>}>
  <p>整包安装，分别选择要加载的资源。规则原样传给 Pi：普通规则包含，! 排除，+ 强制包含，- 强制排除，后两者优先级更高。Pi Web 不支持的终端交互仍不可用。</p>
  {kinds.map(([kind,label])=>{
   const mode=item.disabledResources?.includes(kind)?'none':item.resourceRules?.[kind]!==undefined?'custom':'all';
   return <section key={kind} aria-label={`${label}加载选择`}>
    <label className="field"><span>{label}</span><select className="input" value={mode} onChange={event=>update(kind,event.target.value)}><option value="all">按包默认加载</option><option value="none">不加载</option><option value="custom">指定文件规则</option></select></label>
    {mode==='custom'&&<label className="field"><span>{label}文件规则（每行一条）</span><textarea className="input" rows={3} spellCheck={false} value={item.resourceRules?.[kind]?.join('\n')??''} onChange={event=>update(kind,'custom',event.target.value.split('\n'))}/><small>使用包内相对路径或 Pi 支持的匹配规则，保留先后顺序。不要填写本机绝对路径；清空规则请改选“不加载”。</small></label>}
   </section>;
  })}
 </Section>;
}
