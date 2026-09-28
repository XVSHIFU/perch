import type {Recipe} from './domain';

export default function ThinkingPresetEditor({recipe,onChange}:{recipe:Recipe;onChange:(recipe:Recipe)=>void}){
 return <section aria-label="公开配置预设">
  <h3>公开配置预设</h3>
  <label className="field"><span>默认思考强度</span>
   <select className="input" value={recipe.thinkingLevel??''} onChange={event=>onChange({...recipe,thinkingLevel:(event.target.value||undefined) as Recipe['thinkingLevel']})}>
    <option value="">不指定，沿用工作台设置</option>
    <option value="off">关闭 · off</option><option value="low">低 · low</option>
    <option value="high">高 · high</option><option value="max">最高 · max</option>
   </select>
  </label>
  <p>随组合分享，下次启动时应用。可用档位取决于模型；不支持时请取消预设或选择模型支持的档位。</p>
  <p>已有不同的工作台设置会阻止预设写入。请选择与工作台一致的值，或选“不指定”保留手动修改。移除未被手动修改的预设时，会恢复应用前的设置。</p>
 </section>;
}
