use crate::catalog::Recipe;
use serde::{Deserialize,Serialize};
use tauri::Manager;
use std::fs;
#[derive(Serialize,Deserialize)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
pub struct PackDraft {pub id:String,pub revision:u32,pub recipe:Recipe,pub ready:bool,#[serde(default,skip_serializing_if="Option::is_none")]pub last_ready:Option<Recipe>}
fn directory(app:&tauri::AppHandle)->Result<std::path::PathBuf,String>{
 let state=app.state::<crate::data_commands::DataState>();
 Ok(state.root.as_ref().map_err(|_|"工作空间不可用")?.join("pack-drafts"))
}
#[tauri::command]
pub fn pack_drafts(app:tauri::AppHandle)->Result<Vec<PackDraft>,String>{
 let dir=directory(&app)?;if !dir.exists(){return Ok(vec![]);}
 let mut drafts=vec![];
 for entry in fs::read_dir(dir).map_err(|_|"无法读取组合草稿")? {
  let path=entry.map_err(|_|"无法读取草稿文件")?.path();
  if path.extension().and_then(|e|e.to_str())!=Some("json"){continue;}
  let bytes=fs::read(path).map_err(|_|"无法读取组合草稿")?;
  let draft=serde_json::from_slice(&bytes).map_err(|_|"有组合草稿无法解析，原文件已保留")?;drafts.push(draft);
 }
 Ok(drafts)
}
#[tauri::command]
pub fn save_pack_draft(app:tauri::AppHandle,id:Option<String>,expected_revision:Option<u32>,recipe:Recipe,ready:bool)->Result<PackDraft,String>{
 recipe.validate_metadata().map_err(|e|e.message)?;
 if recipe.name.trim().is_empty()||recipe.name.chars().count()>80||recipe.extensions.len()>128{return Err("请填写组合名称，且资源不能超过 128 项".into());}
 if ready{recipe.validate().map_err(|e|e.message)?;recipe.validate_workshop_hosts().map_err(|e|e.message)?;
 let state=app.state::<crate::data_commands::DataState>();
 let root=state.root.as_ref().map_err(|_|"工作空间不可用")?;
 crate::local_skills::validate_selection(root,&recipe).map_err(|e|e.message)?;}
 write_draft(&directory(&app)?,id,expected_revision,recipe,ready)
}
pub(crate) fn write_draft(dir:&std::path::Path,id:Option<String>,expected_revision:Option<u32>,recipe:Recipe,ready:bool)->Result<PackDraft,String>{
 static WRITES:std::sync::Mutex<()>=std::sync::Mutex::new(());
 let _guard=WRITES.lock().map_err(|_|"组合保存状态不可用，请重新打开应用")?;
 let id=id.unwrap_or_else(||uuid::Uuid::new_v4().to_string());uuid::Uuid::parse_str(&id).map_err(|_|"组合标识无效")?;
 fs::create_dir_all(dir).map_err(|_|"无法创建草稿目录")?;
 let path=dir.join(format!("{id}.json"));
 let previous:Option<PackDraft>=if path.exists(){Some(serde_json::from_slice(&fs::read(&path).map_err(|_|"无法读取旧草稿")?).map_err(|_|"旧草稿无效，保留原文件")?)}else{None};
 if previous.as_ref().map(|draft|draft.revision)!=expected_revision{return Err("组合已变化，请重新打开后编辑".into());}
 let revision=previous.as_ref().map_or(Some(1),|draft|draft.revision.checked_add(1)).ok_or("组合修订已达到上限")?;
 let last_ready=if ready{None}else{previous.and_then(|draft|if draft.ready{Some(draft.recipe)}else{draft.last_ready})};
 let mut recipe=recipe;recipe.origin=Some(crate::catalog::PackOrigin{id:id.clone(),revision});
 let draft=PackDraft{id,revision,recipe,ready,last_ready};
 let pending=path.with_extension("pending");
 fs::write(&pending,serde_json::to_vec_pretty(&draft).map_err(|_|"无法编码草稿")?).map_err(|_|"无法保存草稿")?;
 fs::rename(pending,path).map_err(|_|"无法启用草稿，旧内容保留")?;Ok(draft)
}

#[derive(Serialize,Deserialize)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
struct PortablePack {
 schema_version:u32,
 recipe:Recipe,
 profile:crate::engine_profile::ResolvedProfile,
}
pub(crate) fn export_bytes(recipe:&Recipe)->Result<Vec<u8>,String>{
 recipe.validate().map_err(|e|e.message)?;
 // A local update subscription is not transferred to another machine.
 let mut portable=recipe.clone();portable.origin=None;let recipe=&portable;
 if !recipe.profile_id.starts_with("resolved-"){return serde_json::to_vec_pretty(recipe).map_err(|_|"清单编码失败".into());}
 let mut profile=crate::engine_profile::resolve(&recipe.engine,&recipe.profile_id).map_err(|e|e.message)?;
 // Share only installer fields, never arbitrary package metadata or local config.
 let package:serde_json::Value=serde_json::from_str(&profile.package).map_err(|_|"依赖清单无效")?;
 let lock:serde_json::Value=serde_json::from_str(&profile.lock).map_err(|_|"依赖锁无效")?;
 profile.package=serde_json::json!({"private":true,"dependencies":package["dependencies"]}).to_string();
 let mut packages=serde_json::Map::new();
 for (path,item) in lock["packages"].as_object().ok_or("依赖锁缺少包目录")? {
  let mut selected=serde_json::Map::new();
  for key in ["name","version","resolved","integrity","dependencies","optionalDependencies","peerDependencies","peerDependenciesMeta","engines","os","cpu","libc","bin","hasInstallScript","dev","optional","devOptional"] {
   if let Some(value)=item.get(key){selected.insert(key.into(),value.clone());}
  }
  packages.insert(path.clone(),serde_json::Value::Object(selected));
 }
 profile.lock=serde_json::json!({"lockfileVersion":3,"requires":true,"packages":packages}).to_string();
 profile.validate().map_err(|e|e.message)?;
 serde_json::to_vec_pretty(&PortablePack{schema_version:1,recipe:recipe.clone(),profile}).map_err(|_|"分享包编码失败".into())
}
pub(crate) fn import_bytes(root:&std::path::Path,bytes:&[u8])->Result<Recipe,String>{
 if bytes.len()>8*1024*1024{return Err("清单超过 8 MB 上限".into());}
 let value:serde_json::Value=serde_json::from_slice(bytes).map_err(|_|"清单不是有效 JSON")?;
 if value.get("recipe").is_none(){
  let mut recipe:Recipe=serde_json::from_value(value).map_err(|_|"清单格式不支持或包含未知字段")?;
  recipe.validate().map_err(|e|e.message)?;recipe.origin=None;return Ok(recipe);
 }
 let mut pack:PortablePack=serde_json::from_value(value).map_err(|_|"分享包格式不支持或包含未知字段")?;
 if pack.schema_version!=1||pack.recipe.profile_id!=pack.profile.id||pack.recipe.engine!=pack.profile.engine{return Err("分享包版本或引擎锁不匹配".into());}
 pack.profile.validate().map_err(|e|e.message)?;
 pack.recipe.origin=None;
 // Imported locks receive a fresh identity and never overwrite a local profile.
 pack.profile.id=format!("resolved-{}",uuid::Uuid::new_v4());
 pack.recipe.profile_id=pack.profile.id.clone();
 // Validate recipe content against the same engine's built-in profile before writing.
 let mut checked=pack.recipe.clone();
 checked.profile_id=if checked.engine=="DSH"{crate::engine_profile::DSH.id}else{crate::engine_profile::PI.id}.into();
 let dependencies:serde_json::Value=serde_json::from_str(&pack.profile.package).map_err(|_|"依赖清单无效")?;
 let mut seen=std::collections::HashSet::new();
 for resource in &checked.extensions{resource.validate_resources().map_err(|e|e.message)?;if let Some(name)=resource.id.strip_prefix("npm:").or_else(||resource.id.strip_prefix("dsh-npm:")){
  if checked.engine!=(if resource.id.starts_with("dsh-npm:"){"DSH"}else{"Pi"})||!crate::engine_profile::valid_package_name(name)||dependencies["dependencies"][name].as_str()!=Some(resource.version.as_str())||!seen.insert(name){return Err("外部包不在对应引擎的精确依赖锁中".into());}
 }}
 checked.extensions.retain(|resource|!resource.id.starts_with("npm:")&&!resource.id.starts_with("dsh-npm:"));
 checked.validate().map_err(|e|e.message)?;
 crate::engine_profile::register(root,pack.profile).map_err(|e|e.message)?;
 Ok(pack.recipe)
}
#[tauri::command]
pub async fn export_pack(app:tauri::AppHandle,recipe:Recipe)->Result<Option<String>,String>{
 let bytes=export_bytes(&recipe)?;
 let root=app.state::<crate::data_commands::DataState>().root.as_ref().map_err(|_|"工作空间不可用")?.clone();
 tauri::async_runtime::spawn_blocking(move||{
  let archive=recipe.extensions.iter().any(|item|crate::local_skills::valid_id(&item.id));
  let bytes=if archive{crate::pack_archive::encode(&root,&recipe,&bytes)?}else{bytes};
  let Some(path)=rfd::FileDialog::new().set_title("导出整合包清单与资源").set_file_name(if archive{"perch-pack.zip"}else{"perch-pack.json"}).add_filter("整合包",if archive{&["zip"]}else{&["json"]}).save_file()else{return Ok(None);};
  fs::write(&path,bytes).map_err(|_|"清单保存失败")?;
  Ok(Some(path.to_string_lossy().into()))
 }).await.map_err(|_|"导出任务中断".to_string())?
}
#[tauri::command]
pub async fn import_pack(app:tauri::AppHandle)->Result<Option<Recipe>,String>{
 let root=app.state::<crate::data_commands::DataState>().root.as_ref().map_err(|_|"工作空间不可用")?.clone();
 tauri::async_runtime::spawn_blocking(move||{
  use std::io::Read;
  let Some(path)=rfd::FileDialog::new().set_title("导入整合包清单").add_filter("整合包",&["json","zip"]).pick_file()else{return Ok(None);};
  let mut bytes=Vec::new();fs::File::open(path).map_err(|_|"无法读取清单")?.take(64*1024*1024+1).read_to_end(&mut bytes).map_err(|_|"无法读取清单")?;
  if bytes.len()>64*1024*1024{return Err("整合包超过 64 MB 上限".into());}
  if bytes.starts_with(b"PK"){crate::pack_archive::decode(&root,&bytes).map(Some)}else{import_bytes(&root,&bytes).map(Some)}
 }).await.map_err(|_|"导入任务中断".to_string())?
}

#[cfg(test)]
mod tests {
 use super::*;
 #[test]
 fn selected_cover_roundtrip_and_unknown_cover_rejected(){
  let root=std::env::temp_dir();let mut recipe=crate::catalog::packs().remove(0);
  assert!(recipe.cover.is_empty());
  for cover in ["developer","researcher","minimal","explorer"]{recipe.cover=cover.into();let restored=import_bytes(&root,&export_bytes(&recipe).unwrap()).unwrap();assert_eq!(restored.cover,cover);}
  recipe.cover="file:///private/image.png".into();assert!(recipe.validate_metadata().is_err());assert!(export_bytes(&recipe).is_err());
 }
 #[test]
 fn draft_revision_provenance_is_local_and_old_instance_stays_unchanged(){
  let root=std::env::temp_dir().join(format!("perch-pack-revision-{}",uuid::Uuid::new_v4()));
  let recipe=crate::catalog::packs().remove(0);
  let first=write_draft(&root,None,None,recipe,true).unwrap();let instance_recipe=first.recipe.clone();
  let mut edited=first.recipe.clone();edited.extensions.clear();
  let second=write_draft(&root,Some(first.id.clone()),Some(1),edited,true).unwrap();
  assert_eq!(second.revision,2);assert_eq!(second.recipe.origin.as_ref().unwrap().id,first.id);assert_eq!(second.recipe.origin.as_ref().unwrap().revision,2);
  assert_eq!(instance_recipe.origin.as_ref().unwrap().revision,1);assert!(!instance_recipe.extensions.is_empty());
  let path=root.join(format!("{}.json",first.id));let before=fs::read(&path).unwrap();
  assert!(write_draft(&root,Some(first.id),Some(1),instance_recipe.clone(),true).is_err());assert_eq!(before,fs::read(path).unwrap());
  let exported=export_bytes(&second.recipe).unwrap();let value:serde_json::Value=serde_json::from_slice(&exported).unwrap();assert!(value.get("origin").is_none());
  let imported=import_bytes(&root,&serde_json::to_vec(&second.recipe).unwrap()).unwrap();assert!(imported.origin.is_none());
  let legacy:Recipe=serde_json::from_slice(&exported).unwrap();assert!(legacy.origin.is_none());
  let mut invalid=instance_recipe;invalid.origin.as_mut().unwrap().revision=0;assert!(invalid.validate().is_err());
  fs::remove_dir_all(root).unwrap();
 }
 #[test]
 fn unfinished_revision_preserves_last_ready(){
  let root=std::env::temp_dir().join(format!("perch-ready-draft-{}",uuid::Uuid::new_v4()));
  let first=write_draft(&root,None,None,crate::catalog::packs().remove(0),true).unwrap();
  let mut edited=first.recipe.clone();edited.name="未完成组合".into();
  let second=write_draft(&root,Some(first.id.clone()),Some(1),edited.clone(),false).unwrap();
  assert_eq!(second.last_ready.as_ref(),Some(&first.recipe));
  let third=write_draft(&root,Some(first.id.clone()),Some(2),edited.clone(),false).unwrap();
  assert_eq!(third.last_ready.as_ref(),Some(&first.recipe));
  let fourth=write_draft(&root,Some(first.id.clone()),Some(3),edited.clone(),true).unwrap();
  assert!(fourth.last_ready.is_none());
  let fifth=write_draft(&root,Some(first.id),Some(4),edited,false).unwrap();
  assert_eq!(fifth.last_ready.as_ref(),Some(&fourth.recipe));
  fs::remove_dir_all(root).unwrap();
 }
 #[test]
 fn public_description_roundtrip_and_legacy_recipe(){
  let root=std::env::temp_dir();let mut recipe=crate::catalog::packs().remove(0);
  let legacy=export_bytes(&recipe).unwrap();
  let restored=import_bytes(&root,&legacy).unwrap();assert!(restored.description.is_empty());assert!(restored.source_note.is_empty());
  recipe.description="用于文档审阅\n保留人工确认".into();recipe.source_note="社区资源，遵循原作者许可".into();
  let restored=import_bytes(&root,&export_bytes(&recipe).unwrap()).unwrap();assert_eq!(recipe,restored);
  recipe.description="x".repeat(1001);assert!(export_bytes(&recipe).is_err());
 }

