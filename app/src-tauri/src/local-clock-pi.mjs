export default function(pi){
 pi.registerTool({
  name:'perch_local_time',label:'本地时间',description:'Read the current local time and time zone. Does not access files or the network.',
  parameters:{type:'object',properties:{},additionalProperties:false},
  async execute(){const value={time:new Date().toISOString(),zone:Intl.DateTimeFormat().resolvedOptions().timeZone};return {content:[{type:'text',text:JSON.stringify(value)}],details:value};}
 });
 console.log("PERCH_EXTENSION_READY: pi-local-clock");
}
