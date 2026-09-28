export function resourceKind(kind:string){return kind==='themes'?'skins':kind;}
export function resourceKindLabel(kind:string){return ({skins:'主题',pets:'桌宠',presets:'预设',plugins:'插件',extensions:'扩展',skills:'Skill',prompts:'提示词','dsh-bundle':'DSH 插件'} as Record<string,string>)[resourceKind(kind)]??kind;}
export function matchesResourceFilter(item:{id:string;source:string},kinds:string[],filter:string){
 if(filter==='all')return true;
 if(filter==='npm')return item.source==='npm'||item.source==='Pi 官方目录';
 if(filter==='仓库发现')return item.id.startsWith('github-dsh:');
 if(filter==='plugins')return item.id.startsWith('workshop:plugins:');
 return kinds.some(kind=>resourceKind(kind)===resourceKind(filter));
}
