//! Public catalog snapshots shipped with the binary; no installation payloads or credentials.
use std::{fs,path::Path};
fn snapshot(name:&str)->Option<&'static [u8]>{Some(match name{
 "resource-catalog.json"=>include_bytes!("../catalog-seed/resource-catalog.json"),
 "workshop-catalog.json"=>include_bytes!("../catalog-seed/workshop-catalog.json"),
 "pi-official-catalog.json"=>include_bytes!("../catalog-seed/pi-official-catalog.json"),
 "dsh-topic-catalog.json"=>include_bytes!("../catalog-seed/dsh-topic-catalog.json"),
 _=>return None,
})}
pub(crate) fn ensure(path:&Path)->Result<(),String>{
 let Some(bytes)=path.file_name().and_then(|s|s.to_str()).and_then(snapshot) else{return Ok(());};
 let _guard=crate::resource_catalog::CacheRefresh::begin(path)?;
 match fs::read(path){
  Ok(existing)=>{
   let doc:serde_json::Value=serde_json::from_slice(&existing).map_err(|_|"本机目录缓存无法读取，原文件已保留；可手动重新同步")?;
   // Never replace an existing usable catalog or unrelated source preferences.
   if doc["entries"].as_array().is_none_or(|entries|!entries.is_empty()){return Ok(());}
  },
  Err(error) if error.kind()==std::io::ErrorKind::NotFound=>{},
  Err(_)=>return Err("本机目录缓存不可读，原文件已保留".into()),
 }
 crate::resource_catalog::save_cache(path,bytes)
}
#[cfg(test)]mod tests{use super::*;
 #[test]fn offline_catalogs_preserve_user_data(){
  let root=std::env::temp_dir().join(format!("perch-seed-{}",uuid::Uuid::new_v4()));fs::create_dir_all(&root).unwrap();
  fs::write(root.join("resource-sources.json"),"custom preferences").unwrap();
  for name in ["resource-catalog.json","workshop-catalog.json","pi-official-catalog.json","dsh-topic-catalog.json"]{
   let path=root.join(name);ensure(&path).unwrap();let data=fs::read(&path).unwrap();let doc:serde_json::Value=serde_json::from_slice(&data).unwrap();assert!(!doc["entries"].as_array().unwrap().is_empty());assert_eq!(doc["bundled"],"2026-09-28");ensure(&path).unwrap();assert_eq!(data,fs::read(&path).unwrap());
  }
  let path=root.join("resource-catalog.json");fs::write(&path,r#"{"entries":[{"id":"user-cached"}]}"#).unwrap();let saved=fs::read(&path).unwrap();ensure(&path).unwrap();assert_eq!(saved,fs::read(&path).unwrap());
  fs::write(&path,"invalid document").unwrap();assert!(ensure(&path).is_err());assert_eq!(fs::read_to_string(&path).unwrap(),"invalid document");assert_eq!(fs::read_to_string(root.join("resource-sources.json")).unwrap(),"custom preferences");fs::remove_dir_all(root).unwrap();
 }
}
