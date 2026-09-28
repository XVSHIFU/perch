use std::{fs,path::Path};
use tauri::Manager;
// Display upstream declarations only; these are not permissions or a compatibility verdict.
pub(crate) fn declarations(content:&Path)->serde_json::Value{
 use std::io::Read;
 let result=(||->Result<serde_json::Value,String>{
  let path=content.join("SKILL.md");
  let info=fs::symlink_metadata(&path).map_err(|_|"无法读取 SKILL.md")?;
  if !info.is_file()||info.file_type().is_symlink(){return Err("SKILL.md 不是普通文件".into());}
  #[cfg(windows)]{use std::os::windows::fs::MetadataExt;if info.file_attributes()&0x400!=0{return Err("SKILL.md 是链接".into());}}
  let mut bytes=Vec::new();fs::File::open(path).map_err(|_|"无法读取 SKILL.md")?.take(65537).read_to_end(&mut bytes).map_err(|_|"无法读取 SKILL.md")?;
  let text=String::from_utf8_lossy(&bytes);let mut lines=text.trim_start_matches('\u{feff}').lines();
  if lines.next().map(str::trim)!=Some("---"){return Err("未找到 YAML 声明，请查看 Skill 原文要求".into());}
  let mut header=String::new();let mut closed=false;
  for line in lines{if matches!(line.trim_end(),"---"|"..."){closed=true;break;}header.push_str(line);header.push('\n');}
  if !closed{return Err("YAML 声明未闭合或超过 64 KB，请查看原文".into());}
  let value:serde_yaml_ng::Value=serde_yaml_ng::from_str(&header).map_err(|_|"YAML 声明无法解析，请查看原文")?;
  let map=value.as_mapping().ok_or("YAML 声明应为字段映射")?;
  let mut output=serde_json::Map::new();let mut warnings=Vec::new();
  for key in ["name","description","license","compatibility","allowed-tools"]{
   if let Some(value)=map.get(serde_yaml_ng::Value::String(key.into())){
    if let Some(text)=value.as_str(){output.insert(key.into(),serde_json::json!(text));}
    else{warnings.push(format!("{key} 不是文本，请查看原文"));}
   }
  }
  if !warnings.is_empty(){output.insert("warning".into(),serde_json::json!(warnings.join("；")));}
  Ok(serde_json::Value::Object(output))
 })();
 result.unwrap_or_else(|warning|serde_json::json!({"warning":warning}))
}
pub(crate) fn license_files(content:&Path)->Vec<String>{
 let Ok(entries)=fs::read_dir(content)else{return vec![];};
 let mut names=entries.filter_map(|entry|{
  let entry=entry.ok()?;if !entry.file_type().ok()?.is_file(){return None;}
  let name=entry.file_name().into_string().ok()?;
  let stem=name.split('.').next().unwrap_or("").to_ascii_uppercase();
  if matches!(stem.as_str(),"LICENSE"|"LICENCE"|"COPYING"|"NOTICE"){Some(name)}else{None}
 }).collect::<Vec<_>>();names.sort();names
}
pub(crate) fn content_digest(root:&Path)->Result<String,String>{
 use sha2::{Digest,Sha256};use std::io::Read;
 let mut paths=vec![(root.to_path_buf(),String::new(),0usize)];let mut files=std::collections::BTreeMap::new();let mut count=0;let mut budget=0u64;
 while let Some((path,relative,depth))=paths.pop(){
  count+=1;if count>10000||depth>32{return Err("Skill 目录过大或过深".into());}
  let metadata=fs::symlink_metadata(&path).map_err(|_|"无法读取 Skill 内容")?;
  #[cfg(windows)]{use std::os::windows::fs::MetadataExt;if metadata.file_attributes()&0x400!=0{return Err("Skill 内容包含链接".into());}}
  if metadata.file_type().is_symlink(){return Err("Skill 内容包含链接".into());}
  if metadata.is_dir(){for entry in fs::read_dir(&path).map_err(|_|"无法读取 Skill 目录")?{let entry=entry.map_err(|_|"Skill 条目不可读")?;let name=entry.file_name().into_string().map_err(|_|"Skill 文件名不支持")?;paths.push((entry.path(),if relative.is_empty(){name}else{format!("{relative}/{name}")},depth+1));}}
  else if metadata.is_file(){budget=budget.checked_add(metadata.len()).ok_or("Skill 过大")?;if budget>32*1024*1024{return Err("Skill 超过 32 MB".into());}files.insert(relative,path);}else{return Err("Skill 文件类型不支持".into());}
 }
 let mut hash=Sha256::new();hash.update(b"perch-skill-v1\0");
 for (name,path) in files{let mut bytes=Vec::new();fs::File::open(path).map_err(|_|"Skill 文件不可读")?.take(32*1024*1024+1).read_to_end(&mut bytes).map_err(|_|"Skill 文件读取失败")?;if bytes.len()>32*1024*1024{return Err("Skill 文件过大".into());}hash.update((name.len() as u64).to_le_bytes());hash.update(name.as_bytes());hash.update((bytes.len() as u64).to_le_bytes());hash.update(bytes);}
 Ok(format!("sha256:{:x}",hash.finalize()))
}
pub fn valid_id(id:&str)->bool{id.strip_prefix("local-skill-").is_some_and(|id|uuid::Uuid::parse_str(id).is_ok())}
pub fn validate_selection(root:&Path,recipe:&crate::catalog::Recipe)->crate::store::Result<()> {
 for item in recipe.extensions.iter().filter(|item|valid_id(&item.id)) {
  let base=root.join("local-skills").join(&item.id);
  let metadata=fs::read(base.join("metadata.json")).ok().and_then(|bytes|serde_json::from_slice::<serde_json::Value>(&bytes).ok());
  if !base.join("content/SKILL.md").is_file()||!metadata.as_ref().is_some_and(|value|value["id"]==item.id&&value["version"]==item.version){
   return Err(crate::store::Failure::new("SKILL_MISSING","组合引用的本地 Skill 内容或版本缺失，请重新导入或移除该资源"));
  }
  if let Some(expected)=metadata.as_ref().and_then(|value|value["digest"].as_str()){
   if content_digest(&base.join("content")).map_err(|message|crate::store::Failure::new("SKILL_CONTENT",&message))?!=expected{return Err(crate::store::Failure::new("SKILL_CHANGED","导入的 Skill 内容已变化，请重新导入为独立资源后再应用"));}
  }
 }
 Ok(())
}
pub fn copy_tree(source:&Path,target:&Path,budget:&mut u64)->Result<(),String>{
 copy_tree_bounded(source,target,budget,&mut 0,0)
}
fn copy_tree_bounded(source:&Path,target:&Path,budget:&mut u64,entries:&mut usize,depth:usize)->Result<(),String>{
 *entries+=1;
 if *entries>10000||depth>32{return Err("Skill 目录超过 10000 个条目或 32 层深度上限".into());}
 let info=fs::symlink_metadata(source).map_err(|_|"无法读取 Skill 文件")?;
 #[cfg(windows)]{use std::os::windows::fs::MetadataExt;if info.file_attributes()&0x400!=0{return Err("Skill 目录包含链接，不能导入".into());}}
 if info.file_type().is_symlink(){return Err("Skill 目录包含链接，不能导入".into());}
 if info.is_dir(){fs::create_dir_all(target).map_err(|_|"无法创建 Skill 目录")?;for entry in fs::read_dir(source).map_err(|_|"无法枚举 Skill 目录")?{let entry=entry.map_err(|_|"无法读取 Skill 条目")?;if entry.file_name()==".git"{continue;}copy_tree_bounded(&entry.path(),&target.join(entry.file_name()),budget,entries,depth+1)?;}}
 else if info.is_file(){*budget=budget.checked_add(info.len()+1).ok_or("Skill 过大")?;if *budget>32*1024*1024{return Err("Skill 超过 32 MB 导入上限".into());}fs::copy(source,target).map_err(|_|"复制 Skill 失败")?;}
 else{return Err("Skill 含不支持的文件类型".into());}Ok(())
}
// Publish a complete copy only; a failed copy must never be loaded on retry.
pub fn copy_atomic(source:&Path,target:&Path)->Result<(),String>{
 if target.exists(){return Err("目标 Skill 已存在".into());}
 let parent=target.parent().ok_or("Skill 目标目录无效")?;
 fs::create_dir_all(parent).map_err(|_|"无法创建 Skill 父目录")?;
 let staging=parent.join(format!(".skill-staging-{}",uuid::Uuid::new_v4()));
 let result=(||{copy_tree(source,&staging,&mut 0)?;fs::rename(&staging,target).map_err(|_|"无法完成 Skill 复制".to_string())})();
 if result.is_err(){let _=fs::remove_dir_all(&staging);}
 result
}
#[tauri::command]
pub async fn import_local_skill(app:tauri::AppHandle)->Result<Option<serde_json::Value>,String>{
 let root=app.state::<crate::data_commands::DataState>().root.as_ref().map_err(|_|"工作空间不可用")?.clone();
 tauri::async_runtime::spawn_blocking(move||{
  let Some(source)=rfd::FileDialog::new().set_title("选择包含 SKILL.md 的完整目录").pick_folder()else{return Ok(None);};
  if !source.join("SKILL.md").is_file(){return Err("所选目录缺少 SKILL.md".into());}
  let id=format!("local-skill-{}",uuid::Uuid::new_v4());
  let name=source.file_name().unwrap_or_default().to_string_lossy().to_string();
  let target=root.join("local-skills").join(&id);
  copy_atomic(&source,&target.join("content"))?;
  let metadata=serde_json::json!({"id":id,"name":name,"version":"1","kind":"skill","source":"本地导入","declarations":declarations(&target.join("content")),"licenseFiles":license_files(&target.join("content")),"digest":content_digest(&target.join("content"))?});
  fs::write(target.join("metadata.json"),serde_json::to_vec(&metadata).map_err(|_|"Skill 元数据无效")?).map_err(|_|"无法保存 Skill 元数据")?;
  Ok(Some(metadata))
 }).await.map_err(|_|"Skill 导入任务中断".to_string())?
}
fn zip_files(bytes:&[u8])->Result<(std::collections::BTreeMap<String,Vec<u8>>,String),String>{
 use std::io::{Cursor,Read};
 if bytes.len()>32*1024*1024{return Err("Skill ZIP 超过 32 MB 上限".into());}
 let mut archive=zip::ZipArchive::new(Cursor::new(bytes)).map_err(|_|"无法读取 Skill ZIP")?;
 if archive.len()>10000{return Err("Skill ZIP 条目数量超过上限".into());}
 let mut files=std::collections::BTreeMap::new();let mut seen=std::collections::HashSet::new();let mut budget=0u64;
 for index in 0..archive.len(){
  let mut file=archive.by_index(index).map_err(|_|"无法读取 ZIP 条目")?;
  let name=file.name().trim_end_matches('/').to_string();
  if !crate::pack_archive::safe_name(&name)||name.split('/').count()>32||!seen.insert(name.to_lowercase())||file.unix_mode().is_some_and(|mode|mode&0o170000==0o120000){return Err("Skill ZIP 包含不安全、重复或过深路径，未导入".into());}
  if file.is_dir(){continue;}
  budget=budget.checked_add(file.size()).ok_or("Skill 过大")?;if budget>32*1024*1024{return Err("Skill 解压内容超过 32 MB 上限".into());}
  let mut content=Vec::new();file.by_ref().take(32*1024*1024+1).read_to_end(&mut content).map_err(|_|"Skill 内容读取失败")?;
  if content.len() as u64!=file.size(){return Err("Skill 文件大小不一致".into());}files.insert(name,content);
 }
 let roots:Vec<_>=files.keys().filter(|name|name.rsplit('/').next()==Some("SKILL.md")).cloned().collect();
 if roots.len()!=1{return Err("请选择只包含一个完整 Skill 的 ZIP；需要且只能有一个 SKILL.md".into());}
 let prefix=roots[0].strip_suffix("SKILL.md").unwrap();
 if files.keys().any(|name|!name.starts_with(prefix)){return Err("ZIP 包含 Skill 目录之外的文件，请把完整 Skill 目录单独压缩".into());}
 let prefix=prefix.to_string();Ok((files,prefix))
}
pub(crate) fn import_zip(root:&Path,bytes:&[u8])->Result<serde_json::Value,String>{import_zip_from(root,bytes,"本地 ZIP")}
fn import_zip_from(root:&Path,bytes:&[u8],source:&str)->Result<serde_json::Value,String>{
 let (files,prefix)=zip_files(bytes)?;
 let id=format!("local-skill-{}",uuid::Uuid::new_v4());
 let metadata=serde_json::json!({"id":id,"name":prefix.trim_end_matches('/').rsplit('/').next().filter(|name|!name.is_empty()).unwrap_or("导入的 Skill"),"version":"1","kind":"skill","source":source});
 let parent=root.join("local-skills");fs::create_dir_all(&parent).map_err(|_|"无法创建资源目录")?;
 let staging=parent.join(format!(".skill-staging-{}",uuid::Uuid::new_v4()));
 let result=(||{
  for (name,content) in files.iter(){let path=staging.join("content").join(name.strip_prefix(prefix.as_str()).unwrap());fs::create_dir_all(path.parent().unwrap()).map_err(|_|"无法创建 Skill 文件目录")?;fs::write(path,content).map_err(|_|"无法写入 Skill 文件")?;}
  let mut metadata=metadata;metadata["declarations"]=declarations(&staging.join("content"));metadata["digest"]=serde_json::Value::String(content_digest(&staging.join("content"))?);metadata["licenseFiles"]=serde_json::json!(license_files(&staging.join("content")));
  fs::write(staging.join("metadata.json"),metadata.to_string()).map_err(|_|"无法保存 Skill 元数据")?;
  fs::rename(&staging,parent.join(&id)).map_err(|_|"无法完成 Skill 导入")?;Ok(metadata)
 })();
 if result.is_err(){let _=fs::remove_dir_all(staging);}result
}
fn skill_zip_url(value:&str)->Result<url::Url,String>{
 let url=url::Url::parse(value).map_err(|_|"请填写完整 HTTPS ZIP 地址")?;
 if url.scheme()!="https"||url.host_str().is_none()||!url.username().is_empty()||url.password().is_some()||url.query().is_some()||url.fragment().is_some(){return Err("仅支持不含账号、查询参数和片段的 HTTPS 直达 ZIP 地址".into());}Ok(url)
}
#[tauri::command]
pub async fn preview_skill_zip_url(app:tauri::AppHandle,address:String)->Result<serde_json::Value,String>{
 let root=app.state::<crate::data_commands::DataState>().root.as_ref().map_err(|_|"工作空间不可用")?.clone();let url=skill_zip_url(&address)?;
 tauri::async_runtime::spawn_blocking(move||{
  use std::io::Read;use sha2::{Digest,Sha256};
  let response=reqwest::blocking::Client::builder().timeout(std::time::Duration::from_secs(30)).redirect(reqwest::redirect::Policy::none()).build().map_err(|_|"下载初始化失败")?.get(url).send().map_err(|_|"无法下载 Skill ZIP")?;
  if !response.status().is_success(){return Err(format!("来源返回 HTTP {}；请提供直达 ZIP 文件的地址",response.status().as_u16()));}
  let mut bytes=Vec::new();response.take(32*1024*1024+1).read_to_end(&mut bytes).map_err(|_|"Skill ZIP 下载失败")?;
  let (files,prefix)=zip_files(&bytes)?;let size:usize=files.values().map(Vec::len).sum();let digest=format!("{:x}",Sha256::digest(&bytes));
  let id=uuid::Uuid::new_v4().to_string();let cache=root.join("resource-downloads");fs::create_dir_all(&cache).map_err(|_|"无法缓存下载文件")?;
  fs::write(cache.join(format!("{id}.zip")),&bytes).map_err(|_|"无法保存下载文件")?;
  fs::write(cache.join(format!("{id}.json")),serde_json::json!({"address":address}).to_string()).map_err(|_|"无法保存下载来源")?;
  Ok(serde_json::json!({"id":id,"address":address,"digest":digest,"bytes":size,"files":files.keys().map(|name|name.strip_prefix(prefix.as_str()).unwrap()).collect::<Vec<_>>()}))
 }).await.map_err(|_|"ZIP 下载任务中断".to_string())?
}
#[tauri::command]
pub async fn import_downloaded_skill(app:tauri::AppHandle,id:String,digest:String)->Result<serde_json::Value,String>{
 uuid::Uuid::parse_str(&id).map_err(|_|"下载标识无效")?;let root=app.state::<crate::data_commands::DataState>().root.as_ref().map_err(|_|"工作空间不可用")?.clone();
 tauri::async_runtime::spawn_blocking(move||{
  use std::io::Read;use sha2::{Digest,Sha256};
  let path=root.join("resource-downloads").join(format!("{id}.zip"));let mut bytes=Vec::new();fs::File::open(&path).map_err(|_|"下载缓存已不存在，请重新读取来源")?.take(32*1024*1024+1).read_to_end(&mut bytes).map_err(|_|"下载缓存不可读")?;
  if format!("{:x}",Sha256::digest(&bytes))!=digest{return Err("下载内容已变化，请重新预览".into());}
  let metadata_path=path.with_extension("json");let metadata:serde_json::Value=serde_json::from_slice(&fs::read(&metadata_path).map_err(|_|"下载来源记录缺失")?).map_err(|_|"下载来源记录无效")?;
  let address=metadata["address"].as_str().ok_or("下载来源地址缺失")?;skill_zip_url(address)?;
  let result=import_zip_from(&root,&bytes,address)?;let _=fs::remove_file(path);let _=fs::remove_file(metadata_path);Ok(result)
 }).await.map_err(|_|"ZIP 导入任务中断".to_string())?
}
#[tauri::command]
pub async fn import_skill_zip(app:tauri::AppHandle)->Result<Option<serde_json::Value>,String>{
 let root=app.state::<crate::data_commands::DataState>().root.as_ref().map_err(|_|"工作空间不可用")?.clone();
 tauri::async_runtime::spawn_blocking(move||{
  use std::io::Read;
  let Some(path)=rfd::FileDialog::new().set_title("导入完整 Skill ZIP").add_filter("Skill ZIP",&["zip"]).pick_file()else{return Ok(None);};
  let mut bytes=Vec::new();fs::File::open(path).map_err(|_|"无法打开 ZIP")?.take(32*1024*1024+1).read_to_end(&mut bytes).map_err(|_|"无法读取 ZIP")?;
  import_zip(&root,&bytes).map(Some)
 }).await.map_err(|_|"Skill ZIP 导入任务中断".to_string())?
}
#[tauri::command]
pub fn local_skills(app:tauri::AppHandle)->Result<Vec<serde_json::Value>,String>{
 let state=app.state::<crate::data_commands::DataState>();let root=state.root.as_ref().map_err(|_|"工作空间不可用")?.join("local-skills");
 if !root.exists(){return Ok(vec![]);}
 let mut items=vec![];for entry in fs::read_dir(root).map_err(|_|"无法读取 Skill 目录")?{let entry=entry.map_err(|_|"无法读取 Skill")?;if let Ok(bytes)=fs::read(entry.path().join("metadata.json")){if let Ok(mut item)=serde_json::from_slice::<serde_json::Value>(&bytes){if item.is_object(){item["declarations"]=declarations(&entry.path().join("content"));item["licenseFiles"]=serde_json::json!(license_files(&entry.path().join("content")));items.push(item);}}}}Ok(items)
}

