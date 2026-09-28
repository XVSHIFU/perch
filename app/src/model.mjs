import {initialInstances} from './fixtures/instances.mjs';
export {initialInstances};
export const resources=[
{id:'code',name:'代码审查',type:'Skill',engine:'通用',description:'检查改动、梳理风险，准备一次放心的提交。',icon:'code'},
{id:'rules',name:'项目规范',type:'Skill',engine:'通用',description:'让助手遵循项目约定、目录结构和代码风格。',icon:'file'},
{id:'research',name:'资料研究',type:'Skill',engine:'通用',description:'从问题出发收集资料，形成有条理的笔记。',icon:'globe'},
{id:'bridge',name:'Agent Plugins Bridge',type:'插件',engine:'DSH',description:'适配其他 Agent 生态的部分扩展能力。',icon:'puzzle',url:'https://github.com/openma-ai/dsh-agents-plugins'},
{id:'web',name:'Pi Web',type:'工作台',engine:'Pi',description:'在浏览器中访问 Pi 的会话、文件与模型配置。',icon:'globe',url:'https://github.com/agegr/pi-web'},
{id:'broken',name:'research-helper',type:'示例插件',engine:'DSH',description:'用于展示故障修复的示例，不对应真实插件检测。',icon:'alert'}];
export const packs=[
{id:'developer',name:'开发者的日常',en:'THE DEVELOPER KIT',description:'从读懂项目，到一次放心的提交。',engine:'Pi',category:'开发',extensions:['code','rules']},
{id:'researcher',name:'把线索变成答案',en:'RESEARCH ESSENTIALS',description:'收集资料，连接线索，写出清楚的结论。',engine:'DSH',category:'研究',extensions:['research','rules']},
{id:'minimal',name:'轻装上阵',en:'LESS, BUT BETTER',description:'从最少的能力开始，留足自由生长的空间。',engine:'Pi',category:'轻量',extensions:['rules']},
{id:'explorer',name:'插件探索实验室',en:'THE EXPLORER EDITION',description:'独立的试验空间，尝试新的工作方式。',engine:'DSH',category:'探索',extensions:['bridge','research']}];
export function switchEngine(engine,ids){return ids.filter(id=>{const r=resources.find(r=>r.id===id);return r&&(r.engine==='通用'||r.engine===engine)})}
export function addPack(engine,ids,pack){return {engine:pack.engine,ids:[...new Set([...switchEngine(pack.engine,ids),...pack.extensions])]}}
export function validateManifest(input){if(!input||input.format!=='perch-pack/v1'||!['Pi','DSH'].includes(input.engine)||typeof input.name!=='string'||!input.name.trim()||input.name.length>60||!Array.isArray(input.extensions)||input.extensions.some(id=>typeof id!=='string'||!resources.some(r=>r.id===id&&id!=='broken')))throw Error('清单格式不正确，请使用本样机导出的 JSON 文件');if(switchEngine(input.engine,input.extensions).length!==input.extensions.length)throw Error('清单中包含不兼容的扩展');return {name:input.name,engine:input.engine,extensions:[...new Set(input.extensions)]}}
export function restoreInstance(instances,snapshot){return instances.map(i=>i.id===snapshot.instance.id?{...structuredClone(snapshot.instance),status:'ready'}:i)}
export function loadState(raw){let s;try{s=JSON.parse(raw)}catch{return null}if(!s||s.schema!==1||!Array.isArray(s.instances)||!s.instances.length)return null;if(s.instances.some(i=>typeof i.id!=='string'||typeof i.name!=='string'||!['Pi','DSH'].includes(i.engine)||!Array.isArray(i.extensions)||!Array.isArray(i.disabled)))return null;return {...s,instances:s.instances.map(i=>({...i,status:i.status==='error'?'error':'ready'}))}}
