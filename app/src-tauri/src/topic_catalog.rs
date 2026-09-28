use crate::resource_catalog::{conditional_json,save_cache,CacheRefresh,Resource,ResourceCatalog};
use serde_json::Value;
use std::{fs,path::Path,time::Duration};
use tauri::Manager;

fn valid_repository(repo:&str)->bool{
 let parts=repo.split('/').collect::<Vec<_>>();
 parts.len()==2&&parts.iter().all(|p|!p.is_empty()&&*p!="."&&*p!=".."&&p.bytes().all(|c|c.is_ascii_alphanumeric()||b"._-".contains(&c)))
}
fn declared_package(doc:&Value)->Result<(&str,&str),String>{
 if doc["private"].as_bool()==Some(true){return Err("仓库声明为私有包，不能从 npm 安装".into());}
 let name=doc["name"].as_str().filter(|s|crate::engine_profile::valid_package_name(s)).ok_or("根目录缺少有效 npm 包名；多包仓库请在来源页手动填写具体包名与版本")?;
 let version=doc["version"].as_str().filter(|s|!s.is_empty()&&s.as_bytes()[0].is_ascii_digit()&&s.bytes().all(|c|c.is_ascii_alphanumeric()||b".-+".contains(&c))).ok_or("仓库未声明精确 npm 版本")?;
 if doc["dsh"]["bundle"].is_null(){return Err("根目录包未声明 DSH bundle；请查看来源中的安装说明".into()) ;}
 Ok((name,version))
}
fn verify_published(repo:&str,declared:&Value,published:&Value)->Result<(),String>{
 let (name,version)=declared_package(declared)?;
 if published["name"]!=name||published["version"]!=version||published["dsh"]["bundle"]!=declared["dsh"]["bundle"]{return Err("npm 发行版与仓库包名、版本或 DSH 声明不一致，未进入安装预览".into());}
 let address=published["repository"].as_str().or_else(||published["repository"]["url"].as_str()).unwrap_or("").trim_start_matches("git+");
 let canonical=address.strip_prefix("https://github.com/").or_else(||address.strip_prefix("git://github.com/")).or_else(||address.strip_prefix("git@github.com:")).map(|s|s.trim_end_matches('/').trim_end_matches(".git"));
 if !canonical.is_some_and(|s|s.eq_ignore_ascii_case(repo)){return Err("npm 发行版未声明相同 GitHub 来源；请核对来源后通过自定义 npm 入口添加".into());}
 Ok(())
}
#[derive(serde::Serialize)]
pub struct TopicPackage {repository:String,commit:String,manifest:crate::resource_catalog::ResourceManifest}
#[tauri::command]
pub async fn inspect_topic_package(app:tauri::AppHandle,repository:String)->Result<TopicPackage,String>{
 if !valid_repository(&repository){return Err("仓库名称无效".into());}
 let root=app.state::<crate::data_commands::DataState>().root.as_ref().map_err(|_|"工作空间不可用")?.clone();
 tauri::async_runtime::spawn_blocking(move||{
  let client=reqwest::blocking::Client::builder().user_agent("Perch-resource-catalog").timeout(Duration::from_secs(25)).redirect(reqwest::redirect::Policy::none()).build().map_err(|_|"GitHub 请求初始化失败")?;
  let commits=conditional_json(&root,&client,&format!("https://api.github.com/repos/{repository}/commits?per_page=1"))?;
  let commit=commits[0]["sha"].as_str().filter(|s|s.len()==40&&s.bytes().all(|c|c.is_ascii_hexdigit())).ok_or("无法取得仓库固定提交")?;
  let declared=crate::resource_catalog::conditional_json_with(&root,&client,&format!("https://api.github.com/repos/{repository}/contents/package.json?ref={commit}"),Some("application/vnd.github.raw+json"),1024*1024).map_err(|error|format!("无法读取根目录 package.json：{error}；多包仓库可在来源页手动添加 npm 包"))?;
  let (name,version)=declared_package(&declared)?;
  let published=crate::resource_catalog::conditional_json_with(&root,&client,&format!("https://registry.npmjs.org/{}/{}",name.replace('/',"%2F"),version),None,2*1024*1024).map_err(|error|format!("无法读取声明的 npm 发行版 {version}：{error}"))?;
  verify_published(&repository,&declared,&published)?;
  let manifest=crate::resource_catalog::expected_manifest(published,name,version)?;
  Ok(TopicPackage{repository,commit:commit.into(),manifest})
 }).await.map_err(|_|"仓库包识别任务中断".to_string())?
}

