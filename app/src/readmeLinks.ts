export function headingSlug(text:string){return text.toLowerCase().trim().replace(/[^\p{L}\p{N}\p{M}_\-\s]/gu,'').replace(/ /g,'-');}
export function readmeUrl(value:string,base:string,image=false):string|null{
 if(!value.trim())return null;
 try{
  const origin=new URL(base);
  if(!/^(?:https:)?\/\//i.test(value)&&origin.hostname!=='github.com'&&origin.hostname!=='raw.githubusercontent.com')return null;
  if(origin.hostname==='github.com'){
   const parts=origin.pathname.split('/').filter(Boolean);
   if(parts.length===2)origin.pathname=`/${parts[0]}/${parts[1].replace(/\.git$/,'')}/blob/HEAD/`;
   else if(parts[2]==='tree')origin.pathname=origin.pathname.replace('/tree/','/blob/').replace(/\/$/,'')+'/';
  }
  origin.hash='';origin.search='';const url=new URL(value,origin);
  if(url.protocol!=='https:'||url.username||url.password)return null;
  if(image&&url.hostname==='github.com'&&/^\/[^/]+\/[^/]+\/(blob|raw)\//.test(url.pathname)){url.hostname='raw.githubusercontent.com';url.pathname=url.pathname.replace(/\/(blob|raw)\//,'/');url.search='';}
  return url.href;
 }catch{return null;}
}