#[cfg(test)]
mod tests {
 use super::*;
 #[test]
 fn skill_declarations_preserve_multiline_and_report_invalid_headers(){
  let root=std::env::temp_dir().join(format!("perch-skill-declarations-{}",uuid::Uuid::new_v4()));fs::create_dir_all(&root).unwrap();
  let path=root.join("SKILL.md");
  fs::write(&path,"\u{feff}---\r\nname: sample\r\ndescription: >-\r\n  A complete\r\n  description\r\ncompatibility: 'Requires Python 3 and Linux'\r\nallowed-tools: Bash(git:*) Read\r\nlicense: MIT # author declaration\r\n---\r\nIgnore this body").unwrap();
  let value=declarations(&root);assert_eq!(value["description"],"A complete description");assert_eq!(value["compatibility"],"Requires Python 3 and Linux");assert_eq!(value["license"],"MIT");assert_eq!(value["allowed-tools"],"Bash(git:*) Read");assert!(value.get("warning").is_none());
  fs::write(&path,"---\ndescription: |-\n  ---\n  text\nlicense: MIT\n---\n").unwrap();assert_eq!(declarations(&root)["description"],"---\ntext");
  fs::write(&path,"---\ncompatibility: [linux]\n---\n").unwrap();let value=declarations(&root);assert!(value.get("compatibility").is_none());assert!(value["warning"].as_str().unwrap().contains("compatibility"));
  for input in ["no header".to_string(),"---\nlicense: [\n---".into(),format!("---\n{}\n---","x".repeat(66000)),"---\nlicense: MIT\nlicense: BSD\n---".into()]{fs::write(&path,input).unwrap();assert!(declarations(&root)["warning"].is_string());}
  fs::remove_dir_all(root).unwrap();
 }
 #[test]
 fn https_skill_source_rejects_credential_urls(){
  assert!(skill_zip_url("https://example.org/skill.zip").is_ok());
  for address in ["http://example.org/skill.zip","https://user:secret@example.org/skill.zip","https://example.org/skill.zip?key=secret","file:///tmp/a.zip"]{assert!(skill_zip_url(address).is_err());}
 }

