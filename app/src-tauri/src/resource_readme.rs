use serde::Serialize;
use std::{io::Read,time::{Duration,Instant},sync::{LazyLock,Mutex},collections::HashMap};
#[derive(Clone,Serialize)]
#[serde(rename_all="camelCase")]
pub struct Readme { markdown:String, source_url:String, base_url:String, language:String, languages:Vec<String> }
static CACHE:LazyLock<Mutex<HashMap<String,(Instant,Vec<Readme>)>>>=LazyLock::new(Default::default);
fn repository(raw:&str)->Option<String>{
 let url=url::Url::parse(raw.trim_start_matches("git+")).ok()?;
 if url.scheme()!="https"||url.host_str()!=Some("github.com")||url.port().is_some()||!url.username().is_empty()||url.password().is_some(){return None;}
 let parts:Vec<_>=url.path().trim_matches('/').split('/').collect();
 if parts.len()<2||parts[..2].iter().any(|p|p.is_empty()||!p.bytes().all(|c|c.is_ascii_alphanumeric()||b"-_.".contains(&c))){return None;}
 Some(format!("{}/{}",parts[0],parts[1].trim_end_matches(".git")))
}
fn fetch(client:&reqwest::blocking::Client,address:&str,limit:u64)->Result<String,String>{
 let response=client.get(address).send().map_err(|_|"无法读取 README，请检查网络后重试")?;
 if !response.status().is_success(){return Err(format!("README 暂不可用（HTTP {}）",response.status().as_u16()));}
 let mut bytes=Vec::new();response.take(limit+1).read_to_end(&mut bytes).map_err(|_|"README 读取中断")?;
 if bytes.len() as u64>limit{return Err("README 过大，请在来源网站查看".into());}
 String::from_utf8(bytes).map_err(|_|"README 文本编码无法识别".into())
}
fn language(text:&str)->String{
 // Ignore badge/link URLs; distinguish actual prose rather than a single 中文 link.
 let mut chinese=0usize;let mut latin=0usize;
 for line in text.lines().filter(|line|!line.trim_start().starts_with("![")&&!line.contains("https://")){
  for c in line.chars().take(300){if ('\u{4e00}'..='\u{9fff}').contains(&c){chinese+=1;}else if c.is_ascii_alphabetic(){latin+=1;}}
 }
 if chinese>20&&chinese*8>latin{"zh"}else{"en"}.into()
}
fn file_language(path:&str)->Option<&'static str>{
 let name=path.rsplit('/').next()?.to_lowercase();if !name.starts_with("readme")||!(name.ends_with(".md")||name.ends_with(".markdown")){return None;}
 let suffix=name.trim_start_matches("readme").trim_end_matches(".markdown").trim_end_matches(".md").trim_matches(['.','_','-']);
 if suffix.contains("zh")||suffix=="cn"||suffix.contains("中文"){Some("zh")}else if suffix=="en"||suffix.starts_with("en-"){Some("en")}else if suffix.is_empty(){Some("default")}else{None}
}
fn github_docs(client:&reqwest::blocking::Client,repo:&str)->Result<Vec<Readme>,String>{
 let listing:serde_json::Value=fetch(client,&format!("https://api.github.com/repos/{repo}/contents"),2*1024*1024).ok().and_then(|text|serde_json::from_str(&text).ok()).unwrap_or(serde_json::json!([]));
 let mut files:Vec<(String,String,String)>=listing.as_array().ok_or("仓库目录不可用")?.iter().filter_map(|entry|{
  let path=entry["path"].as_str()?;let kind=file_language(path)?;let raw=entry["download_url"].as_str()?;
  if !raw.starts_with("https://raw.githubusercontent.com/"){return None;}
  Some((kind.into(),path.into(),raw.into()))
 }).collect();
 if files.is_empty(){files.push(("default".into(),"README.md".into(),format!("https://raw.githubusercontent.com/{repo}/HEAD/README.md")));}
 files.sort_by_key(|(kind,path,_)|(match kind.as_str(){"default"=>0,"zh"=>1,_=>2},path.len()));
 let mut seen=std::collections::HashSet::new();files.retain(|(kind,_,_)|seen.insert(kind.clone()));
 let mut docs=Vec::new();let mut failure=None;
 for (_,path,address) in files.into_iter().take(3){match fetch(client,&address,512*1024){
  Ok(markdown)=>{let lang=language(&markdown);docs.push(Readme{markdown,source_url:format!("https://github.com/{repo}/blob/HEAD/{path}"),base_url:address,language:lang,languages:vec![]});},Err(error)=>failure=Some(error)
 }}
 // Some projects keep a translated README in docs/ and link it from the root README.
 if !docs.iter().any(|doc|doc.language=="zh"){
  let link=docs.iter().flat_map(|doc|doc.markdown.split("](").skip(1).filter_map(|part|part.split(')').next()).map(move|link|(doc.base_url.clone(),link.split_whitespace().next().unwrap_or("").to_string()))).find(|(_,link)|file_language(link)==Some("zh"));
  if let Some((base,link))=link{if let Ok(url)=url::Url::parse(&base).and_then(|u|u.join(&link)){
   if url.scheme()=="https"&&url.host_str()==Some("raw.githubusercontent.com")&&url.path().starts_with(&format!("/{repo}/")){
    if let Ok(markdown)=fetch(client,url.as_str(),512*1024){let language=language(&markdown);docs.push(Readme{markdown,source_url:url.to_string(),base_url:url.to_string(),language,languages:vec![]});}
   }
  }}
 }
 if docs.is_empty(){return Err(failure.unwrap_or("此仓库未提供可读取的 README".into()));} Ok(docs)
}
fn load(package:Option<String>,repo:Option<String>,preferred:Option<String>)->Result<Readme,String>{
 let key=format!("{}|{}",package.as_deref().unwrap_or(""),repo.as_deref().unwrap_or(""));
 let cached=CACHE.lock().ok().and_then(|cache|cache.get(&key).filter(|(time,_)|time.elapsed()<Duration::from_secs(900)).map(|(_,docs)|docs.clone()));
 let mut docs=if let Some(docs)=cached{docs}else{
  let client=reqwest::blocking::Client::builder().user_agent("Perch-readme").timeout(Duration::from_secs(15)).redirect(reqwest::redirect::Policy::none()).build().map_err(|_|"README 请求初始化失败")?;
  let mut repo=repo.as_deref().and_then(repository);let mut npm=None;
  // Prefer the author's repository variants; registry README is the fallback.
  if repo.is_none(){if let Some(package)=package.as_ref().filter(|p|!p.is_empty()){
   if !crate::engine_profile::valid_package_name(package){return Err("npm 包名无效".into());}
   let doc:serde_json::Value=serde_json::from_str(&fetch(&client,&format!("https://registry.npmjs.org/{}",package.replace('/',"%2F")),8*1024*1024)?).map_err(|_|"npm 返回内容无法识别")?;
   repo=doc["repository"]["url"].as_str().or_else(||doc["repository"].as_str()).and_then(repository);
   if let Some(markdown)=doc["readme"].as_str().filter(|s|!s.is_empty()&&!s.starts_with("ERROR:")){
    if markdown.len()<=512*1024{let source=format!("https://www.npmjs.com/package/{package}");npm=Some(Readme{markdown:markdown.into(),source_url:source.clone(),base_url:repo.as_ref().map(|r|format!("https://github.com/{r}")).unwrap_or(source),language:language(markdown),languages:vec![]});}
   }
  }}
  let docs=match repo{Some(repo)=>match github_docs(&client,&repo){Ok(docs)=>docs,Err(error)=>match npm{Some(doc)=>vec![doc],None=>return Err(error)}},None=>vec![npm.ok_or("此资源未提供可读取的 README")?]};
  if let Ok(mut cache)=CACHE.lock(){if cache.len()>=32{cache.retain(|_,(time,_)|time.elapsed()<Duration::from_secs(900));if cache.len()>=32{cache.clear();}}cache.insert(key,(Instant::now(),docs.clone()));}docs
 };
 let languages:Vec<String>=["zh","en"].iter().filter(|lang|docs.iter().any(|doc|doc.language==**lang)).map(|s|s.to_string()).collect();
 let wanted=preferred.as_deref().unwrap_or("zh");let index=docs.iter().position(|doc|doc.language==wanted).unwrap_or(0);
 let mut doc=docs.swap_remove(index);doc.languages=languages;Ok(doc)
}
#[tauri::command]
pub async fn resource_readme(package_name:Option<String>,repository:Option<String>,language:Option<String>)->Result<Readme,String>{
 tauri::async_runtime::spawn_blocking(move||load(package_name,repository,language)).await.map_err(|_|"README 读取任务中断")?
}
#[cfg(test)] mod tests{use super::*;
 #[test] fn languages_and_repository(){assert_eq!(file_language("README.zh-CN.md"),Some("zh"));assert_eq!(file_language("docs/README_CN.md"),Some("zh"));assert_eq!(file_language("README.en.md"),Some("en"));assert_eq!(language("English content with a 中文 link"),"en");assert_eq!(language(&"这是项目的中文介绍，包含安装和使用方法。".repeat(5)),"zh");assert_eq!(repository("https://github.com/owner/repo.git"),Some("owner/repo".into()));assert!(repository("https://github.com@evil.com/owner/repo").is_none());}
}
