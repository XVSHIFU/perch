import {defineTool} from '__DSH_TOOLS_URL__';
export const name='perch-local-clock';
export const inject=['tools'];
export function apply(ctx){
 ctx.tools.register(defineTool({
  name:'perch_local_time',description:'Read the current local time and time zone. Does not access files or the network.',parameters:{},
  output:{schema:{type:'object',additionalProperties:false,properties:{time:{type:'string',required:true},zone:{type:'string',required:true}}},render:(_args,value)=>[{type:'text',text:JSON.stringify(value)}]},
  async execute(){return {time:new Date().toISOString(),zone:Intl.DateTimeFormat().resolvedOptions().timeZone};}
 }));
 console.log("PERCH_EXTENSION_READY: dsh-local-clock");
}