 #[test]
 fn skill_digest_detects_changes_and_preserves_legacy(){
  let root=std::env::temp_dir().join(format!("perch-skill-digest-{}",uuid::Uuid::new_v4()));let id=format!("local-skill-{}",uuid::Uuid::new_v4());let base=root.join("local-skills").join(&id);let content=base.join("content");fs::create_dir_all(&content).unwrap();fs::write(content.join("SKILL.md"),"original").unwrap();
  let mut recipe=crate::catalog::packs().remove(0);recipe.extensions.push(crate::catalog::ExtensionRef{resource_rules:Default::default(),disabled_resources:Vec::new(),id:id.clone(),version:"1".into(),enabled:true});
  let digest=content_digest(&content).unwrap();assert_eq!(digest,content_digest(&content).unwrap());
  fs::write(base.join("metadata.json"),serde_json::json!({"id":id,"version":"1","digest":digest}).to_string()).unwrap();assert!(validate_selection(&root,&recipe).is_ok());
  fs::write(content.join("SKILL.md"),"modified").unwrap();assert_eq!(validate_selection(&root,&recipe).unwrap_err().code,"SKILL_CHANGED");
  fs::write(base.join("metadata.json"),serde_json::json!({"id":id,"version":"1"}).to_string()).unwrap();assert!(validate_selection(&root,&recipe).is_ok());
  fs::remove_dir_all(root).unwrap();
 }

