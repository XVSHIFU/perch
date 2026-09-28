use serde::{Deserialize,Serialize};
use serde_json::Value;
use std::{fs,io::Read,path::Path,time::Duration};
use tauri::Manager;
#[derive(Clone,Serialize,Deserialize)]
#[serde(rename_all="camelCase")]
pub struct SkillSource {pub repository:String,pub revision:String,pub commit:String,pub paths:Vec<String>,pub synced_at:u64,pub error:Option<String>,#[serde(default="enabled_by_default")]pub enabled:bool}
fn enabled_by_default()->bool{true}
fn validate(repository:&str,revision:&str)->Result<(),String>{
 let pieces:Vec<_>=repository.split('/').collect();
 if pieces.len()!=2||pieces.iter().any(|p|p.is_empty()||*p=="."||*p==".."||!p.bytes().all(|c|c.is_ascii_alphanumeric()||b"._-".contains(&c)))||revision.is_empty()||revision.len()>200||!revision.bytes().all(|c|c.is_ascii_alphanumeric()||b"._/-".contains(&c)){return Err("请填写 owner/repository 和分支、标签或提交".into());}Ok(())
}
fn client()->Result<reqwest::blocking::Client,String>{reqwest::blocking::Client::builder().user_agent("Perch-resource-catalog").timeout(Duration::from_secs(25)).redirect(reqwest::redirect::Policy::none()).build().map_err(|_|"网络初始化失败".into())}
fn read(client:&reqwest::blocking::Client,url:url::Url,limit:u64)->Result<Vec<u8>,String>{
 let response=client.get(url).send().map_err(|_|"无法访问 GitHub 来源")?;
 if !response.status().is_success(){return Err(format!("GitHub HTTP {}（可能离线、限流或来源不可用）",response.status().as_u16()));}
 let mut bytes=Vec::new();response.take(limit+1).read_to_end(&mut bytes).map_err(|_|"来源读取失败")?;if bytes.len() as u64>limit{return Err("来源内容超过大小限制".into());}Ok(bytes)
}
fn endpoint(repository:&str,tail:&[&str])->url::Url{
 let mut url=url::Url::parse("https://api.github.com/repos/").unwrap();{let mut parts=url.path_segments_mut().unwrap();parts.pop_if_empty();for piece in repository.split('/').chain(tail.iter().copied()){parts.push(piece);}}url
}
fn tree(root:&Path,client:&reqwest::blocking::Client,repository:&str,commit:&str)->Result<Vec<Value>,String>{
 let mut url=endpoint(repository,&["git","trees",commit]);url.set_query(Some("recursive=1"));let value=crate::resource_catalog::conditional_json(root,client,url.as_str())?;
 if value["truncated"]==true{return Err("仓库目录被 GitHub 截断，请使用更小的来源仓库".into());}
 value["tree"].as_array().cloned().ok_or("仓库缺少文件目录".into())
}
fn cache_path(root:&Path,repository:&str,revision:&str)->std::path::PathBuf{root.join("skill-sources").join(repository).join(format!("{}.json",revision.bytes().map(|byte|format!("{byte:02x}")).collect::<String>()))}
fn sync(root:&Path,repository:String,revision:String,refresh:bool)->Result<SkillSource,String>{
 validate(&repository,&revision)?;let path=cache_path(root,&repository,&revision);
 let cached:Option<SkillSource>=fs::read(&path).ok().and_then(|bytes|serde_json::from_slice(&bytes).ok());
 if cached.as_ref().is_some_and(|source|!source.enabled)||!refresh {if let Some(cached)=cached{return Ok(cached);}}
 let result=(||{
  let client=client()?;let commit=crate::resource_catalog::conditional_json(root,&client,endpoint(&repository,&["commits",&revision]).as_str())?;
  let commit=commit["sha"].as_str().filter(|sha|sha.len()==40&&sha.bytes().all(|c|c.is_ascii_hexdigit())).ok_or("来源缺少固定提交")?.to_string();
  let rows=tree(root,&client,&repository,&commit)?;
  let mut paths:Vec<_>=rows.iter().filter(|row|row["type"]=="blob"&&row["mode"]=="100644").filter_map(|row|row["path"].as_str()).filter_map(|path|if path=="SKILL.md"{Some("".to_string())}else{path.strip_suffix("/SKILL.md").map(str::to_string)}).collect();paths.sort();
  let source=SkillSource{repository:repository.clone(),revision:revision.clone(),commit,paths,synced_at:std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_secs(),error:None,enabled:true};
  fs::create_dir_all(path.parent().unwrap()).map_err(|_|"无法创建来源缓存")?;crate::resource_catalog::save_cache(&path,&serde_json::to_vec(&source).map_err(|_|"来源编码失败")?)?;Ok(source)
 })();
 source_result(&path,cached,result)
}
fn source_result(path:&Path,cached:Option<SkillSource>,result:Result<SkillSource,String>)->Result<SkillSource,String>{
 match result {Ok(source)=>Ok(source),Err(error)=>if let Some(mut cached)=cached{cached.error=Some(error);crate::resource_catalog::save_cache(&path,&serde_json::to_vec(&cached).map_err(|_|"来源编码失败")?)?;Ok(cached)}else{Err(error)}}
}
fn saved_sources(root:&Path)->Result<Vec<SkillSource>,String>{
 let base=root.join("skill-sources");if !base.exists(){return Ok(vec![]);}
 let mut sources=Vec::new();
 for owner in fs::read_dir(&base).map_err(|_|"无法读取来源")?{let owner=owner.map_err(|_|"来源条目无效")?;if !owner.file_type().map_err(|_|"来源类型无效")?.is_dir(){continue;}
  for repo in fs::read_dir(owner.path()).map_err(|_|"无法读取来源仓库")?{let repo=repo.map_err(|_|"仓库条目无效")?;if !repo.file_type().map_err(|_|"来源类型无效")?.is_dir(){continue;}
   for file in fs::read_dir(repo.path()).map_err(|_|"无法读取来源缓存")?{let file=file.map_err(|_|"缓存条目无效")?;
    if file.path().extension().and_then(|value|value.to_str())!=Some("json"){continue;}
    if let Ok(bytes)=fs::read(file.path()){if let Ok(source)=serde_json::from_slice::<SkillSource>(&bytes){sources.push(source);}}
   }
  }
 }
 sources.sort_by(|a,b|(&a.repository,&a.revision).cmp(&(&b.repository,&b.revision)));Ok(sources)
}
#[tauri::command]
pub fn github_skill_sources(app:tauri::AppHandle)->Result<Vec<SkillSource>,String>{
 let state=app.state::<crate::data_commands::DataState>();saved_sources(state.root.as_ref().map_err(|_|"工作空间不可用")?)
}
#[tauri::command]
pub fn set_github_skill_source_enabled(app:tauri::AppHandle,repository:String,revision:String,enabled:bool)->Result<SkillSource,String>{
 validate(&repository,&revision)?;let state=app.state::<crate::data_commands::DataState>();let root=state.root.as_ref().map_err(|_|"工作空间不可用")?;
 let path=cache_path(root,&repository,&revision);let mut source:SkillSource=serde_json::from_slice(&fs::read(&path).map_err(|_|"请先添加此来源")?).map_err(|_|"来源缓存无效")?;
 source.enabled=enabled;fs::write(path,serde_json::to_vec(&source).map_err(|_|"来源编码失败")?).map_err(|_|"无法保存来源状态")?;Ok(source)
}
#[tauri::command]
pub async fn github_skill_source(app:tauri::AppHandle,repository:String,revision:String,refresh:bool)->Result<SkillSource,String>{
 let root=app.state::<crate::data_commands::DataState>().root.as_ref().map_err(|_|"工作空间不可用")?.clone();
 tauri::async_runtime::spawn_blocking(move||sync(&root,repository,revision,refresh)).await.map_err(|_|"来源同步中断".to_string())?
}
fn import(root:&Path,source:SkillSource,skill_path:String)->Result<Value,String>{
 validate(&source.repository,&source.revision)?;
 if source.commit.len()!=40||!source.commit.bytes().all(|c|c.is_ascii_hexdigit()){return Err("请先刷新来源固定提交".into());}
 let client=client()?;let rows=tree(root,&client,&source.repository,&source.commit)?;
 let prefix=if skill_path.is_empty(){String::new()}else{format!("{skill_path}/")};
 if rows.iter().any(|row|row["type"]=="commit"&&row["path"].as_str().is_some_and(|path|path.starts_with(&prefix))){return Err("Skill 含子模块，无法完整导入，请提供已展开的资源目录".into());}
 let selected:Vec<_>=rows.iter().filter(|row|row["type"]=="blob"&&row["path"].as_str().is_some_and(|path|path.starts_with(&prefix))).collect();
 if selected.len()>1000||!selected.iter().any(|row|row["path"]==format!("{prefix}SKILL.md")){return Err("Skill 目录无效或文件过多".into());}
 let id=format!("local-skill-{}",uuid::Uuid::new_v4());let staging=root.join(format!("skill-download-{}",uuid::Uuid::new_v4()));
 let result=(||{
  let mut budget=0u64;
  for row in selected {
   if row["mode"]!="100644"&&row["mode"]!="100755"{return Err("Skill 含链接或不支持的文件".into());}
   let path=row["path"].as_str().ok_or("文件路径无效")?;let relative=path.strip_prefix(&prefix).ok_or("文件越界")?;
   if !crate::pack_archive::safe_name(relative){return Err("Skill 文件路径不支持".into());}
   let size=row["size"].as_u64().ok_or("来源未声明文件大小")?;budget=budget.checked_add(size).ok_or("Skill 过大")?;if budget>32*1024*1024{return Err("Skill 超过 32 MB 上限".into());}
   let mut url=url::Url::parse("https://raw.githubusercontent.com/").unwrap();{let mut parts=url.path_segments_mut().unwrap();parts.pop_if_empty();for piece in source.repository.split('/').chain(std::iter::once(source.commit.as_str())).chain(path.split('/')){parts.push(piece);}}
   let bytes=read(&client,url,size)?;if bytes.len() as u64!=size{return Err("Skill 文件大小不符".into());}
   let target=staging.join("content").join(relative);fs::create_dir_all(target.parent().unwrap()).map_err(|_|"无法创建 Skill 下载目录")?;fs::write(target,bytes).map_err(|_|"Skill 写入失败")?;
  }
  let name=skill_path.rsplit('/').next().filter(|name|!name.is_empty()).unwrap_or(&source.repository);
  let metadata=serde_json::json!({"id":id,"name":name,"version":"1","kind":"skill","source":format!("https://github.com/{}/tree/{}/{}",source.repository,source.commit,skill_path),"commit":source.commit,"declarations":crate::local_skills::declarations(&staging.join("content")),"licenseFiles":crate::local_skills::license_files(&staging.join("content")),"digest":crate::local_skills::content_digest(&staging.join("content"))?});
  fs::write(staging.join("metadata.json"),metadata.to_string()).map_err(|_|"Skill 元数据保存失败")?;
  let destination=root.join("local-skills").join(&id);fs::create_dir_all(destination.parent().unwrap()).map_err(|_|"无法创建资源目录")?;fs::rename(&staging,destination).map_err(|_|"Skill 导入失败")?;Ok(metadata)
 })();
 if result.is_err(){let _=fs::remove_dir_all(staging);}result
}
#[tauri::command]
pub async fn import_github_skill(app:tauri::AppHandle,source:SkillSource,skill_path:String)->Result<Value,String>{
 let root=app.state::<crate::data_commands::DataState>().root.as_ref().map_err(|_|"工作空间不可用")?.clone();
 tauri::async_runtime::spawn_blocking(move||import(&root,source,skill_path)).await.map_err(|_|"Skill 下载任务中断".to_string())?
}

#[cfg(test)]
mod tests {
 use super::*;
 #[test]
 fn disabled_sources_and_failed_refresh_preserve_cache(){
  let root=std::env::temp_dir().join(format!("perch-source-state-{}",uuid::Uuid::new_v4()));
  let path=cache_path(&root,"example/skills","main");fs::create_dir_all(path.parent().unwrap()).unwrap();
  let source=SkillSource{repository:"example/skills".into(),revision:"main".into(),commit:"a".repeat(40),paths:vec!["skills/sample".into()],synced_at:123,error:None,enabled:false};
  fs::write(&path,serde_json::to_vec(&source).unwrap()).unwrap();
  let disabled=sync(&root,"example/skills".into(),"main".into(),true).unwrap();assert!(!disabled.enabled);assert_eq!(disabled.synced_at,123);
  let failed=source_result(&path,Some(source),Err("offline".into())).unwrap();assert_eq!(failed.paths,vec!["skills/sample"]);assert_eq!(failed.synced_at,123);
  let listed=saved_sources(&root).unwrap();assert_eq!(listed.len(),1);assert_eq!(listed[0].error.as_deref(),Some("offline"));
  fs::remove_dir_all(root).unwrap();
 }
 #[test]
 #[ignore="reads a public GitHub Skill repository and downloads one complete resource"]
 fn real_github_skill_import(){
  let root=std::env::temp_dir().join(format!("perch-github-skills-{}",uuid::Uuid::new_v4()));
  let source=sync(&root,"anthropics/skills".into(),"main".into(),true).unwrap();
  assert!(source.paths.iter().any(|path|path=="skills/skill-creator"));
  let path=source.paths.iter().find(|path|path.ends_with("brand-guidelines")).unwrap().clone();
  let imported=import(&root,source.clone(),path).unwrap();
  let skill=root.join("local-skills").join(imported["id"].as_str().unwrap()).join("content/SKILL.md");
  assert!(fs::metadata(skill).unwrap().len()>0);
  let cached=sync(&root,"anthropics/skills".into(),"main".into(),false).unwrap();assert_eq!(cached.commit,source.commit);
  println!("GitHub Skill imported from {} into {}",source.commit,root.display());
 }
}