 #[test]
 fn portable_lock_roundtrip_and_invalid_import(){
  let root=std::env::temp_dir().join(format!("perch-pack-share-{}",uuid::Uuid::new_v4()));
  let mut profile:crate::engine_profile::ResolvedProfile=(&crate::engine_profile::PI).into();
  profile.id=format!("resolved-{}",uuid::Uuid::new_v4());
  let original_id=profile.id.clone();
  let mut recipe=crate::catalog::packs().into_iter().find(|recipe|recipe.engine=="Pi").unwrap();
  recipe.profile_id=profile.id.clone();
  crate::engine_profile::register(&root,profile).unwrap();
  let bytes=export_bytes(&recipe).unwrap();
  let imported=import_bytes(&root,&bytes).unwrap();
  assert_ne!(imported.profile_id,original_id);
  assert_eq!(imported.extensions,recipe.extensions);
  imported.validate().unwrap();
  let recovered=crate::engine_profile::resolve("Pi",&imported.profile_id).unwrap();
  recovered.validate().unwrap();
  let original=crate::engine_profile::resolve("Pi",&original_id).unwrap();
  let original_lock:serde_json::Value=serde_json::from_str(&original.lock).unwrap();
  let recovered_lock:serde_json::Value=serde_json::from_str(&recovered.lock).unwrap();
  for (path,item) in original_lock["packages"].as_object().unwrap(){assert_eq!(item["integrity"],recovered_lock["packages"][path]["integrity"]);}
  let count=fs::read_dir(root.join("resolved-profiles")).unwrap().count();
  let mut invalid:serde_json::Value=serde_json::from_slice(&bytes).unwrap();invalid["recipe"]["connectionId"]=serde_json::json!("private");
  assert!(import_bytes(&root,&serde_json::to_vec(&invalid).unwrap()).is_err());
  assert_eq!(count,fs::read_dir(root.join("resolved-profiles")).unwrap().count());
  fs::remove_dir_all(root).unwrap();
 }
}
