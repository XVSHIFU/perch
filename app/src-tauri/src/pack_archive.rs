use crate::catalog::Recipe;
use std::{fs,io::{Cursor,Read,Write},path::Path};
use zip::{ZipArchive,ZipWriter,write::SimpleFileOptions};
const LIMIT:u64=64*1024*1024;
pub(crate) fn safe_name(name:&str)->bool{
 !name.is_empty()&&!name.chars().any(char::is_control)&&!name.contains(['\\',':'])&&name.split('/').all(|part|!part.is_empty()&&part!="."&&part!=".."&&!part.ends_with(['.',' '])&&!matches!(part.split('.').next().unwrap_or("").to_ascii_uppercase().as_str(),"CON"|"PRN"|"AUX"|"NUL"|"COM1"|"COM2"|"COM3"|"COM4"|"COM5"|"COM6"|"COM7"|"COM8"|"COM9"|"LPT1"|"LPT2"|"LPT3"|"LPT4"|"LPT5"|"LPT6"|"LPT7"|"LPT8"|"LPT9"))
}
fn private_path(path:&str)->bool {
 path.split('/').any(|part|{
  let name=part.to_ascii_lowercase();
  matches!(name.as_str(),".git"|".ssh"|".aws"|".azure"|".npmrc"|".netrc"|"auth.json"|"credentials"|"credentials.json"|"secrets.json"|"connections.json"|"sessions"|"logs"|"id_rsa"|"id_ed25519")
   || name==".env" || (name.starts_with(".env.")&&!matches!(name.as_str(),".env.example"|".env.sample"|".env.template"))
   || name.ends_with(".log")||name.ends_with(".pem")||name.ends_with(".p12")||name.ends_with(".pfx")
 })
}
#[derive(serde::Serialize,serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct ResourceLabel {name:String,#[serde(default,skip_serializing_if="Option::is_none")]source:Option<String>}
fn public_source(value:Option<&str>)->Option<String>{
 let url=url::Url::parse(value?).ok()?;
 if url.scheme()!="https"||url.host_str()!=Some("github.com")||!url.username().is_empty()||url.password().is_some()||url.query().is_some()||url.fragment().is_some(){return None;}
 Some(url.to_string())
}
fn label_name(value:Option<&str>)->String{
 value.filter(|name|!name.trim().is_empty()&&name.chars().count()<=120&&!name.chars().any(char::is_control)).unwrap_or("导入的 Skill").to_owned()
}
pub fn encode(root:&Path,recipe:&Recipe,manifest:&[u8])->Result<Vec<u8>,String>{
 crate::local_skills::validate_selection(root,recipe).map_err(|e|e.message)?;
 let mut writer=ZipWriter::new(Cursor::new(Vec::new()));
 let options=SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
 writer.start_file("manifest.json",options).map_err(|e|e.to_string())?;writer.write_all(manifest).map_err(|e|e.to_string())?;
 let mut budget=manifest.len() as u64;let mut count=0;
 let mut labels=std::collections::BTreeMap::new();
 for resource in recipe.extensions.iter().filter(|item|crate::local_skills::valid_id(&item.id)){
  let owner=root.join("local-skills").join(&resource.id);
  let metadata:serde_json::Value=serde_json::from_slice(&fs::read(owner.join("metadata.json")).map_err(|_|"无法读取 Skill 名称")?).map_err(|_|"Skill 元数据无效")?;
  labels.insert(resource.id.clone(),ResourceLabel{name:label_name(metadata["name"].as_str()),source:public_source(metadata["source"].as_str())});
  let base=owner.join("content");
  let mut pending=vec![(base,String::new())];
  while let Some((path,relative))=pending.pop(){
   if private_path(&relative){return Err(format!("分享资源包含私有文件或目录：{relative}。请从待分享的 Skill 副本移除后重新导入。"));}
   count+=1;if count>10000{return Err("资源文件数量超过上限".into());}
   let metadata=fs::symlink_metadata(&path).map_err(|e|e.to_string())?;
   #[cfg(windows)]{use std::os::windows::fs::MetadataExt;if metadata.file_attributes()&0x400!=0{return Err("不能分享链接资源".into());}}
   if metadata.file_type().is_symlink(){return Err("不能分享链接资源".into());}
   if metadata.is_dir(){for entry in fs::read_dir(path).map_err(|e|e.to_string())?{let entry=entry.map_err(|e|e.to_string())?;let name=entry.file_name().into_string().map_err(|_|"资源文件名无效")?;if name==".git"{continue;}pending.push((entry.path(),if relative.is_empty(){name}else{format!("{relative}/{name}")}));}}
   else if metadata.is_file(){
    if !safe_name(&relative){return Err("资源文件路径不受支持".into());}
    budget=budget.checked_add(metadata.len()).ok_or("资源过大")?;if budget>LIMIT{return Err("资源超过 64 MB 上限".into());}
    writer.start_file(format!("resources/{}/{relative}",resource.id),options).map_err(|e|e.to_string())?;
    let mut file=fs::File::open(path).map_err(|e|e.to_string())?;std::io::copy(&mut file,&mut writer).map_err(|e|e.to_string())?;
   }else{return Err("资源文件类型不受支持".into());}
  }
 }
 writer.start_file("resource-index.json",options).map_err(|e|e.to_string())?;
 writer.write_all(&serde_json::to_vec(&labels).map_err(|e|e.to_string())?).map_err(|e|e.to_string())?;
 Ok(writer.finish().map_err(|e|e.to_string())?.into_inner())
}
pub fn decode(root:&Path,bytes:&[u8])->Result<Recipe,String>{
 let mut archive=ZipArchive::new(Cursor::new(bytes)).map_err(|_|"无效整合包压缩文件")?;
 if archive.len()>10000{return Err("资源文件数量超过上限".into());}
 let mut entries=std::collections::BTreeMap::new();let mut seen=std::collections::HashSet::new();let mut budget=0u64;
 for index in 0..archive.len(){
  let mut file=archive.by_index(index).map_err(|_|"无法读取压缩条目")?;
  let name=file.name().to_owned();
  if !safe_name(&name)||!seen.insert(name.to_lowercase())||file.unix_mode().is_some_and(|mode|mode&0o170000==0o120000){return Err("压缩包含越界、重复路径或链接".into());}
  if name!="manifest.json"&&name!="resource-index.json"&&!name.starts_with("resources/"){return Err("压缩包包含未声明的文件".into());}
  if private_path(&name){return Err("分享包包含私有配置、会话或日志文件，未导入".into());}
  budget=budget.checked_add(file.size()).ok_or("资源过大")?;if budget>LIMIT{return Err("解压资源超过 64 MB 上限".into());}
  let mut data=Vec::new();file.by_ref().take(LIMIT+1).read_to_end(&mut data).map_err(|_|"资源读取失败")?;
  if data.len() as u64!=file.size(){return Err("资源大小不匹配".into());}entries.insert(name,data);
 }
 let labels:std::collections::BTreeMap<String,ResourceLabel>=match entries.remove("resource-index.json"){
  Some(bytes)=>serde_json::from_slice(&bytes).map_err(|_|"资源名称索引包含不支持的字段")?,None=>Default::default()
 };
 let manifest=entries.remove("manifest.json").ok_or("整合包缺少清单")?;
 let value:serde_json::Value=serde_json::from_slice(&manifest).map_err(|_|"清单无效")?;
 let preview:Recipe=serde_json::from_value(value.get("recipe").unwrap_or(&value).clone()).map_err(|_|"清单格式不支持")?;
 let ids:std::collections::HashSet<_>=preview.extensions.iter().filter(|item|crate::local_skills::valid_id(&item.id)).map(|item|item.id.clone()).collect();
 if labels.keys().any(|id|!ids.contains(id)){return Err("资源名称不在组合清单中".into());}
 for name in entries.keys(){let id=name.split('/').nth(1).ok_or("资源路径无效")?;if !ids.contains(id){return Err("资源不在组合清单中".into());}}
 for id in &ids{if !entries.contains_key(&format!("resources/{id}/SKILL.md")){return Err("Skill 缺少 SKILL.md".into());}}
 let staging=root.join(format!("pack-import-{}",uuid::Uuid::new_v4()));
 fs::create_dir_all(&staging).map_err(|e|e.to_string())?;
 let mut committed=Vec::new();
 let result=(||{
  let mut mapping=std::collections::HashMap::new();
  for id in &ids{
   let fresh=format!("local-skill-{}",uuid::Uuid::new_v4());let target=staging.join(&fresh);
   for (name,data) in &entries{if let Some(relative)=name.strip_prefix(&format!("resources/{id}/")){
    let file=target.join("content").join(relative);fs::create_dir_all(file.parent().unwrap()).map_err(|e|e.to_string())?;fs::write(file,data).map_err(|e|e.to_string())?;
   }}
   let source=public_source(labels.get(id).and_then(|label|label.source.as_deref())).unwrap_or_else(||"整合包导入".into());
   fs::write(target.join("metadata.json"),serde_json::json!({"id":fresh,"name":label_name(labels.get(id).map(|label|label.name.as_str())),"version":"1","kind":"skill","source":source,"declarations":crate::local_skills::declarations(&target.join("content")),"licenseFiles":crate::local_skills::license_files(&target.join("content")),"digest":crate::local_skills::content_digest(&target.join("content"))?}).to_string()).map_err(|e|e.to_string())?;
   mapping.insert(id.clone(),fresh);
  }
  fs::create_dir_all(root.join("local-skills")).map_err(|e|e.to_string())?;
  for fresh in mapping.values(){let destination=root.join("local-skills").join(fresh);fs::rename(staging.join(fresh),&destination).map_err(|e|e.to_string())?;committed.push(destination);}
  // Register the imported profile only after resource writes succeed. If manifest
  // validation fails, the same rollback below removes this import's resources.
  let mut recipe=crate::pack_drafts::import_bytes(root,&manifest)?;
  for item in &mut recipe.extensions{if let Some(fresh)=mapping.get(&item.id){item.id=fresh.clone();}}
  Ok(recipe)
 })();
 let _=fs::remove_dir_all(&staging);
 if result.is_err(){for path in committed{let _=fs::remove_dir_all(path);}}
 result
}