fn pages(mut fetch:impl FnMut(usize)->Result<Value,String>)->Result<Vec<Resource>,String>{
 let mut entries=Vec::new();let mut seen=std::collections::HashSet::new();
 for page in 1..=10 {
  let doc=fetch(page)?;
  if doc["incomplete_results"].as_bool()!=Some(false){return Err("GitHub 搜索结果不完整，请稍后刷新".into());}
  let total=doc["total_count"].as_u64().ok_or("GitHub 目录缺少总数")?;
  if total>1000{return Err("GitHub 搜索超过 1000 项，需要细分来源后读取".into());}
  let rows=doc["items"].as_array().ok_or("GitHub 目录缺少仓库列表")?;
  if rows.len()>100{return Err("GitHub 分页大小异常".into());}
  for row in rows {
   let repo=row["full_name"].as_str().ok_or("仓库名称缺失")?;
   let parts=repo.split('/').collect::<Vec<_>>();
   if parts.len()!=2||parts.iter().any(|p|p.is_empty()||*p=="."||*p==".."||!p.bytes().all(|c|c.is_ascii_alphanumeric()||b"._-".contains(&c))){return Err("仓库名称无效".into());}
   if !seen.insert(repo.to_ascii_lowercase()){return Err("GitHub 分页发生重叠，请重新刷新".into());}
   entries.push(Resource{id:format!("github-dsh:{repo}"),name:repo.into(),description:row["description"].as_str().unwrap_or("").into(),version:"未解析".into(),engine:"DSH".into(),source:"GitHub · dsh-plugin".into(),homepage:format!("https://github.com/{repo}"),kinds:vec!["仓库发现".into()],package_name:None,requirements:serde_json::json!({"license":row["license"]["spdx_id"],"author":row["owner"]["login"],"updatedAt":row["updated_at"],"archived":row["archived"],"note":"社区 topic 不是兼容认证；尚未读取安装声明"})});
  }
  if entries.len() as u64>=total{return Ok(entries);}
  if rows.is_empty(){return Err("GitHub 分页提前结束".into());}
 }
 Err("GitHub 分页未完整读取".into())
}
// Split only overflowing ranges. Each leaf still uses the strict page parser,
// so a partial search can never be published as a complete catalogue.
#[cfg(test)]
fn partitioned_pages(mut fetch:impl FnMut(&str,usize)->Result<Value,String>)->Result<Vec<Resource>,String>{
 fn month_days(year:u32,month:u32)->u32{match month{2=>if year%4==0&&(year%100!=0||year%400==0){29}else{28},4|6|9|11=>30,_=>31}}
 enum Range {Years(u32,u32),Month(u32,u32),Day(u32,u32,u32)}
 // GitHub Search rejects dates outside 1970..=2970 with HTTP 422.
 let mut pending=vec![Range::Years(1970,2970)];let mut entries=Vec::new();
 let mut seen=std::collections::HashSet::new();
 while let Some(range)=pending.pop(){
  let qualifier=match range{
   Range::Years(first,last)=>format!("created:{first:04}-01-01..{last:04}-12-31"),
   Range::Month(year,month)=>format!("created:{year:04}-{month:02}-01..{year:04}-{month:02}-{:02}",month_days(year,month)),
   Range::Day(year,month,day)=>format!("created:{year:04}-{month:02}-{day:02}"),
  };
  let first=fetch(&qualifier,1)?;
  if first["incomplete_results"].as_bool()!=Some(false){return Err("GitHub 搜索结果不完整，请稍后刷新".into());}
  let total=first["total_count"].as_u64().ok_or("GitHub 目录缺少总数")?;
  if total>1000{
   match range{
    Range::Years(first,last) if first<last=>{let middle=first+(last-first)/2;pending.push(Range::Years(middle+1,last));pending.push(Range::Years(first,middle));},
    Range::Years(year,_)=>for month in (1..=12).rev(){pending.push(Range::Month(year,month));},
    Range::Month(year,month)=>{let days=month_days(year,month);for day in (1..=days).rev(){pending.push(Range::Day(year,month,day));}},
    Range::Day(_,_,_)=>return Err(format!("{qualifier} 仍超过 GitHub 搜索上限，未发布不完整目录")),
   }
   continue;
  }
  let mut first=Some(first);
  let part=pages(|page|if page==1{Ok(first.take().unwrap())}else{fetch(&qualifier,page)})?;
  for item in part{if seen.insert(item.id.to_ascii_lowercase()){entries.push(item);}}
 }
 Ok(entries)
}
#[derive(serde::Serialize,serde::Deserialize,Clone)]
enum TopicRange {Years(u32,u32),Month(u32,u32),Day(u32,u32,u32),Hour(u32,u32,u32,u32)}
impl TopicRange {
 fn days(y:u32,m:u32)->u32{match m{2=>if y%4==0&&(y%100!=0||y%400==0){29}else{28},4|6|9|11=>30,_=>31}}
 fn query(&self)->String{match *self{
  Self::Years(a,b)=>format!("created:{a:04}-01-01..{b:04}-12-31"),
  Self::Month(y,m)=>format!("created:{y:04}-{m:02}-01..{y:04}-{m:02}-{:02}",Self::days(y,m)),
  Self::Day(y,m,d)=>format!("created:{y:04}-{m:02}-{d:02}"),
  Self::Hour(y,m,d,h)=>format!("created:{y:04}-{m:02}-{d:02}T{h:02}:00:00Z..{y:04}-{m:02}-{d:02}T{h:02}:59:59Z"),
 }}
 fn split(&self)->Vec<Self>{match *self{
  Self::Years(a,b) if a<b=>{let m=a+(b-a)/2;vec![Self::Years(a,m),Self::Years(m+1,b)]},
  Self::Years(y,_) =>(1..=12).map(|m|Self::Month(y,m)).collect(),
  Self::Month(y,m)=>(1..=Self::days(y,m)).map(|d|Self::Day(y,m,d)).collect(),
  Self::Day(y,m,d)=>(0..24).map(|h|Self::Hour(y,m,d,h)).collect(),
  Self::Hour(..)=>vec![],
 }}
}
#[derive(serde::Serialize,serde::Deserialize,Clone)]
struct TopicCursor {range:TopicRange,page:usize}
#[derive(serde::Serialize,serde::Deserialize,Default)]
#[serde(rename_all="camelCase")]
pub struct TopicCatalog {
 #[serde(flatten)] catalog:ResourceCatalog,
 #[serde(default)] pending:Vec<TopicCursor>,
 #[serde(default)] has_more:bool,
}
fn topic_batch(cached:&mut TopicCatalog,mut fetch:impl FnMut(&str,usize)->Result<Value,String>){
 cached.catalog.errors.clear();
 // Persist progress after a small batch, including overlapping parent search rows.
 // Never discard already discovered repositories when a later request is limited.
 for _ in 0..3 {
  let Some(cursor)=cached.pending.last().cloned() else{break;};
  let result=(||{
   let doc=fetch(&cursor.range.query(),cursor.page)?;
   if doc["incomplete_results"].as_bool()!=Some(false){return Err("GitHub 搜索结果暂不完整，已保留进度".to_string());}
   let total=doc["total_count"].as_u64().ok_or("GitHub 目录缺少总数")?;
   let rows=doc["items"].as_array().ok_or("GitHub 目录缺少仓库列表")?;
   if rows.is_empty()&&total>0{return Err("GitHub 分页提前结束，已保留进度".into());}
   let part=pages(|_|Ok(serde_json::json!({"incomplete_results":false,"total_count":rows.len(),"items":rows})))?;
   let mut seen=cached.catalog.entries.iter().map(|r|r.id.to_ascii_lowercase()).collect::<std::collections::HashSet<_>>();
   for item in part{if seen.insert(item.id.to_ascii_lowercase()){cached.catalog.entries.push(item);}}
   cached.pending.pop();
   if total>1000 {
    let children=cursor.range.split();
    if children.is_empty(){cached.pending.push(cursor);return Err("单个时间段超过 GitHub 搜索上限，已保留已发现条目；可打开来源页继续浏览".into());}
    cached.pending.extend(children.into_iter().map(|range|TopicCursor{range,page:1}));
   }else if (cursor.page*100) < total as usize {cached.pending.push(TopicCursor{page:cursor.page+1,..cursor});}
   cached.catalog.synced_at=std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_secs();
   Ok::<(),String>(())
  })();
  if let Err(error)=result{cached.catalog.errors.push(error);break;}
 }
 cached.has_more=!cached.pending.is_empty();
 if cached.has_more{cached.catalog.errors.push(format!("已缓存 {} 项，目录尚未同步完整；可继续同步，已有内容可用。",cached.catalog.entries.len()));}
}
fn read(root:&Path,refresh:bool,more:bool)->Result<TopicCatalog,String>{
 let path=root.join("dsh-topic-catalog.json");if !refresh{crate::catalog_seed::ensure(&path)?;}
 let mut cached:TopicCatalog=fs::read(&path).ok().and_then(|b|serde_json::from_slice(&b).ok()).unwrap_or_default();
 if (!refresh && path.exists())||!crate::resource_catalog::preferences(root)?.dsh_topic{return Ok(cached);}
 let _guard=CacheRefresh::begin(&path)?;
 cached.catalog.bundled=None;
 if !more{cached.pending=vec![TopicCursor{range:TopicRange::Years(1970,2970),page:1}];}
 let client=reqwest::blocking::Client::builder().user_agent("Perch-resource-catalog").timeout(Duration::from_secs(25)).redirect(reqwest::redirect::Policy::none()).build().map_err(|_|"GitHub 请求初始化失败")?;
 let mut last_request:Option<std::time::Instant>=None;
 topic_batch(&mut cached,|range,page|{
  if let Some(last)=last_request{if let Some(wait)=Duration::from_secs(7).checked_sub(last.elapsed()){std::thread::sleep(wait);}}
  last_request=Some(std::time::Instant::now());
  let mut address=reqwest::Url::parse("https://api.github.com/search/repositories").map_err(|_|"GitHub 地址无效")?;
  address.query_pairs_mut().append_pair("q",&format!("topic:dsh-plugin {range}")).append_pair("per_page","100").append_pair("page",&page.to_string()).append_pair("sort","updated").append_pair("order","desc");
  conditional_json(root,&client,address.as_str())
 });
 save_cache(&path,&serde_json::to_vec(&cached).map_err(|_|"社区目录编码失败")?)?;Ok(cached)
}
#[tauri::command]
pub async fn dsh_topic_catalog(app:tauri::AppHandle,refresh:bool,more:Option<bool>)->Result<TopicCatalog,String>{
 let root=app.state::<crate::data_commands::DataState>().root.as_ref().map_err(|_|"工作空间不可用")?.clone();
 tauri::async_runtime::spawn_blocking(move||read(&root,refresh,more.unwrap_or(false))).await.map_err(|_|"社区目录任务中断".to_string())?
}
#[cfg(test)]
mod tests {
 use super::*;
 #[test]
 fn topic_batch_retains_rows_and_resumes_after_failure(){
  let mut catalog=TopicCatalog::default();
  catalog.pending.push(TopicCursor{range:TopicRange::Day(2026,8,14),page:1});
  let mut calls=0;
  topic_batch(&mut catalog,|_,_|{calls+=1;if calls==2{return Err("HTTP 429".into());}Ok(serde_json::json!({"incomplete_results":false,"total_count":1200,"items":[{"full_name":"owner/plugin"}]}))});
  assert_eq!(calls,2);assert_eq!(catalog.catalog.entries.len(),1);assert_eq!(catalog.pending.len(),24);assert!(catalog.has_more);
  let bytes=serde_json::to_vec(&catalog).unwrap();let mut resumed:TopicCatalog=serde_json::from_slice(&bytes).unwrap();
  topic_batch(&mut resumed,|_,_|Ok(serde_json::json!({"incomplete_results":false,"total_count":1,"items":[{"full_name":"owner/another"}]})));
  assert_eq!(resumed.catalog.entries.len(),2);assert_eq!(resumed.pending.len(),21);assert!(resumed.catalog.errors.iter().all(|e|!e.contains("429")));
 }

