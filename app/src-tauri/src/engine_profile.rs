use crate::store::{Failure, Result};

/// The installation identity includes both the workbench and its engine SDK.
/// Both engines share the installer and supervisor; only these facts differ.
pub struct EngineProfile {
    pub id: &'static str,
    pub engine: &'static str,
    pub entry: &'static str,
    pub package: &'static str,
    pub lock: &'static str,
    pub node: &'static str,
    pub npm: &'static str,
}

pub const DSH: EngineProfile = EngineProfile {
    id: "dsh-0.1.5-rc.3",
    engine: "DSH",
    entry: "node_modules/@deepseek-ai/dsh/lib/bin.js",
    package: include_str!("../../../docs/profiles/dsh/package.json"),
    lock: include_str!("../../../docs/profiles/dsh/package-lock.json"),
    node: "24.18.0",
    npm: "12.0.2",
};

pub const PI: EngineProfile = EngineProfile {
    id: "pi-web-0.9.3-pi-0.87.1",
    engine: "Pi",
    entry: "node_modules/@agegr/pi-web/bin/pi-web.js",
    package: include_str!("../../../docs/profiles/pi/package.json"),
    lock: include_str!("../../../docs/profiles/pi/package-lock.json"),
    node: "24.18.0",
    npm: "12.0.2",
};

#[derive(Clone, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResolvedProfile {
    pub id: String,
    pub engine: String,
    pub entry: String,
    pub package: String,
    pub lock: String,
    pub node: String,
    pub npm: String,
}
impl From<&EngineProfile> for ResolvedProfile {
    fn from(p: &EngineProfile) -> Self {
        Self {
            id: p.id.into(),
            engine: p.engine.into(),
            entry: p.entry.into(),
            package: p.package.into(),
            lock: p.lock.into(),
            node: p.node.into(),
            npm: p.npm.into(),
        }
    }
}
impl ResolvedProfile {
    pub fn marker(&self) -> String {
        format!("{}/node-{}", self.id, self.node)
    }
    pub fn validate(&self) -> Result<()> {
        let invalid = || Failure::new("PROFILE_INVALID", "版本配置或依赖锁无效，未启用");
        if !self.id.starts_with("resolved-")
            || self.id.len() > 80
            || !self
                .id
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || c == b'-')
        {
            return Err(invalid());
        }
        let expected = match self.engine.as_str() {
            "DSH" => DSH.entry,
            "Pi" => PI.entry,
            _ => return Err(invalid()),
        };
        if self.entry != expected || self.node != DSH.node || self.npm != DSH.npm {
            return Err(invalid());
        }
        let package: serde_json::Value =
            serde_json::from_str(&self.package).map_err(|_| invalid())?;
        let lock: serde_json::Value = serde_json::from_str(&self.lock).map_err(|_| invalid())?;
        if package.get("scripts").is_some()
            || lock["lockfileVersion"] != 3
            || lock["packages"][""]["dependencies"] != package["dependencies"]
        {
            return Err(invalid());
        }
        let dependencies=package["dependencies"].as_object().ok_or_else(invalid)?;
        let primary=if self.engine=="DSH"{"@deepseek-ai/dsh"}else{"@agegr/pi-web"};
        let version=dependencies.get(primary).and_then(|v|v.as_str()).ok_or_else(invalid)?;
        if dependencies.len()>17 || version.is_empty() || !version.bytes().all(|c|c.is_ascii_alphanumeric()||b".-+".contains(&c)) || !version.as_bytes()[0].is_ascii_digit(){return Err(invalid());}
        if lock["packages"][format!("node_modules/{primary}")]["version"].as_str()!=Some(version){return Err(invalid());}
        for (name,value) in dependencies {
            let version=value.as_str().ok_or_else(invalid)?;
            if !valid_package_name(name)||version.is_empty()||!version.as_bytes()[0].is_ascii_digit()||!version.bytes().all(|c|c.is_ascii_alphanumeric()||b".-+".contains(&c))||lock["packages"][format!("node_modules/{name}")]["version"].as_str()!=Some(version){return Err(invalid());}
        }
        let packages = lock["packages"].as_object().ok_or_else(invalid)?;
        for (path, item) in packages {
            if path.is_empty() {
                continue;
            }
            if !path.starts_with("node_modules/")
                || path.split('/').any(|s| s == "..")
                || item["link"] == true
            {
                return Err(invalid());
            }
            let url = item["resolved"].as_str().ok_or_else(invalid)?;
            let parsed = url::Url::parse(url).map_err(|_| invalid())?;
            if parsed.scheme() != "https"
                || parsed.host_str() != Some("registry.npmjs.org")
                || !parsed.username().is_empty() || parsed.password().is_some()
                || !item["integrity"].as_str().is_some_and(|value|value.starts_with("sha512-")||value.starts_with("sha1-"))
            {
                return Err(invalid());
            }
        }
        Ok(())
    }
}
pub fn valid_package_name(name:&str)->bool{
 let part=|value:&str|!value.is_empty()&&!value.starts_with('.')&&value.bytes().all(|c|c.is_ascii_lowercase()||c.is_ascii_digit()||b"._-".contains(&c));
 if name.len()>214{return false;}
 if let Some(scoped)=name.strip_prefix('@'){let pieces:Vec<_>=scoped.split('/').collect();pieces.len()==2&&pieces.iter().all(|value|part(value))}else{part(name)}
}
fn registry() -> &'static std::sync::RwLock<std::collections::HashMap<String, ResolvedProfile>> {
    static PROFILES: std::sync::OnceLock<
        std::sync::RwLock<std::collections::HashMap<String, ResolvedProfile>>,
    > = std::sync::OnceLock::new();
    PROFILES.get_or_init(Default::default)
}
pub fn register(root: &std::path::Path, profile: ResolvedProfile) -> Result<()> {
    use std::io::Write;
    profile.validate()?;
    let bytes=serde_json::to_vec(&profile)?;
    let mut profiles=registry().write().map_err(|_| Failure::new("PROFILE_STATE", "版本配置不可用"))?;
    let dir = root.join("resolved-profiles");
    std::fs::create_dir_all(&dir)?;
    let path = dir.join(format!("{}.json", profile.id));
    if path.exists() {
        return Err(Failure::new(
            "PROFILE_EXISTS",
            "版本组合已存在，不能覆盖锁文件",
        ));
    }
    let mut file=std::fs::OpenOptions::new().write(true).create_new(true).open(&path)?;
    if let Err(error)=file.write_all(&bytes).and_then(|_|file.sync_all()){
        drop(file);let _=std::fs::remove_file(&path);return Err(error.into());
    }
    profiles.insert(profile.id.clone(), profile);
    Ok(())
}
pub fn load(root: &std::path::Path) -> Result<()> {
    let dir = root.join("resolved-profiles");
    if !dir.exists() {
        return Ok(());
    }
    for entry in std::fs::read_dir(dir)? {
        let path = entry?.path();
        if path.extension().and_then(|s| s.to_str()) != Some("json") {
            continue;
        }
        let Ok(bytes)=std::fs::read(&path) else {continue;};
        let Ok(profile)=serde_json::from_slice::<ResolvedProfile>(&bytes) else {continue;};
        if profile.validate().is_err(){continue;}
        registry()
            .write()
            .map_err(|_| Failure::new("PROFILE_STATE", "版本配置不可用"))?
            .insert(profile.id.clone(), profile);
    }
    Ok(())
}
pub fn resolve(engine: &str, id: &str) -> Result<ResolvedProfile> {
    if let Some(profile) = [&DSH, &PI]
        .into_iter()
        .find(|p| p.engine == engine && p.id == id)
    {
        return Ok(profile.into());
    }
    registry()
        .read()
        .ok()
        .and_then(|items| items.get(id).filter(|p| p.engine == engine).cloned())
        .ok_or_else(|| {
            Failure::new(
                "PROFILE_UNSUPPORTED",
                "引擎与版本组合不匹配，请先解析并锁定此组合",
            )
        })
}

