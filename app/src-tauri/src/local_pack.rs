use crate::{catalog::ExtensionRef,local_skills,managed,pack_drafts,store::{Failure,Instance,Result}};
use serde::Serialize;
use std::{fs,path::{Path,PathBuf}};

#[derive(Serialize)]
#[serde(rename_all="camelCase")]
pub struct SkillPreview {pub name:String,pub path:String,pub digest:String,pub license:Option<String>}
#[derive(Serialize)]
#[serde(rename_all="camelCase")]
pub struct Preview {pub token:String,pub skills:Vec<SkillPreview>}
fn failure(message:String)->Failure{Failure::new("LOCAL_PACK",&message)}
pub fn preview(root:&Path,instance:&Instance)->Result<Preview>{
    managed::validate_managed_shareable_packages(root,instance)?;
    let mut skills=Vec::new();
    for path in managed::discovered_skills(root,instance)? {
        let digest=local_skills::content_digest(&path).map_err(failure)?;
        let declarations=local_skills::declarations(&path);
        let name=declarations["name"].as_str().or_else(||path.file_name().and_then(|name|name.to_str())).unwrap_or("本地 Skill").to_string();
        skills.push(SkillPreview{name,path:path.to_string_lossy().into(),digest,license:declarations["license"].as_str().map(str::to_owned)});
    }
    // Revalidate config and content at submission; client paths are never accepted.
    use sha2::{Digest,Sha256};
    let mut recipe=managed::recipe(root,instance)?;
    managed::capture_public_preset(root,instance,&mut recipe)?;
    recipe.workshop=managed::workshop_references(root,instance)?;
    let bytes=serde_json::to_vec(&(instance.revision,&recipe,&skills))?;
    Ok(Preview{token:format!("{:x}",Sha256::digest(bytes)),skills})
}
pub fn save(root:&Path,instance:&Instance,token:&str)->Result<pack_drafts::PackDraft>{
    let current=preview(root,instance)?;
    if current.token!=token{return Err(Failure::new("LOCAL_PACK_CHANGED","实例配置或 Skill 内容已变化，请重新预览后保存"));}
    if current.skills.is_empty(){return Err(Failure::new("LOCAL_PACK_EMPTY","未发现待整理的 Skill，请使用保存为整合包"));}
    let mut recipe=managed::recipe(root,instance)?;
    managed::capture_public_preset(root,instance,&mut recipe)?;
    recipe.workshop=managed::workshop_references(root,instance)?;
    let mut created:Vec<PathBuf>=Vec::new();
    let result=(||{
        for skill in &current.skills {
            let id=format!("local-skill-{}",uuid::Uuid::new_v4());
            let owner=root.join("local-skills").join(&id);
            created.push(owner.clone());
            local_skills::copy_atomic(Path::new(&skill.path),&owner.join("content")).map_err(failure)?;
            let digest=local_skills::content_digest(&owner.join("content")).map_err(failure)?;
            if digest!=skill.digest{return Err(Failure::new("LOCAL_PACK_CHANGED","复制期间 Skill 内容发生变化，未保存组合；请重新预览"));}
            let metadata=serde_json::json!({"id":id,"name":skill.name,"version":"1","kind":"skill","source":"本地资源快照","digest":digest,"declarations":local_skills::declarations(&owner.join("content")),"licenseFiles":local_skills::license_files(&owner.join("content"))});
            fs::write(owner.join("metadata.json"),serde_json::to_vec_pretty(&metadata)?)?;
            recipe.extensions.push(ExtensionRef{id,version:"1".into(),enabled:true,disabled_resources:vec![],resource_rules:Default::default()});
        }
        recipe.validate()?;
        recipe.validate_workshop_hosts()?;
        // Apply the existing export whitelist before marking this combination ready.
        let manifest=pack_drafts::export_bytes(&recipe).map_err(failure)?;
        crate::pack_archive::encode(root,&recipe,&manifest).map_err(failure)?;
        // Reject changes occurring while the snapshots were being copied.
        if preview(root,instance)?.token!=token{return Err(Failure::new("LOCAL_PACK_CHANGED","源内容已变化，请重新预览"));}
        pack_drafts::write_draft(&root.join("pack-drafts"),None,None,recipe,true).map_err(failure)
    })();
    if result.is_err(){for path in created{let _=fs::remove_dir_all(path);}}
    result
}

#[cfg(test)]
mod tests {
 use super::*;
 #[test]
 fn discovered_skill_snapshot_is_portable_and_rejects_changes_or_private_files(){
  let root=std::env::temp_dir().join(format!("perch-local-pack-{}",uuid::Uuid::new_v4()));
  let project=root.join("project");fs::create_dir_all(&project).unwrap();
  let recipe=crate::catalog::packs().remove(1);
  let instance=Instance{schema_version:1,id:uuid::Uuid::new_v4().to_string(),name:"fixture".into(),engine:"Pi".into(),project_path:project.to_string_lossy().into(),connection_id:None,profile_id:recipe.profile_id.clone(),revision:1,created_at:0,updated_at:0};
  managed::apply(&root,&instance,&recipe,"fixture").unwrap();
  let skill=project.join(".pi/skills/example");fs::create_dir_all(skill.join("references")).unwrap();
  fs::write(skill.join("SKILL.md"),"---\nname: example\nlicense: MIT\n---\nUse references/data.txt").unwrap();
  fs::write(skill.join("references/data.txt"),"original").unwrap();
  let first=preview(&root,&instance).unwrap();assert_eq!(first.skills.len(),1);
  fs::write(skill.join("references/data.txt"),"changed").unwrap();
  assert!(save(&root,&instance,&first.token).is_err());assert!(!root.join("pack-drafts").exists());
  let current=preview(&root,&instance).unwrap();let draft=save(&root,&instance,&current.token).unwrap();assert!(draft.ready);
  assert_eq!(managed::recipe(&root,&instance).unwrap(),recipe);
  let manifest=pack_drafts::export_bytes(&draft.recipe).unwrap();let zip=crate::pack_archive::encode(&root,&draft.recipe,&manifest).unwrap();
  let destination=root.join("rebuilt");let rebuilt=crate::pack_archive::decode(&destination,&zip).unwrap();
  let added=rebuilt.extensions.iter().find(|item|local_skills::valid_id(&item.id)).unwrap();
  assert_eq!(fs::read_to_string(destination.join("local-skills").join(&added.id).join("content/references/data.txt")).unwrap(),"changed");
  fs::write(skill.join(".env"),"SECRET=do-not-share").unwrap();
  let before=fs::read_dir(root.join("local-skills")).unwrap().count();
  assert!(save(&root,&instance,&preview(&root,&instance).unwrap().token).is_err());
  assert_eq!(fs::read_dir(root.join("local-skills")).unwrap().count(),before);
  assert_eq!(fs::read_to_string(skill.join(".env")).unwrap(),"SECRET=do-not-share");
  fs::remove_dir_all(root).unwrap();
 }
}