 #[test]
 fn zip_import_complete_and_rejects_traversal(){
  use std::io::{Cursor,Write};
  fn zip(entries:&[(&str,&str)])->Vec<u8>{let mut writer=zip::ZipWriter::new(Cursor::new(Vec::new()));for (name,text) in entries{writer.start_file(*name,zip::write::SimpleFileOptions::default()).unwrap();writer.write_all(text.as_bytes()).unwrap();}writer.finish().unwrap().into_inner()}
  let root=std::env::temp_dir().join(format!("perch-skill-zip-{}",uuid::Uuid::new_v4()));
  let good=zip(&[("sample/SKILL.md","---\nname: sample\ndescription: test\n---\nTest"),("sample/scripts/test.txt","complete")]);
  let result=import_zip(&root,&good).unwrap();let id=result["id"].as_str().unwrap();
  assert_eq!(fs::read_to_string(root.join("local-skills").join(id).join("content/scripts/test.txt")).unwrap(),"complete");
  let before=fs::read_dir(root.join("local-skills")).unwrap().count();
  for bad in [zip(&[("../SKILL.md","bad")]),zip(&[("a/SKILL.md","a"),("b/SKILL.md","b")]),zip(&[("SKILL.md","a"),("skill.md","b")])]{assert!(import_zip(&root,&bad).is_err());}
  assert_eq!(fs::read_dir(root.join("local-skills")).unwrap().count(),before);
  fs::remove_dir_all(root).unwrap();
 }