#[cfg(test)]
impl EngineProfile {
    pub fn marker(&self) -> String {
        format!("{}/node-{}", self.id, self.node)
    }
}

#[tauri::command]
pub fn resolved_profiles()->Vec<serde_json::Value>{
    registry().read().map(|items|items.values().map(|p|{
        let package:serde_json::Value=serde_json::from_str(&p.package).unwrap_or_default();
        serde_json::json!({"id":p.id,"engine":p.engine,"dependencies":package["dependencies"]})
    }).collect()).unwrap_or_default()
}

pub(crate) fn cache_profiles() -> Result<Vec<(String, String)>> {
    let profiles = registry().read().map_err(|_| Failure::new("PROFILE_STATE", "版本配置不可用，未执行缓存清理"))?;
    let mut result = vec![(DSH.id.into(), DSH.engine.into()), (PI.id.into(), PI.engine.into())];
    result.extend(profiles.values().map(|profile| (profile.id.clone(), profile.engine.clone())));
    result.sort();
    result.dedup_by(|a,b| a.0 == b.0);
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn dynamic_profile_rejects_missing_engine_and_foreign_artifacts() {
        let mut profile:ResolvedProfile=(&PI).into();
        profile.id=format!("resolved-{}",uuid::Uuid::new_v4());
        // Existing full lock is accepted under a new immutable profile identity.
        profile.validate().unwrap();
        let original=profile.lock.clone();
        let mut lock:serde_json::Value=serde_json::from_str(&original).unwrap();
        lock["packages"]["node_modules/@agegr/pi-web"]["resolved"]="https://example.org/package.tgz".into();
        profile.lock=lock.to_string();assert!(profile.validate().is_err());
        profile.lock=original;profile.package="{\"dependencies\":{}}".into();assert!(profile.validate().is_err());
    }
}