#[cfg(test)]
mod tests {
 use super::*;
 #[test]
 fn failed_resource_commit_does_not_publish_imported_profile(){
  let root=std::env::temp_dir().join(format!("perch-import-failure-{}",uuid::Uuid::new_v4()));
  let source=root.join("source");
  let mut profile:crate::engine_profile::ResolvedProfile=(&crate::engine_profile::PI).into();
  profile.id=format!("resolved-{}",uuid::Uuid::new_v4());
  let mut recipe=crate::catalog::packs().into_iter().find(|p|p.engine=="Pi").unwrap();recipe.profile_id=profile.id.clone();
  crate::engine_profile::register(&source,profile).unwrap();
  let manifest=crate::pack_drafts::export_bytes(&recipe).unwrap();
  let bytes=encode(&source,&recipe,&manifest).unwrap();
  let destination=root.join("destination");fs::create_dir_all(&destination).unwrap();
  fs::write(destination.join("local-skills"),"existing file must survive").unwrap();
  assert!(decode(&destination,&bytes).is_err());
  assert!(!destination.join("resolved-profiles").exists());
  assert_eq!(fs::read_to_string(destination.join("local-skills")).unwrap(),"existing file must survive");
  assert!(!fs::read_dir(&destination).unwrap().any(|entry|entry.unwrap().file_name().to_string_lossy().starts_with("pack-import-")));
  fs::remove_dir_all(root).unwrap();
 }
 #[test]
 fn shared_skill_preserves_public_origin_and_license_file(){
  let root=std::env::temp_dir().join(format!("perch-share-origin-{}",uuid::Uuid::new_v4()));
  let id=format!("local-skill-{}",uuid::Uuid::new_v4());let base=root.join("local-skills").join(&id);
  fs::create_dir_all(base.join("content")).unwrap();
  fs::write(base.join("content/SKILL.md"),"fixture").unwrap();fs::write(base.join("content/LICENSE.txt"),"Fixture permission text").unwrap();
  let source="https://github.com/example/skills/tree/0123456789012345678901234567890123456789/review";
  fs::write(base.join("metadata.json"),serde_json::json!({"id":id,"version":"1","source":source,"apiKey":"private-field"}).to_string()).unwrap();
  let mut recipe=crate::catalog::packs().remove(1);recipe.extensions.push(crate::catalog::ExtensionRef{id:id.clone(),version:"1".into(),enabled:true,resource_rules:Default::default(),disabled_resources:vec![]});
  let bytes=encode(&root,&recipe,&serde_json::to_vec(&recipe).unwrap()).unwrap();
  let destination=root.join("imported");let imported=decode(&destination,&bytes).unwrap();
  let path=destination.join("local-skills").join(&imported.extensions.last().unwrap().id);
  let metadata:serde_json::Value=serde_json::from_slice(&fs::read(path.join("metadata.json")).unwrap()).unwrap();
  assert_eq!(metadata["source"],source);assert_eq!(metadata["licenseFiles"],serde_json::json!(["LICENSE.txt"]));assert!(metadata.get("apiKey").is_none());
  assert_eq!(fs::read_to_string(path.join("content/LICENSE.txt")).unwrap(),"Fixture permission text");
  for private in ["C:\\private\\folder","https://private.example.org/skill.zip","https://github.com/example/repo?token=secret","https://user:secret@github.com/example/repo"]{assert!(public_source(Some(private)).is_none());}
  let old:ResourceLabel=serde_json::from_str(r#"{"name":"Old Skill"}"#).unwrap();assert!(old.source.is_none());
 }
 #[test]
 fn share_names_and_private_file_rejection(){
  let root=std::env::temp_dir().join(format!("perch-share-private-{}",uuid::Uuid::new_v4()));
  let id=format!("local-skill-{}",uuid::Uuid::new_v4());let base=root.join("local-skills").join(&id);
  fs::create_dir_all(base.join("content")).unwrap();fs::write(base.join("content/SKILL.md"),"sample").unwrap();
  fs::write(base.join("metadata.json"),serde_json::json!({"id":id,"version":"1","name":"Review tools","apiKey":"never-export-this"}).to_string()).unwrap();
  let mut recipe=crate::catalog::packs().remove(0);recipe.extensions.push(crate::catalog::ExtensionRef{resource_rules:Default::default(),disabled_resources:Vec::new(),id:id.clone(),version:"1".into(),enabled:true});
  let manifest=serde_json::to_vec(&recipe).unwrap();let bytes=encode(&root,&recipe,&manifest).unwrap();
  let imported=decode(&root.join("imported"),&bytes).unwrap();let imported_id=&imported.extensions.last().unwrap().id;
  let metadata=fs::read_to_string(root.join("imported/local-skills").join(imported_id).join("metadata.json")).unwrap();
  assert!(metadata.contains("Review tools"));assert!(!metadata.contains("never-export-this"));
  fs::write(base.join("content/.env"),"TOKEN=secret").unwrap();assert!(encode(&root,&recipe,&manifest).is_err());
  let mut archive=ZipWriter::new(Cursor::new(Vec::new()));archive.start_file(format!("resources/{id}/auth.json"),SimpleFileOptions::default()).unwrap();archive.write_all(b"private").unwrap();
  assert!(decode(&root.join("rejected"),&archive.finish().unwrap().into_inner()).is_err());assert!(!root.join("rejected").exists());
  fs::remove_dir_all(root).unwrap();
 }
 #[test]
 fn skill_archive_roundtrip_and_traversal(){
  let root=std::env::temp_dir().join(format!("perch-archive-test-{}",uuid::Uuid::new_v4()));
  let id=format!("local-skill-{}",uuid::Uuid::new_v4());
  let base=root.join("local-skills").join(&id);fs::create_dir_all(base.join("content/scripts")).unwrap();
  fs::write(base.join("content/SKILL.md"),"sample").unwrap();fs::write(base.join("content/scripts/run.txt"),"attached resource").unwrap();
  fs::write(base.join("metadata.json"),serde_json::json!({"id":id,"version":"1"}).to_string()).unwrap();
  let mut recipe=crate::catalog::packs().remove(0);recipe.extensions.push(crate::catalog::ExtensionRef{resource_rules:Default::default(),disabled_resources:Vec::new(),id:id.clone(),version:"1".into(),enabled:true});
  let bytes=encode(&root,&recipe,&serde_json::to_vec(&recipe).unwrap()).unwrap();
  let destination=root.join("destination");let imported=decode(&destination,&bytes).unwrap();
  let new_id=&imported.extensions.last().unwrap().id;assert_ne!(new_id,&id);
  assert_eq!(fs::read_to_string(destination.join("local-skills").join(new_id).join("content/scripts/run.txt")).unwrap(),"attached resource");
  crate::local_skills::validate_selection(&destination,&imported).unwrap();
  let mut malicious=ZipWriter::new(Cursor::new(Vec::new()));malicious.start_file("../escape",SimpleFileOptions::default()).unwrap();malicious.write_all(b"escape").unwrap();
  assert!(decode(&destination,&malicious.finish().unwrap().into_inner()).is_err());assert!(!root.join("escape").exists());
  fs::remove_dir_all(root).unwrap();
 }
}