 #[test]
 #[ignore = "explicit network smoke; uses a fresh temporary cache"]
 fn live_topic_catalog_and_cached_read(){
  let root=std::env::temp_dir().join(format!("perch-topic-smoke-{}",uuid::Uuid::new_v4()));
  let result=read(&root,true,false).unwrap();
  let catalog=&result.catalog;
  assert!(catalog.errors.iter().all(|e|e.starts_with("已缓存")),"{:?}",catalog.errors);
  assert!(catalog.synced_at>0);assert!(!catalog.entries.is_empty());
  let cached=read(&root,false,false).unwrap().catalog;
  assert_eq!(catalog.entries.len(),cached.entries.len());assert_eq!(catalog.synced_at,cached.synced_at);
  println!("Real GitHub repositories: {}; cache: {}",catalog.entries.len(),root.display());
 }
 #[test]
 fn repository_package_requires_matching_published_identity(){
  let declared=serde_json::json!({"name":"@owner/plugin","version":"1.2.3","dsh":{"bundle":"./bundle.js"}});
  let mut published=declared.clone();published["repository"]=serde_json::json!({"url":"git+https://github.com/owner/plugin.git"});
  assert!(verify_published("owner/plugin",&declared,&published).is_ok());
  assert!(verify_published("fork/plugin",&declared,&published).is_err());
  published["version"]=serde_json::json!("1.2.4");assert!(verify_published("owner/plugin",&declared,&published).is_err());
  let mut private=declared.clone();private["private"]=serde_json::json!(true);assert!(declared_package(&private).is_err());
  assert!(declared_package(&serde_json::json!({"name":"workspace","version":"1.0.0"})).is_err());
  assert!(!valid_repository("owner/../repo"));assert!(!valid_repository("owner/repo?secret"));
 }
 #[test]
 fn topic_partition_uses_github_accepted_year_range(){
  let mut calls=Vec::new();
  let result=partitioned_pages(|range,page|{
   calls.push((range.to_owned(),page));
   Ok(serde_json::json!({"incomplete_results":false,"total_count":0,"items":[]}))
  }).unwrap();
  assert!(result.is_empty());
  assert_eq!(calls,vec![("created:1970-01-01..2970-12-31".to_string(),1)]);
 }
 #[test]
 fn topic_pagination_rejects_partial_and_keeps_forks_separate(){
  let result=pages(|page|Ok(serde_json::json!({"incomplete_results":false,"total_count":2,"items":[{"full_name":if page==1{"owner/plugin"}else{"fork/plugin"}}]}))).unwrap();
  assert_eq!(result.len(),2);assert_ne!(result[0].id,result[1].id);assert!(result[0].package_name.is_none());
  assert!(pages(|_|Ok(serde_json::json!({"incomplete_results":true,"total_count":0,"items":[]}))).is_err());
  assert!(pages(|_|Ok(serde_json::json!({"incomplete_results":false,"total_count":2,"items":[{"full_name":"owner/plugin"}]}))).is_err());
  assert!(pages(|_|Ok(serde_json::json!({"incomplete_results":false,"total_count":1001,"items":[]}))).is_err());
 }
}