 #[test]
 fn missing_resource_is_rejected_before_application(){
  let root=std::env::temp_dir().join(format!("perch-skill-selection-{}",uuid::Uuid::new_v4()));
  let id=format!("local-skill-{}",uuid::Uuid::new_v4());
  let mut recipe=crate::catalog::packs().remove(0);
  recipe.extensions.push(crate::catalog::ExtensionRef{resource_rules:Default::default(),disabled_resources:Vec::new(),id:id.clone(),version:"1".into(),enabled:true});
  assert!(validate_selection(&root,&recipe).is_err());
  let base=root.join("local-skills").join(&id);fs::create_dir_all(base.join("content")).unwrap();
  fs::write(base.join("content/SKILL.md"),"sample").unwrap();
  fs::write(base.join("metadata.json"),serde_json::to_vec(&serde_json::json!({"id":id,"version":"1"})).unwrap()).unwrap();
  assert!(validate_selection(&root,&recipe).is_ok());
  recipe.extensions.last_mut().unwrap().version="2".into();
  assert!(validate_selection(&root,&recipe).is_err());
  fs::remove_dir_all(root).unwrap();
 }
 #[test]
 fn complete_copy_and_failure_cleanup(){
  let root=std::env::temp_dir().join(format!("perch-skill-test-{}",uuid::Uuid::new_v4()));
  let source=root.join("source");fs::create_dir_all(source.join("scripts")).unwrap();
  fs::write(source.join("SKILL.md"),"sample").unwrap();
  fs::write(source.join("scripts/run.txt"),"resource").unwrap();
  let target=root.join("installed");copy_atomic(&source,&target).unwrap();
  assert_eq!(fs::read_to_string(target.join("scripts/run.txt")).unwrap(),"resource");
  assert!(copy_atomic(&source,&target).is_err());
  let oversized=fs::File::create(source.join("large.bin")).unwrap();oversized.set_len(33*1024*1024).unwrap();
  let failed=root.join("failed");assert!(copy_atomic(&source,&failed).is_err());assert!(!failed.exists());
  assert!(!fs::read_dir(&root).unwrap().any(|e|e.unwrap().file_name().to_string_lossy().starts_with(".skill-staging-")));
  drop(oversized);
  fs::remove_file(source.join("large.bin")).unwrap();
  let mut deep=source.clone();for _ in 0..34{deep=deep.join("d");fs::create_dir(&deep).unwrap();}
  assert!(copy_atomic(&source,&failed).unwrap_err().contains("32 层"));
  assert!(!failed.exists());
  assert!(!fs::read_dir(&root).unwrap().any(|e|e.unwrap().file_name().to_string_lossy().starts_with(".skill-staging-")));
  fs::remove_dir_all(root).unwrap();
 }
}
