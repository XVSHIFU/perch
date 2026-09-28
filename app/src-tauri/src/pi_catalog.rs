//! Pi's public catalog is server-rendered HTML, not an assumed private JSON API.
use crate::resource_catalog::{Resource,ResourceCatalog,CacheRefresh,conditional_text,save_cache};
use std::{fs,path::Path,time::Duration};
use tauri::Manager;
#[derive(serde::Serialize,serde::Deserialize,Default)]
#[serde(rename_all="camelCase")]
pub struct PiCatalog {#[serde(flatten)]catalog:ResourceCatalog,#[serde(default)]next_page:Option<usize>}
fn decode(text:&str)->String{
 let mut output=String::new();let mut rest=text;
 while let Some(index)=rest.find('&'){
  output.push_str(&rest[..index]);rest=&rest[index..];
  let Some(end)=rest.find(';').filter(|n|*n<=12) else{output.push('&');rest=&rest[1..];continue;};
  let entity=&rest[1..end];let ch=match entity{"amp"=>Some('&'),"quot"=>Some('"'),"apos"=>Some('\''),"lt"=>Some('<'),"gt"=>Some('>'),"nbsp"=>Some(' '),_=>entity.strip_prefix("#x").and_then(|s|u32::from_str_radix(s,16).ok()).or_else(||entity.strip_prefix('#').and_then(|s|s.parse().ok())).and_then(char::from_u32)};
  if let Some(ch)=ch{output.push(ch);}else{output.push_str(&rest[..=end]);}rest=&rest[end+1..];
 }output.push_str(rest);output
}
fn attr<'a>(tag:&'a str,name:&str)->Option<&'a str>{tag.split_once(&format!("{name}=\""))?.1.split_once('"').map(|v|v.0)}
fn parse(html:&str,page:usize)->Result<(Vec<Resource>,Option<usize>),String>{
 let mut entries=Vec::new();
 for fragment in html.split("<article").skip(1){
  let (tag,body)=fragment.split_once('>').ok_or("Pi 目录卡片结构已变化")?;
  if attr(tag,"data-package-card")!=Some("true"){continue;}
  let name=decode(attr(tag,"data-package-name").ok_or("Pi 卡片缺少包名")?);
  if !crate::engine_profile::valid_package_name(&name){return Err("Pi 目录包名无效".into());}
  let description=body.split_once("class=\"packages-desc\">").and_then(|(_,rest)|rest.split_once("</p>")).map(|(s,_)|decode(s)).unwrap_or_default();
  let kinds=attr(tag,"data-package-types").unwrap_or("").split_whitespace().filter_map(|kind|match kind{"extension"=>Some("extensions"),"skill"=>Some("skills"),"prompt"=>Some("prompts"),"theme"=>Some("themes"),_=>None}).map(str::to_string).collect::<Vec<_>>();
  entries.push(Resource{id:format!("npm:{name}"),homepage:format!("https://pi.dev/packages/{name}"),package_name:Some(name.clone()),name,description,version:"latest".into(),engine:"Pi".into(),source:"Pi 官方目录".into(),kinds:if kinds.is_empty(){vec!["待读取包清单".into()]}else{kinds},requirements:serde_json::json!({"catalog":"https://pi.dev/packages","versionStatus":"安装前读取 npm 精确版本"})});
 }
 if entries.is_empty(){return Err("Pi 官方目录页面未识别到资源卡片，旧缓存保留；可打开来源页面浏览".into());}
 let next=page+1;
 Ok((entries,html.contains(&format!("href=\"/packages?page={next}\"")).then_some(next)))
}
fn read(root:&Path,refresh:bool,more:bool)->Result<PiCatalog,String>{
 let path=root.join("pi-official-catalog.json");if !refresh{crate::catalog_seed::ensure(&path)?;}let mut cached:PiCatalog=fs::read(&path).ok().and_then(|b|serde_json::from_slice(&b).ok()).unwrap_or_default();
 if (!refresh&&path.exists())||!crate::resource_catalog::preferences(root)?.pi_official{return Ok(cached);}
 let _guard=CacheRefresh::begin(&path)?;
 cached.catalog.bundled=None;
 let client=reqwest::blocking::Client::builder().user_agent("Perch-resource-catalog").timeout(Duration::from_secs(25)).redirect(reqwest::redirect::Policy::none()).build().map_err(|_|"Pi 目录网络初始化失败")?;
 let mut page=if more{match cached.next_page{Some(page)=>page,None=>return Ok(cached)}}else{1};
 cached.catalog.errors.clear();
 // A small batch keeps refresh responsive; the user can explicitly continue.
 for index in 0..5{
  if index>0{std::thread::sleep(Duration::from_millis(600));}
  let address=if page==1{"https://pi.dev/packages".into()}else{format!("https://pi.dev/packages?page={page}")};
  match conditional_text(root,&client,&address).and_then(|html|parse(&html,page)){
   Ok((entries,next))=>{
    for entry in entries{if let Some(existing)=cached.catalog.entries.iter_mut().find(|item|item.id==entry.id){*existing=entry;}else{cached.catalog.entries.push(entry);}}
    cached.catalog.synced_at=std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_secs();cached.next_page=next;
    if let Some(next)=next{page=next;}else{break;}
   },
   Err(error)=>{cached.next_page=Some(page);cached.catalog.errors.push(error);break;}
  }
 }
 if cached.next_page.is_some(){cached.catalog.errors.push(format!("已缓存 {} 项，目录尚未同步完整，可继续同步或打开来源页面",cached.catalog.entries.len()));}
 save_cache(&path,&serde_json::to_vec(&cached).map_err(|_|"Pi 目录编码失败")?)?;Ok(cached)
}
#[tauri::command]
pub async fn pi_official_catalog(app:tauri::AppHandle,refresh:bool,more:Option<bool>)->Result<PiCatalog,String>{
 let root=app.state::<crate::data_commands::DataState>().root.as_ref().map_err(|_|"工作空间不可用")?.clone();
 tauri::async_runtime::spawn_blocking(move||read(&root,refresh,more.unwrap_or(false))).await.map_err(|_|"Pi 目录任务中断".to_string())?
}
#[cfg(test)]mod tests{
 use super::*;
 #[test]fn official_cards_decode_and_keep_npm_identity(){
  let html=r#"<article data-package-card="true" data-package-name="@owner/tools" data-package-types="extension skill"><p class="packages-desc">Read &amp; write &#39;notes&#39;</p></article><a href="/packages?page=2">Next</a>"#;
  let (items,next)=parse(html,1).unwrap();assert_eq!(next,Some(2));assert_eq!(items[0].id,"npm:@owner/tools");assert_eq!(items[0].description,"Read & write 'notes'");assert_eq!(items[0].kinds,vec!["extensions","skills"]);assert!(parse("<html>access denied</html>",1).is_err());
 }
 #[test]#[ignore="explicit network catalog check"]fn live_official_catalog(){let root=std::env::temp_dir().join(format!("perch-pi-catalog-{}",uuid::Uuid::new_v4()));let value=read(&root,true,false).unwrap();assert!(!value.catalog.entries.is_empty(),"{:?}",value.catalog.errors);assert_eq!(read(&root,false,false).unwrap().catalog.entries.len(),value.catalog.entries.len());println!("Pi official: {} entries; next {:?}; cache {}",value.catalog.entries.len(),value.next_page,root.display());}
}
