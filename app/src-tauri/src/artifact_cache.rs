use crate::{
    engine_profile, managed,
    store::{Failure, Instance, Result},
};
use serde::Serialize;
use std::{collections::HashSet, fs, path::Path};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CacheEntry {
    pub profile_id: String,
    pub in_use: bool,
    pub installed: bool,
}
pub fn list(root: &Path, instances: &[Instance]) -> Result<Vec<CacheEntry>> {
    let mut protected = HashSet::new();
    for instance in instances {
        protected.insert(instance.profile_id.clone());
        for point in managed::points(root, instance)? {
            protected.insert(point.recipe.profile_id);
        }
    }
    let drafts = root.join("pack-drafts");
    if drafts.exists() {
        for entry in fs::read_dir(drafts)? {
            let path = entry?.path();
            if path.extension().and_then(|value| value.to_str()) != Some("json") {
                continue;
            }
            let draft: crate::pack_drafts::PackDraft = serde_json::from_slice(&fs::read(path)?)
                .map_err(|_| Failure::new("CACHE_DRAFT", "有整合包草稿无法读取，无法确认缓存引用；未执行清理"))?;
            protected.insert(draft.recipe.profile_id);
            if let Some(ready) = draft.last_ready {
                protected.insert(ready.profile_id);
            }
        }
    }
    Ok(engine_profile::cache_profiles()?
        .into_iter()
        .filter(|(id,_)| root.join("artifacts").join(id).exists())
        .map(|(id,engine)| CacheEntry {
            in_use: protected.contains(&id),
            installed: crate::engine::installed(root, &engine, &id),
            profile_id: id,
        })
        .collect())
}
pub fn clean(root: &Path, instances: &[Instance]) -> Result<Vec<String>> {
    let candidates = list(root, instances)?;
    let parent = root.join("artifacts");
    if !parent.exists() {
        return Ok(vec![]);
    }
    let absolute = parent.canonicalize()?;
    let mut removed = vec![];
    for entry in candidates.into_iter().filter(|e| !e.in_use) {
        let target = parent.join(&entry.profile_id);
        let metadata = fs::symlink_metadata(&target)?;
        #[cfg(windows)]
        {
            use std::os::windows::fs::MetadataExt;
            if metadata.file_attributes() & 0x400 != 0 {
                return Err(Failure::new("CACHE_LINK", "缓存目录是链接，未执行清理"));
            }
        }
        let resolved = target.canonicalize()?;
        if metadata.file_type().is_symlink() || resolved.parent() != Some(absolute.as_path()) {
            return Err(Failure::new(
                "CACHE_PATH",
                "缓存路径不在受管目录中，未执行清理",
            ));
        }
        fs::remove_dir_all(&resolved)?;
        removed.push(entry.profile_id);
    }
    Ok(removed)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn dynamic_cache_is_listed_and_only_unused_artifact_is_removed() {
        let root = std::env::temp_dir().join(format!("perch-dynamic-cache-{}", uuid::Uuid::new_v4()));
        let mut profile: engine_profile::ResolvedProfile = (&engine_profile::PI).into();
        profile.id = format!("resolved-{}", uuid::Uuid::new_v4());
        let id = profile.id.clone();
        engine_profile::register(&root, profile).unwrap();
        fs::create_dir_all(root.join("artifacts").join(&id)).unwrap();
        fs::create_dir_all(root.join("artifacts").join("unknown-directory")).unwrap();
        let rows = list(&root, &[]).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].profile_id, id);
        assert!(!rows[0].in_use);
        assert_eq!(clean(&root, &[]).unwrap(), vec![id.clone()]);
        assert!(root.join("resolved-profiles").join(format!("{id}.json")).exists());
        assert!(root.join("artifacts").join("unknown-directory").exists());
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn saved_drafts_and_last_ready_protect_cache() {
        let root = std::env::temp_dir().join(format!("perch-draft-cache-{}", uuid::Uuid::new_v4()));
        let dir = root.join("pack-drafts");
        fs::create_dir_all(&dir).unwrap();
        for profile in [&engine_profile::PI, &engine_profile::DSH] {
            fs::create_dir_all(root.join("artifacts").join(profile.id)).unwrap();
        }
        let packs = crate::catalog::packs();
        let draft = crate::pack_drafts::PackDraft {
            id: uuid::Uuid::new_v4().to_string(), revision: 2, ready: false,
            recipe: packs.iter().find(|p| p.engine == "Pi").unwrap().clone(),
            last_ready: Some(packs.iter().find(|p| p.engine == "DSH").unwrap().clone()),
        };
        let path = dir.join("draft.json");
        fs::write(&path, serde_json::to_vec(&draft).unwrap()).unwrap();
        assert!(clean(&root, &[]).unwrap().is_empty());
        fs::write(&path, b"{broken").unwrap();
        assert!(clean(&root, &[]).is_err());
        assert!(root.join("artifacts").join(engine_profile::DSH.id).exists());
        assert!(root.join("artifacts").join(engine_profile::PI.id).exists());
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn snapshots_protect_cache_and_only_unreferenced_artifacts_are_removed() {
        let root = std::env::temp_dir().join(format!("perch-cache-{}", uuid::Uuid::new_v4()));
        let instance = Instance {
            schema_version: 1,
            id: uuid::Uuid::new_v4().to_string(),
            name: "test".into(),
            engine: "DSH".into(),
            profile_id: engine_profile::DSH.id.into(),
            connection_id: None,
            project_path: root.join("project").to_string_lossy().into(),
            revision: 1,
            created_at: 0,
            updated_at: 0,
        };
        for profile in [&engine_profile::PI, &engine_profile::DSH] {
            fs::create_dir_all(root.join("artifacts").join(profile.id)).unwrap();
        }
        let point = managed::RestorePoint {
            id: uuid::Uuid::new_v4().to_string(),
            created_at: 0,
            reason: "previous pinned profile".into(),
            recipe: crate::catalog::packs().remove(1),
            data_version: 0,
        };
        let directory = root
            .join("instances")
            .join(&instance.id)
            .join("restore-points")
            .join(&point.id);
        fs::create_dir_all(&directory).unwrap();
        fs::write(
            directory.join("point.json"),
            serde_json::to_vec(&point).unwrap(),
        )
        .unwrap();
        assert!(clean(&root, std::slice::from_ref(&instance))
            .unwrap()
            .is_empty());
        fs::remove_file(directory.join("point.json")).unwrap();
        assert_eq!(
            clean(&root, &[instance]).unwrap(),
            vec![engine_profile::PI.id.to_string()]
        );
        assert!(root.join("artifacts").join(engine_profile::DSH.id).exists());
        fs::remove_dir_all(root).unwrap();
    }
}
