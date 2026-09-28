use crate::{
    catalog::Recipe,
    store::{Failure, Instance, Result},
};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

#[derive(Clone, Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RestorePoint {
    pub id: String,
    pub created_at: u64,
    pub reason: String,
    pub recipe: Recipe,
    #[serde(default)]
    pub data_version: u32,
}
#[derive(Serialize, Deserialize)]
struct Transaction {
    #[serde(default)]
    workshop_data: bool,
    snapshot: String,
    had_previous: bool,
    #[serde(default)]
    restore_data: bool,
    #[serde(default)]
    settings_only: bool,
    #[serde(default)]
    data_existing: Vec<String>,
}

// Auth files, provider credentials, module caches and external workspaces are
// excluded. The current shared Key is always resolved again at the next start.
const DATA_ENTRIES: &[&str] = &[
    "skins",
    "pets",
    "agent-presets",
    "sessions",
    "storages",
    "settings.yaml",
    "settings.json",
    "models.json",
    "extensions",
    "skills",
    "prompts",
];
const WORKSHOP_ENTRIES: &[&str] = &["skins", "pets", "agent-presets"];
pub fn workshop_references(root:&Path,instance:&Instance)->Result<Vec<crate::catalog::WorkshopRef>>{
    use sha2::{Digest,Sha256};use std::io::Read;
    let inventory=workshop_inventory(root,instance)?;
    let home=directory(root,&instance.id)?.join("agent-data");let mut references=Vec::new();
    for item in inventory{
        let relative=item["directory"].as_str().ok_or_else(||Failure::new("WORKSHOP_SOURCE","工坊目录无效"))?;
        let invalid=||Failure::new("WORKSHOP_SOURCE",&format!("{relative} 的来源或文件已变化，不能保存为可重建声明；原资源保留"));
        if item["managed"]!=true{return Err(invalid());}
        let base=home.join(relative);let path=base.join(".perch-resource.json");
        check_entry(&path)?;
        let receipt:serde_json::Value=serde_json::from_slice(&fs::read(path)?)?;let plan=&receipt["plan"];
        let parts=plan["id"].as_str().ok_or_else(invalid)?.split(':').collect::<Vec<_>>();if parts.len()!=3||parts[0]!="workshop"{return Err(invalid());}
        let reference=crate::catalog::WorkshopRef{requires:plan["requires"].clone(),kind:parts[1].into(),id:parts[2].into(),version:plan["version"].as_str().ok_or_else(invalid)?.into(),repository:plan["repository"].as_str().ok_or_else(invalid)?.into(),commit:plan["commit"].as_str().ok_or_else(invalid)?.into(),content_path:plan["contentPath"].as_str().ok_or_else(invalid)?.into(),files:serde_json::from_value(receipt["sha256"].clone()).map_err(|_|invalid())?};
        reference.validate()?;
        if workshop_target(&reference)!=relative{return Err(invalid());}
        let mut actual=std::collections::HashSet::new();let mut pending=vec![(base.clone(),String::new())];let mut total=0u64;
        while let Some((path,prefix))=pending.pop(){
            for entry in fs::read_dir(path)?{
                let entry=entry?;let name=entry.file_name().into_string().map_err(|_|invalid())?;
                if prefix.is_empty()&&name==".perch-resource.json"{continue;}
                let relative=if prefix.is_empty(){name}else{format!("{prefix}/{name}")};let metadata=check_entry(&entry.path())?;
                if metadata.is_dir(){pending.push((entry.path(),relative));continue;}
                if !metadata.is_file()||metadata.len()>200*1024*1024{return Err(invalid());}
                let expected=reference.files.get(&relative).ok_or_else(invalid)?;
                let mut file=fs::File::open(entry.path())?.take(200*1024*1024+1);let mut hash=Sha256::new();let size=std::io::copy(&mut file,&mut hash)?;total=total.checked_add(size).ok_or_else(invalid)?;
                if size>200*1024*1024||total>512*1024*1024||format!("{:x}",hash.finalize())!=*expected{return Err(invalid());}actual.insert(relative);
            }
        }
        if actual.len()!=reference.files.len(){return Err(invalid());}references.push(reference);
    }
    Ok(references)
}
pub fn workshop_inventory(root:&Path,instance:&Instance)->Result<Vec<serde_json::Value>>{
    let home=directory(root,&instance.id)?.join("agent-data");safe_dir(&home)?;
    let mut resources=Vec::new();
    for kind in WORKSHOP_ENTRIES{
        let parent=home.join(kind);if !parent.exists(){continue;}
        if check_entry(&parent).is_err(){resources.push(serde_json::json!({"directory":kind,"name":kind,"status":"目录包含链接或无法读取，未扫描","managed":false}));continue;}
        for entry in fs::read_dir(&parent)?{
            let entry=entry?;let name=entry.file_name().to_string_lossy().into_owned();let relative=format!("{kind}/{name}");
            let metadata=match check_entry(&entry.path()){Ok(value)=>value,Err(_)=>{resources.push(serde_json::json!({"directory":relative,"name":name,"status":"链接或不可读取资源，未扫描","managed":false}));continue;}};
            if !metadata.is_dir(){continue;}
            let receipt_path=entry.path().join(".perch-resource.json");
            let receipt=check_entry(&receipt_path).ok().filter(|meta|meta.is_file()&&meta.len()<=2*1024*1024).and_then(|_|fs::read(receipt_path).ok()).and_then(|bytes|serde_json::from_slice::<serde_json::Value>(&bytes).ok());
            if let Some(value)=receipt.filter(|value|value["schemaVersion"]==1&&value["plan"]["directory"]==relative){
                resources.push(serde_json::json!({"directory":relative,"name":value["plan"]["name"].as_str().unwrap_or(&name),"version":value["plan"]["version"],"commit":value["plan"]["commit"],"managed":true,"status":"来源已记录，文件内容尚未重新校验"}));
            }else{resources.push(serde_json::json!({"directory":relative,"name":name,"managed":false,"status":"来源未记录或记录无效；保留原内容"}));}
        }
    }
    resources.sort_by_key(|value|value["directory"].as_str().unwrap_or("").to_owned());Ok(resources)
}
fn check_entry(path: &Path) -> Result<fs::Metadata> {
    let metadata = fs::symlink_metadata(path)?;
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if metadata.file_attributes() & 0x400 != 0 {
            return Err(Failure::new(
                "SNAPSHOT_LINK",
                "备份范围包含链接，无法安全复制；原数据未改动",
            ));
        }
    }
    if metadata.file_type().is_symlink() {
        return Err(Failure::new("SNAPSHOT_LINK", "备份范围包含符号链接"));
    }
    Ok(metadata)
}
fn remove_entry(path: &Path) -> Result<()> {
    if !path.exists() {
        return Ok(());
    }
    let metadata = check_entry(path)?;
    if metadata.is_dir() {
        fs::remove_dir_all(path)?;
    } else {
        fs::remove_file(path)?;
    }
    Ok(())
}
fn copy_entry(source: &Path, destination: &Path) -> Result<()> {
    let metadata = check_entry(source)?;
    if metadata.is_dir() {
        fs::create_dir_all(destination)?;
        for entry in fs::read_dir(source)? {
            let entry = entry?;
            copy_entry(&entry.path(), &destination.join(entry.file_name()))?;
        }
    } else if metadata.is_file() {
        if let Some(parent) = destination.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::copy(source, destination)?;
    } else {
        return Err(Failure::new(
            "SNAPSHOT_FILE",
            "备份范围包含不支持的文件类型",
        ));
    }
    Ok(())
}
fn copy_data(source: &Path, destination: &Path) -> Result<()> {
    safe_dir(source)?;
    safe_dir(destination)?;
    fs::create_dir_all(destination)?;
    for name in DATA_ENTRIES {
        let path = source.join(name);
        if path.exists() {
            copy_entry(&path, &destination.join(name))?;
        }
    }
    Ok(())
}

fn write_json(path: &Path, value: &impl Serialize) -> Result<()> {
    use std::io::Write;
    let pending = path.with_extension("pending");
    let mut file = fs::File::create(&pending)?;
    file.write_all(&serde_json::to_vec_pretty(value)?)?;
    file.sync_all()?;
    drop(file);
    fs::rename(pending, path)?;
    Ok(())
}
fn uuid(id: &str) -> Result<()> {
    uuid::Uuid::parse_str(id)
        .map(|_| ())
        .map_err(|_| Failure::new("INVALID_ID", "实例或恢复点标识不正确"))
}
fn directory(root: &Path, id: &str) -> Result<PathBuf> {
    uuid(id)?;
    safe_dir(root)?;
    safe_dir(&root.join("instances"))?;
    let path = root.join("instances").join(id);
    safe_dir(&path)?;
    Ok(path)
}
fn safe_dir(path: &Path) -> Result<()> {
    match fs::symlink_metadata(path) {
        Ok(metadata) => {
            #[cfg(windows)]
            {
                use std::os::windows::fs::MetadataExt;
                if metadata.file_attributes() & 0x400 != 0 {
                    return Err(Failure::new(
                        "LINK_UNSUPPORTED",
                        "受管配置目录不能是链接或联接点",
                    ));
                }
            }
            if metadata.file_type().is_symlink() || !metadata.is_dir() {
                return Err(Failure::new("INVALID_DIRECTORY", "受管配置目录类型不正确"));
            }
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
    }
    Ok(())
}
fn read_recipe(path: &Path) -> Result<Recipe> {
    let recipe: Recipe = serde_json::from_slice(&fs::read(path)?)?;
    recipe.validate()?;
    Ok(recipe)
}
pub fn recipe(root: &Path, instance: &Instance) -> Result<Recipe> {
    let base = directory(root, &instance.id)?;
    safe_dir(&base.join("managed"))?;
    let file = base.join("managed/recipe.json");
    if file.exists() {
        read_recipe(&file)
    } else {
        Ok(Recipe {
            thinking_level:None,
            workshop:Vec::new(),
            cover: String::new(),
            origin: None,
            description: String::new(),
            source_note: String::new(),
            schema_version: 1,
            name: instance.name.clone(),
            engine: instance.engine.clone(),
            profile_id: instance.profile_id.clone(),
            extensions: vec![],
            model_protocols: if instance.engine == "DSH" {
                vec!["deepseek".into(), "openai-chat".into()]
            } else {
                vec![
                    "deepseek".into(),
                    "openai-chat".into(),
                    "openai-responses".into(),
                    "anthropic".into(),
                ]
            },
        })
    }
}
pub fn points(root: &Path, instance: &Instance) -> Result<Vec<RestorePoint>> {
    let base = directory(root, &instance.id)?.join("restore-points");
    safe_dir(&base)?;
    if !base.exists() {
        return Ok(vec![]);
    }
    let mut points = vec![];
    for entry in fs::read_dir(base)? {
        let entry = entry?;
        safe_dir(&entry.path())?;
        let file = entry.path().join("point.json");
        if file.is_file() {
            let point: RestorePoint = serde_json::from_slice(&fs::read(file)?)?;
            uuid(&point.id)?;
            if entry.file_name().to_string_lossy() != point.id || point.data_version > 2 {
                return Err(Failure::new("SNAPSHOT_VERSION", "恢复点标识或版本不受支持"));
            }
            point.recipe.validate()?;
            points.push(point);
        }
    }
    points.sort_by_key(|point| std::cmp::Reverse(point.created_at));
    Ok(points)
}
pub fn snapshot(root: &Path, instance: &Instance, reason: &str) -> Result<RestorePoint> {
    let base = directory(root, &instance.id)?;
    let point = RestorePoint {
        id: uuid::Uuid::new_v4().to_string(),
        created_at: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs(),
        reason: reason.into(),
        recipe: recipe(root, instance)?,
        data_version: 2,
    };
    let dir = base.join("restore-points").join(&point.id);
    safe_dir(&base.join("restore-points"))?;
    fs::create_dir_all(&dir)?;
    copy_data(&base.join("agent-data"), &dir.join("agent-data"))?;
    write_json(&dir.join("point.json"), &point)?;
    Ok(point)
}

#[derive(Serialize)]
#[serde(rename_all="camelCase")]
pub struct PackageDrift {
    token: String,
    changes: Vec<PackageDifference>,
    adoption_error: Option<String>,
}
#[derive(Serialize)]
pub struct PackageDifference {
    package: String,
    expected: String,
    actual: String,
}
fn package_source(value: &serde_json::Value) -> Option<&str> {
    value.as_str().or_else(||value.get("source").and_then(|v|v.as_str()))
}
fn selection_summary(value: &serde_json::Value) -> String {
    if value.is_string(){return "按包默认选择".into();}
    ["extensions","skills","prompts","themes"].iter().filter_map(|kind|value.get(kind).map(|rules|{
        let status=match rules.as_array(){Some(items) if items.is_empty()=>"禁用".to_string(),Some(items)=>format!("{} 条选择规则",items.len()),None=>"无效规则".into()};
        format!("{kind}: {status}")
    })).collect::<Vec<_>>().join("；")
}
fn package_settings(root:&Path,instance:&Instance)->Result<(PathBuf,Vec<u8>,serde_json::Value)> {
    if instance.engine!="Pi" {return Err(Failure::new("ENGINE_UNSUPPORTED","此操作仅适用于 Pi 包配置"));}
    let home=directory(root,&instance.id)?.join("agent-data");safe_dir(&home)?;
    let path=home.join("settings.json");
    if !path.exists(){return Ok((path,Vec::new(),serde_json::json!({})));}
    let metadata=check_entry(&path)?;
    if !metadata.is_file()||metadata.len()>2*1024*1024{return Err(Failure::new("SETTINGS_INVALID","Pi 设置文件类型或大小不受支持，原文件保留"));}
    let bytes=fs::read(&path)?;
    let value:serde_json::Value=serde_json::from_slice(&bytes)?;
    if !value.is_object(){return Err(Failure::new("SETTINGS_INVALID","Pi 设置必须是对象，原文件保留"));}
    for key in ["packages","perchManagedPackages"] {
        if let Some(entries)=value.get(key){if !entries.is_array()||entries.as_array().unwrap().iter().any(|entry|package_source(entry).is_none()){return Err(Failure::new("SETTINGS_INVALID","Pi 包声明无效，原文件保留"));}}
    }
    Ok((path,bytes,value))
}
fn settings_token(bytes:&[u8])->String {use sha2::{Digest,Sha256};format!("{:x}",Sha256::digest(bytes))}
pub fn package_drift(root:&Path,instance:&Instance)->Result<PackageDrift> {
    let (_,bytes,value)=package_settings(root,instance)?;
    let empty=Vec::new();
    let owned=value.get("perchManagedPackages").and_then(|v|v.as_array()).unwrap_or(&empty);
    let actual=value.get("packages").and_then(|v|v.as_array()).unwrap_or(&empty);
    let changes=owned.iter().filter_map(|expected|{
        let source=package_source(expected)?;
        let matches=actual.iter().filter(|item|package_source(item)==Some(source)).collect::<Vec<_>>();
        if matches.len()==1&&matches[0]==expected{return None;}
        // Display a local package label, never the complete settings or provider fields.
        let package=source.rsplit(['/', '\\']).next().unwrap_or("Pi 包").to_string();
        let actual=match matches.as_slice(){[]=>"已移除".into(),[item]=>format!("{}（规则已变化）",selection_summary(item)),_=>format!("重复声明 {} 次",matches.len())};
        Some(PackageDifference{package,expected:selection_summary(expected),actual})
    }).collect();
    let adoption_error=adopted_selection(root,instance,&value).err().map(|error|error.message);
    Ok(PackageDrift{token:settings_token(&bytes),changes,adoption_error})
}
// Pi also discovers resources outside settings.packages. Inspect only known
// entry points, without reading prompt contents or following directory links.
fn validate_pi_discovery(root:&Path,instance:&Instance)->Result<()> {
    pi_discovery(root,instance,false).map(|_|())
}
fn pi_discovery(root:&Path,instance:&Instance,collect:bool)->Result<Vec<PathBuf>> {
    let mut skills=Vec::new();
    let home=directory(root,&instance.id)?.join("agent-data");
    let project=Path::new(&instance.project_path).join(".pi");
    for (base,label) in [(&home,"实例数据"),(&project,"项目 .pi")] {
        safe_dir(base)?;
        for name in ["skills","extensions","prompts","themes","SYSTEM.md","APPEND_SYSTEM.md"] {
            let path=base.join(name);
            let metadata=match fs::symlink_metadata(&path) {
                Err(error) if error.kind()==std::io::ErrorKind::NotFound=>continue,
                Err(error)=>return Err(error.into()),Ok(_)=>check_entry(&path)?,
            };
            let present=if metadata.is_dir(){fs::read_dir(&path)?.next().transpose()?.is_some()}else{true};
            if present&&collect&&name=="skills" {collect_skill_directories(&path,&mut skills,0)?;continue;}
            if present{return Err(Failure::new("PACK_UNMANAGED_RESOURCES",&format!("{label}中的 {name} 会被 Pi 自动发现，但尚未记录在组合中，分享后可能缺少这些内容。请将所需资源导入来源或整理为固定版本 Pi 包后加入组合，并处理自动发现目录中的重复内容；原文件未修改，也不会复制项目或私有提示词")));}
        }
    }
    let path=project.join("settings.json");
    match fs::symlink_metadata(&path) {
        Err(error) if error.kind()==std::io::ErrorKind::NotFound=>return Ok(skills),
        Err(error)=>return Err(error.into()),Ok(_)=>{}
    }
    let invalid=||Failure::new("PACK_UNMANAGED_RESOURCES","项目 .pi/settings.json 无法检查，暂不能确认组合是否完整；原配置未修改");
    let metadata=check_entry(&path)?;
    if !metadata.is_file()||metadata.len()>2*1024*1024{return Err(invalid());}
    let value:serde_json::Value=serde_json::from_slice(&fs::read(&path)?).map_err(|_|invalid())?;
    if !value.is_object(){return Err(invalid());}
    for key in ["packages","extensions","skills","prompts","themes"] {
        if let Some(entries)=value.get(key){
            if !entries.as_array().is_some_and(|items|items.is_empty()) {
                return Err(Failure::new("PACK_UNMANAGED_RESOURCES",&format!("项目 .pi/settings.json 的 {key} 含独立资源声明，当前组合未记录。请先把所需资源加入固定组合；不会自动复制项目配置或密钥")));
            }
        }
    }
    Ok(skills)
}
fn validate_dsh_discovery(root:&Path,instance:&Instance)->Result<()> {
    dsh_discovery(root,instance,false).map(|_|())
}
fn dsh_discovery(root:&Path,instance:&Instance,collect:bool)->Result<Vec<PathBuf>> {
    let mut skills=Vec::new();
    let home=directory(root,&instance.id)?.join("agent-data");safe_dir(&home)?;
    let settings=home.join("settings.yaml");
    let mut agents=std::env::var_os("USERPROFILE").map(|path|PathBuf::from(path).join(".agents"));
    if settings.exists(){
        let metadata=check_entry(&settings)?;
        if !metadata.is_file()||metadata.len()>2*1024*1024{return Err(Failure::new("PACK_UNMANAGED_RESOURCES","DSH Skill 默认目录设置无法检查；原配置未修改"));}
        let value:serde_json::Value=serde_yaml_ng::from_slice(&fs::read(&settings)?).map_err(|_|Failure::new("PACK_UNMANAGED_RESOURCES","DSH 设置无法解析，暂不能确认自动发现范围；原配置未修改"))?;
        if value["skill-filesystem"]["includeDefaultRoots"]==false{return Ok(skills);}
        if let Some(path)=value["skill-filesystem"]["agentsHome"].as_str(){agents=Some(PathBuf::from(path));}
        if value["skill-filesystem"].get("dshHome").is_some(){return Err(Failure::new("PACK_UNMANAGED_RESOURCES","DSH 使用了自定义 Skill 数据根目录，当前组合不能完整重建该路径；请先将所需 Skill 导入受管组合，原设置保留"));}
    }
    let cwd=PathBuf::from(&instance.project_path);
    let project=cwd.ancestors().find(|path|path.join(".git").exists()).unwrap_or(&cwd);
    let mut roots=vec![(project.join(".dsh"),"项目 .dsh",false),(project.join(".agents"),"项目 .agents",false),(home,"DSH 实例",true)];
    if let Some(agents)=agents{if !agents.is_absolute(){return Err(Failure::new("PACK_UNMANAGED_RESOURCES","DSH Skill 根目录不是绝对路径，暂不能确认分享范围"));}roots.push((agents,"DSH 默认或自定义 agents",false));}
    for (base,label,skip_system) in roots {
        safe_dir(&base)?;
        let path=base.join("skills");safe_dir(&path)?;
        if !path.exists(){continue;}
        for entry in fs::read_dir(&path)? {
            let entry=entry?;
            if skip_system&&entry.file_name()==".system"{continue;}
            if collect{collect_skill_directories(&entry.path(),&mut skills,0)?;continue;}
            return Err(Failure::new("PACK_UNMANAGED_RESOURCES",&format!("{label}目录含自动发现的 Skill，当前组合未记录其来源，分享后可能遗漏。请先导入所需 Skill 并加入组合，再处理默认目录中的重复内容，或在工作台停用默认目录发现；原文件未修改")));
        }
    }
    Ok(skills)
}
fn collect_skill_directories(path:&Path,skills:&mut Vec<PathBuf>,depth:usize)->Result<()> {
    if depth>16||skills.len()>=128{return Err(Failure::new("SKILL_LIMIT","自动发现目录过深或超过 128 项，请先整理为较小的组合"));}
    safe_dir(path)?;
    if path.join("SKILL.md").is_file(){skills.push(path.to_path_buf());return Ok(());}
    for entry in fs::read_dir(path)?{let entry=entry?;let metadata=check_entry(&entry.path())?;
        if metadata.is_dir(){collect_skill_directories(&entry.path(),skills,depth+1)?;}
        else{return Err(Failure::new("SKILL_LAYOUT","自动发现目录包含独立文件，无法确定完整 Skill 边界；请先整理为包含 SKILL.md 的目录，原文件保留"));}
    }
    Ok(())
}
pub(crate) fn discovered_skills(root:&Path,instance:&Instance)->Result<Vec<PathBuf>> {
    let mut paths=if instance.engine=="Pi"{pi_discovery(root,instance,true)?}else{dsh_discovery(root,instance,true)?};
    paths.sort();paths.dedup();Ok(paths)
}
pub(crate) fn validate_shareable_packages(root:&Path,instance:&Instance)->Result<()> {
    if instance.engine=="Pi"{validate_pi_discovery(root,instance)?;}
    if instance.engine=="DSH"{validate_dsh_discovery(root,instance)?;}
    validate_managed_shareable_packages(root,instance)
}
pub(crate) fn validate_managed_shareable_packages(root:&Path,instance:&Instance)->Result<()> {
    let selected=recipe(root,instance)?;
    let skill_base=directory(root,&instance.id)?.join("config/managed-skills");
    safe_dir(&skill_base)?;
    for item in selected.extensions.iter().filter(|item|item.enabled&&crate::local_skills::valid_id(&item.id)) {
        let parent=skill_base.join(&item.id);
        match fs::symlink_metadata(&parent){
            Err(error) if error.kind()==std::io::ErrorKind::NotFound=>continue, // Not materialized until first start.
            Err(error)=>return Err(error.into()),Ok(_)=>{}
        }
        safe_dir(&parent)?;
        let content=parent.join("skill");
        let actual=crate::local_skills::content_digest(&content).map_err(|_|Failure::new("PACK_RESOURCE_DRIFT","实例中的 Skill 副本无法读取，暂不能完整分享；请检查受管 Skill 目录，原文件保留"))?;
        let source=root.join("local-skills").join(&item.id).join("content");
        let original=crate::local_skills::content_digest(&source).map_err(|_|Failure::new("PACK_RESOURCE_DRIFT","Skill 来源副本无法读取，请重新导入完整目录；原文件保留"))?;
        if actual!=original {
            return Err(Failure::new("PACK_RESOURCE_DRIFT","实例中的 Skill 内容已与导入版本不同，直接分享会导出旧内容。请将修改后的完整 Skill 重新导入并替换组合中的旧引用，再保存整合包；原副本与来源均未修改"));
        }
    }
    if instance.engine=="DSH" {
        let home=directory(root,&instance.id)?.join("agent-data");safe_dir(&home)?;
        // Upstream loads both home and profile overlays in addition to the
        // launcher-provided package patches. Never silently omit those layers.
        for relative in ["cordis.patch.yml","profiles/web/cordis.patch.yml"] {
            let path=home.join(relative);
            match fs::symlink_metadata(&path){
                Err(error) if error.kind()==std::io::ErrorKind::NotFound=>continue,
                Err(error)=>return Err(error.into()),Ok(_)=>{}
            }
            if let Some(parent)=path.parent(){safe_dir(parent)?;}
            let metadata=check_entry(&path)?;
            if !metadata.is_file()||metadata.len()>2*1024*1024{return Err(Failure::new("PACK_UNMANAGED_RESOURCES","DSH 本地补丁无法检查，暂不能完整分享；原文件保留"));}
            let patch:serde_yaml_ng::Value=serde_yaml_ng::from_slice(&fs::read(&path)?).map_err(|_|Failure::new("PACK_UNMANAGED_RESOURCES","DSH 本地补丁无法解析，暂不能完整分享；请检查 cordis.patch.yml，原文件保留"))?;
            let empty=patch.is_null()||patch.as_sequence().is_some_and(|items|items.is_empty())||patch.as_mapping().is_some_and(|items|items.is_empty());
            if !empty{return Err(Failure::new("PACK_UNMANAGED_RESOURCES","DSH 存在组合清单之外的本地补丁，当前分享会遗漏这些修改。请先把所需插件整理为固定版本 DSH 包并加入组合，处理本地补丁后再保存；不会自动复制私有补丁或修改原文件"));}
        }
        let manifest=home.join("profiles/web/package.json");
        match fs::symlink_metadata(&manifest){
            Err(error) if error.kind()==std::io::ErrorKind::NotFound=>{},
            Err(error)=>return Err(error.into()),
            Ok(_)=>{
                safe_dir(manifest.parent().unwrap())?;
                let metadata=check_entry(&manifest)?;
                if !metadata.is_file()||metadata.len()>2*1024*1024{return Err(Failure::new("PACK_UNMANAGED_RESOURCES","DSH profile 清单无法检查，原文件保留"));}
                let value:serde_json::Value=serde_json::from_slice(&fs::read(&manifest)?).map_err(|_|Failure::new("PACK_UNMANAGED_RESOURCES","DSH profile 清单无法解析，原文件保留"))?;
                // Upstream initializes dependencies as {}. Launcher-managed
                // packages live in the fixed installation, not this profile.
                for field in ["dependencies","devDependencies","optionalDependencies"] {
                    if value.get(field).is_some_and(|entries|entries.as_object().map(|items|!items.is_empty()).unwrap_or(true)){
                        return Err(Failure::new("PACK_UNMANAGED_RESOURCES","DSH profile 含本地安装的依赖，当前组合未锁定这些来源。请先从扩展市场加入所需的固定版本并处理 profile 中的重复依赖，再保存分享包；原文件未修改"));
                    }
                }
            }
        }
        return Ok(());
    }
    if instance.engine!="Pi"{return Ok(());}
    let (_,_,value)=package_settings(root,instance)?;
    for kind in ["extensions","skills"] {
        let Some(entries)=value.get(kind) else {continue;};
        let entries=entries.as_array().ok_or_else(||Failure::new("SETTINGS_INVALID","Pi 扩展或 Skill 路径列表无效，原配置未修改"))?;
        if entries.is_empty(){continue;}
        // Derive known paths without calling the startup helpers: those write
        // generated extensions and copy skills, which a share preview must not do.
        let selected=recipe(root,instance)?;
        let base=directory(root,&instance.id)?.join("config");
        let normalize=|path:&str|path.replace('\\',"/").to_lowercase();
        let expected:Vec<String>=selected.extensions.iter().filter(|item|item.enabled).filter_map(|item|{
            let path=if kind=="extensions"&&item.id.ends_with("-local-clock") {
                base.join("managed-extensions").join(format!("{}-{}.mjs",item.id,item.version))
            }else if kind=="skills"&&crate::local_skills::valid_id(&item.id){
                base.join("managed-skills").join(&item.id)
            }else if kind=="skills"&&item.id.ends_with("-review-checklist"){
                base.join("managed-skills").join(format!("{}-{}",item.id,item.version))
            }else{return None;};
            Some(normalize(&path.to_string_lossy()))
        }).collect();
        if entries.iter().any(|entry|entry.as_str().map(|path|!expected.contains(&normalize(path))).unwrap_or(true)){
            return Err(Failure::new("PACK_UNMANAGED_RESOURCES",&format!("Pi 的 {kind} 含当前组合未记录的路径或选择规则，暂不能完整分享。Skill 可从来源页导入完整目录后添加到实例；扩展请加入固定版本的 Pi 包。随后在工作台清理重复路径声明；原文件未修改")));
        }
    }
    // Package-provided prompts/themes are represented by the package lock.
    // Explicit settings paths are independent resources, not package selections.
    for key in ["prompts","themes"] {
        if let Some(entries)=value.get(key) {
            let entries=entries.as_array().ok_or_else(||Failure::new("SETTINGS_INVALID","Pi 提示词或主题路径列表无效，原配置未修改"))?;
            if !entries.is_empty() {
                return Err(Failure::new("PACK_UNMANAGED_RESOURCES",&format!("Pi 设置中的 {key} 含独立资源路径，当前组合尚未记录其来源，不能完整分享。请将这些资源整理为固定版本的 Pi 包并从扩展市场加入组合，再移除重复的路径声明；原配置未修改")));
            }
        }
    }
    let empty=Vec::new();
    let owned=value.get("perchManagedPackages").and_then(|items|items.as_array()).unwrap_or(&empty);
    let actual=value.get("packages").and_then(|items|items.as_array()).unwrap_or(&empty);
    if actual.iter().any(|item|!owned.iter().any(|entry|package_source(entry)==package_source(item))){
        return Err(Failure::new("PACK_UNMANAGED_RESOURCES","Pi 工作台含尚未纳入组合的包，不能完整分享。请先从扩展市场加入对应固定版本，再处理工作台中的重复声明；原配置未修改"));
    }
    if owned.iter().any(|entry|{
        let matches=actual.iter().filter(|item|package_source(item)==package_source(entry)).collect::<Vec<_>>();
        matches.len()!=1||matches[0]!=entry
    }){
        return Err(Failure::new("PACK_RESOURCE_DRIFT","Pi 工作台的包选择已变化。请先在实例的包差异区采纳或恢复选择，再保存整合包；原配置未修改"));
    }
    Ok(())
}
fn adopted_selection(root:&Path,instance:&Instance,value:&serde_json::Value)->Result<(Recipe,serde_json::Value)> {
    let mut next=recipe(root,instance)?;
    let empty=Vec::new();
    let owned=value.get("perchManagedPackages").and_then(|v|v.as_array()).unwrap_or(&empty);
    let actual=value.get("packages").and_then(|v|v.as_array()).unwrap_or(&empty);
    let mut adopted=Vec::new();
    let normalize=|s:&str|s.replace('\\',"/").to_lowercase();
    for entry in owned {
        let source=package_source(entry).ok_or_else(||Failure::new("PACKAGE_UNKNOWN","无法识别原包声明"))?;
        let item=next.extensions.iter_mut().find(|item|item.id.strip_prefix("npm:").map(|name|normalize(&root.join("artifacts").join(&instance.profile_id).join("node_modules").join(name).to_string_lossy())==normalize(source)).unwrap_or(false)).ok_or_else(||Failure::new("PACKAGE_UNKNOWN","包路径不属于当前固定组合，暂不能采纳；可恢复原选择"))?;
        let matches=actual.iter().filter(|value|package_source(value)==Some(source)).collect::<Vec<_>>();
        if matches.len()>1{return Err(Failure::new("PACKAGE_DUPLICATE","存在重复包声明，请在 Pi 中去重后重新检查，或恢复原选择"));}
        item.enabled=!matches.is_empty();item.disabled_resources.clear();item.resource_rules.clear();
        if let Some(selected)=matches.first(){
            if let Some(object)=selected.as_object(){
                for (key,rules) in object {
                    if key=="source"{continue;}
                    if !["extensions","skills","prompts","themes"].contains(&key.as_str()){return Err(Failure::new("PACKAGE_RULE_UNSUPPORTED","包含未知资源选择字段，无法无损采纳；原配置保留"));}
                    let rules:Vec<String>=serde_json::from_value(rules.clone()).map_err(|_|Failure::new("PACKAGE_RULE_UNSUPPORTED","Pi 文件选择必须为字符串列表；原配置保留"))?;
                    if rules.is_empty(){item.disabled_resources.push(key.clone());}else{item.resource_rules.insert(key.clone(),rules);}
                }
            }
            item.validate_resources()?;
            adopted.push((*selected).clone());
        }
    }
    let mut settings=value.clone();settings["perchManagedPackages"]=serde_json::json!(adopted);
    Ok((next,settings))
}
pub fn adopt_package_selection(root:&Path,instance:&Instance,token:&str,expected:&Recipe)->Result<RestorePoint>{
    let (_,bytes,value)=package_settings(root,instance)?;
    if settings_token(&bytes)!=token||&recipe(root,instance)?!=expected{return Err(Failure::new("SETTINGS_CHANGED","配置或组合已变化，请刷新后重新确认"));}
    let (next,settings)=adopted_selection(root,instance,&value)?;
    transact(root,instance,&next,"采纳 Pi 包选择前自动备份",None,Some((&bytes,&settings)))
}
pub fn restore_package_selection(root:&Path,instance:&Instance,token:&str)->Result<RestorePoint> {
    let (path,bytes,mut value)=package_settings(root,instance)?;
    if settings_token(&bytes)!=token{return Err(Failure::new("SETTINGS_CHANGED","工作台配置已变化，请刷新差异后重新确认"));}
    if package_drift(root,instance)?.changes.is_empty(){return Err(Failure::new("NO_PACKAGE_DRIFT","没有需要恢复的受管包差异"));}
    let owned=value["perchManagedPackages"].as_array().cloned().unwrap_or_default();
    let mut remaining=value.get("packages").and_then(|v|v.as_array()).cloned().unwrap_or_default();
    remaining.retain(|item|!owned.iter().any(|entry|package_source(entry)==package_source(item)));
    remaining.extend(owned);
    value["packages"]=serde_json::json!(remaining);
    let point=snapshot(root,instance,"恢复 Pi 包选择前自动备份")?;
    // Recheck after potentially slow snapshot copying; never overwrite a stale preview.
    let (_,current,_)=package_settings(root,instance)?;
    if current!=bytes{return Err(Failure::new("SETTINGS_CHANGED","备份期间工作台配置已变化，原文件保留，请刷新后重试"));}
    write_json(&path,&value)?;
    Ok(point)
}

/// Transactions restore only the instance-owned allowlist. Dedicated auth files and
/// external project files are never copied, moved or overwritten.
pub fn apply(
    root: &Path,
    instance: &Instance,
    next: &Recipe,
    reason: &str,
) -> Result<RestorePoint> {
    transact(root, instance, next, reason, None, None)
}
pub(crate) fn capture_public_preset(root:&Path,instance:&Instance,recipe:&mut Recipe)->Result<()> {
    let home=directory(root,&instance.id)?.join("agent-data");safe_dir(&home)?;
    let path=home.join(if instance.engine=="Pi"{"settings.json"}else{"settings.yaml"});
    match fs::symlink_metadata(&path){
        Err(error) if error.kind()==std::io::ErrorKind::NotFound=>return Ok(()),
        Err(error)=>return Err(error.into()),Ok(_)=>{}
    }
    let invalid=||Failure::new("PUBLIC_PRESET_READ","无法提取默认思考强度；请检查工作台设置。原配置未修改");
    let metadata=check_entry(&path)?;
    if !metadata.is_file()||metadata.len()>2*1024*1024{return Err(invalid());}
    let bytes=fs::read(&path)?;
    let value:serde_json::Value=if instance.engine=="Pi"{
        serde_json::from_slice(&bytes).map_err(|_|invalid())?
    }else{serde_yaml_ng::from_slice(&bytes).map_err(|_|invalid())?};
    if !value.is_object(){return Err(invalid());}
    if instance.engine=="DSH"&&value.get("agent-default-model").is_some_and(|section|!section.is_object()){return Err(invalid());}
    if instance.engine=="DSH" {
        if let Some(section)=value.get("skill-filesystem") {
            let section=section.as_object().ok_or_else(||Failure::new("PACK_UNMANAGED_RESOURCES","DSH Skill 配置无法识别，暂不能完整分享；原文件保留"))?;
            if let Some(dirs)=section.get("customSkillDirs") {
                let dirs=dirs.as_array().ok_or_else(||Failure::new("PACK_UNMANAGED_RESOURCES","DSH Skill 路径必须为列表；原文件保留"))?;
                let base=directory(root,&instance.id)?.join("config/managed-skills");
                let normalize=|path:&str|path.replace('\\',"/").to_lowercase();
                let expected:Vec<String>=recipe.extensions.iter().filter(|item|item.enabled).filter_map(|item|{
                    let name=if crate::local_skills::valid_id(&item.id){item.id.clone()}else if item.id.ends_with("-review-checklist"){format!("{}-{}",item.id,item.version)}else{return None;};
                    Some(normalize(&base.join(name).to_string_lossy()))
                }).collect();
                if dirs.iter().any(|item|item.as_str().map(|path|!expected.contains(&normalize(path))).unwrap_or(true)){
                    return Err(Failure::new("PACK_UNMANAGED_RESOURCES","DSH 引用了组合之外的 Skill 目录，分享时无法自动带上这些内容。请在资源来源中导入完整 Skill 并添加到实例，再处理工作台中的重复路径；原目录与配置未修改"));
                }
            }
        }
    }
    let level=if instance.engine=="Pi"{value.get("defaultThinkingLevel")}else{value.get("agent-default-model").and_then(|section|section.get("reasoningEffort"))};
    let next=match level {
        None=>None,
        Some(value)=>Some(value.as_str().filter(|s|["off","low","high","max"].contains(s))
            .ok_or_else(||Failure::new("PUBLIC_PRESET_UNSUPPORTED","当前思考强度尚不支持分享到组合；请先在工作台选择 off、low、high、max，或取消该设置。原配置未修改"))?.to_owned())
    };
    recipe.thinking_level=next;Ok(())
}
pub(crate) fn install_workshop_files(root:&Path,instance:&Instance,relative:&str,content:&Path,receipt:&serde_json::Value)->Result<RestorePoint>{
    if instance.engine!="DSH"{return Err(Failure::new("WORKSHOP_ENGINE","工坊资源只能安装到 DSH 实例"));}
    let parts=relative.split('/').collect::<Vec<_>>();
    if parts.len()!=2||!WORKSHOP_ENTRIES.contains(&parts[0])||!crate::pack_archive::safe_name(parts[1]){
        return Err(Failure::new("WORKSHOP_PATH","工坊资源目标目录无效"));
    }
    let plan=&receipt["plan"];
    let invalid=||Failure::new("WORKSHOP_RECEIPT","工坊资源来源记录无效，请重新下载资源");
    let reference=crate::catalog::WorkshopRef{requires:plan["requires"].clone(),
        kind:if parts[0]=="agent-presets"{"presets".into()}else{parts[0].into()},
        id:parts[1].into(),version:plan["version"].as_str().ok_or_else(invalid)?.into(),
        repository:plan["repository"].as_str().ok_or_else(invalid)?.into(),
        commit:plan["commit"].as_str().ok_or_else(invalid)?.into(),
        content_path:plan["contentPath"].as_str().ok_or_else(invalid)?.into(),
        files:serde_json::from_value(receipt["sha256"].clone()).map_err(|_|invalid())?,
    };
    reference.validate()?;
    if plan["directory"].as_str()!=Some(relative){return Err(invalid());}
    recover(root,&instance.id)?;
    let base=directory(root,&instance.id)?;let home=base.join("agent-data");safe_dir(&home)?;
    let destination=home.join(relative);
    if fs::symlink_metadata(&destination).is_ok(){return Err(Failure::new("WORKSHOP_EXISTS","同名工坊资源已存在；原内容保留，请先确认更新差异"));}
    let pending=base.join(format!("workshop-next-{}",uuid::Uuid::new_v4()));
    let result=(||{
        copy_data(&home,&pending)?;
        // The cache may contain unrelated files; install only the verified receipt.
        let target=pending.join(relative);
        for path in reference.files.keys(){copy_entry(&content.join(path),&target.join(path))?;}
        verify_workshop_files(&target,&reference)?;
        write_json(&pending.join(relative).join(".perch-resource.json"),receipt)?;
        let current=recipe(root,instance)?;
        transact(root,instance,&current,"安装工坊资源前自动备份",Some((&pending,2)),None)
    })();
    if pending.exists(){let _=remove_entry(&pending);}
    result
}
fn workshop_target(reference:&crate::catalog::WorkshopRef)->String {
    format!("{}/{}",if reference.kind=="presets"{"agent-presets"}else{&reference.kind},reference.id)
}
fn verify_workshop_files(base:&Path,reference:&crate::catalog::WorkshopRef)->Result<()> {
    use sha2::{Digest,Sha256};use std::io::Read;
    let invalid=||Failure::new("WORKSHOP_CHANGED",&format!("工坊资源 {} 的文件已变化，请先确认差异；原内容保留",reference.id));
    safe_dir(base)?;
    let mut pending=vec![(base.to_path_buf(),String::new())];
    let mut found=std::collections::BTreeSet::new();let mut total=0u64;
    while let Some((path,prefix))=pending.pop(){
        for entry in fs::read_dir(path)?{
            let entry=entry?;let name=entry.file_name().into_string().map_err(|_|invalid())?;
            let metadata=check_entry(&entry.path())?;
            if prefix.is_empty()&&name==".perch-resource.json"{
                if !metadata.is_file(){return Err(invalid());}continue;
            }
            let relative=if prefix.is_empty(){name}else{format!("{prefix}/{name}")};
            if metadata.is_dir(){pending.push((entry.path(),relative));continue;}
            let expected=reference.files.get(&relative).ok_or_else(invalid)?;
            if !metadata.is_file()||metadata.len()>200*1024*1024{return Err(invalid());}
            let mut file=fs::File::open(entry.path())?.take(200*1024*1024+1);let mut hash=Sha256::new();
            let size=std::io::copy(&mut file,&mut hash)?;total=total.checked_add(size).ok_or_else(invalid)?;
            if size>200*1024*1024||total>512*1024*1024||format!("{:x}",hash.finalize())!=*expected{return Err(invalid());}
            found.insert(relative);
        }
    }
    if found.len()!=reference.files.len(){return Err(invalid());}Ok(())
}
/// Only modifies the transaction's private staging tree, never the live home.
fn stage_workshop_recipe(root:&Path,staging:&Path,previous:&Recipe,next:&Recipe)->Result<()> {
    for resource in &previous.workshop {
        resource.validate()?;
        let target=staging.join(workshop_target(resource));
        // Do not turn a user's edits or removals into silent replacements.
        verify_workshop_files(&target,resource)?;
        remove_entry(&target)?;
    }
    for resource in &next.workshop {
        let (receipt,content)=crate::resource_catalog::cached_workshop_reference(root,resource)
            .map_err(|message|Failure::new("WORKSHOP_CACHE",&message))?;
        crate::resource_catalog::require_workshop_recipe_host(root,next,&receipt["plan"])
            .map_err(|message|Failure::new("WORKSHOP_HOST",&message))?;
        let relative=workshop_target(resource);let target=staging.join(&relative);
        safe_dir(target.parent().ok_or_else(||Failure::new("WORKSHOP_PATH","资源目录无效"))?)?;
        if fs::symlink_metadata(&target).is_ok(){
            return Err(Failure::new("WORKSHOP_CONFLICT",&format!("{relative} 已有不属于旧组合的资源，请先处理同名冲突；原内容保留")));
        }
        // Copy the locked allowlist, not arbitrary extra files in a cache directory.
        for path in resource.files.keys(){copy_entry(&content.join(path),&target.join(path))?;}
        verify_workshop_files(&target,resource)?;
        write_json(&target.join(".perch-resource.json"),&receipt)?;
    }
    Ok(())
}
fn transact(
    root: &Path,
    instance: &Instance,
    next: &Recipe,
    reason: &str,
    restore_from: Option<(&Path,u32)>,
    settings_update: Option<(&[u8],&serde_json::Value)>,
) -> Result<RestorePoint> {
    next.validate()?;
    crate::local_skills::validate_selection(root,next)?;
    if next.engine != instance.engine || next.profile_id != instance.profile_id {
        return Err(Failure::new(
            "RECIPE_ENGINE",
            "整合包与实例的固定引擎组合不一致",
        ));
    }
    recover(root, &instance.id)?;
    let base = directory(root, &instance.id)?;
    fs::create_dir_all(&base)?;
    let current = base.join("managed");
    let staging = base.join("managed-next");
    safe_dir(&current)?;
    safe_dir(&staging)?;
    if staging.exists() {
        fs::remove_dir_all(&staging)?;
    }
    fs::create_dir(&staging)?;
    fs::write(
        staging.join("recipe.json"),
        serde_json::to_vec_pretty(next)?,
    )?;
    let point = snapshot(root, instance, reason)?;
    let home = base.join("agent-data");
    safe_dir(&home)?;
    let data_staging = base.join("data-next");
    let rebuild_workshop=restore_from.is_none()&&settings_update.is_none()
        &&(!next.workshop.is_empty()||!point.recipe.workshop.is_empty());
    if rebuild_workshop {
        safe_dir(&data_staging)?;
        remove_entry(&data_staging)?;
        copy_data(&home,&data_staging)?;
        stage_workshop_recipe(root,&data_staging,&point.recipe,next)?;
    }
    if let Some((source,version)) = restore_from {
        safe_dir(&data_staging)?;
        if data_staging.exists() {
            fs::remove_dir_all(&data_staging)?;
        }
        copy_data(source, &data_staging)?;
        if version < 2 {
            for name in WORKSHOP_ENTRIES {
                let target=data_staging.join(name);
                remove_entry(&target)?;
                let current=home.join(name);
                if current.exists(){copy_entry(&current,&target)?;}
            }
        }
    }
    if let Some((expected,settings))=settings_update {
        let (_,bytes,_)=package_settings(root,instance)?;
        if bytes!=expected{return Err(Failure::new("SETTINGS_CHANGED","备份期间配置已变化，请刷新后重新确认"));}
        safe_dir(&data_staging)?;
        if data_staging.exists(){fs::remove_dir_all(&data_staging)?;}
        fs::create_dir(&data_staging)?;
        write_json(&data_staging.join("settings.json"),settings)?;
    }
    let transaction = Transaction {
        workshop_data: true,
        snapshot: point.id.clone(),
        had_previous: current.exists(),
        restore_data: rebuild_workshop||restore_from.is_some()||settings_update.is_some(),
        settings_only: settings_update.is_some(),
        data_existing: DATA_ENTRIES
            .iter()
            .filter(|name| home.join(name).exists())
            .map(|s| s.to_string())
            .collect(),
    };
    let journal = base.join("managed-transaction.json");
    write_json(&journal, &transaction)?;
    let result = (|| -> Result<()> {
        if transaction.had_previous {
            fs::rename(
                &current,
                base.join("restore-points").join(&point.id).join("previous"),
            )?;
        }
        fs::rename(&staging, &current)?;
        if transaction.restore_data {
            let previous = base
                .join("restore-points")
                .join(&point.id)
                .join("runtime-previous");
            fs::create_dir_all(&previous)?;
            fs::create_dir_all(&home)?;
            for name in DATA_ENTRIES {
                if transaction.settings_only&&*name!="settings.json"{continue;}
                let target = home.join(name);
                let replacement = data_staging.join(name);
                if target.exists() {
                    check_entry(&target)?;
                    fs::rename(&target, previous.join(name))?;
                }
                if replacement.exists() {
                    fs::rename(replacement, target)?;
                }
            }
        }
        fs::remove_file(&journal)?;
        Ok(())
    })();
    if result.is_err() {
        recover(root, &instance.id)?;
    }
    result?;
    Ok(point)
}
pub fn recover(root: &Path, id: &str) -> Result<()> {
    let base = directory(root, id)?;
    let journal = base.join("managed-transaction.json");
    if !journal.exists() {
        return Ok(());
    }
    let transaction: Transaction = serde_json::from_slice(&fs::read(&journal)?)?;
    uuid(&transaction.snapshot)?;
    if transaction
        .data_existing
        .iter()
        .any(|name| !DATA_ENTRIES.contains(&name.as_str()))
    {
        return Err(Failure::new(
            "INVALID_TRANSACTION",
            "恢复事务内容无效，已保留原数据",
        ));
    }
    safe_dir(&base.join("restore-points"))?;
    safe_dir(&base.join("restore-points").join(&transaction.snapshot))?;
    let current = base.join("managed");
    let previous = base
        .join("restore-points")
        .join(&transaction.snapshot)
        .join("previous");
    safe_dir(&current)?;
    safe_dir(&previous)?;
    if previous.exists() {
        if current.exists() {
            fs::remove_dir_all(&current)?;
        }
        fs::rename(previous, current)?;
    } else if !transaction.had_previous && current.exists() {
        fs::remove_dir_all(current)?;
    }
    if transaction.restore_data {
        if transaction
            .data_existing
            .iter()
            .any(|name| !DATA_ENTRIES.contains(&name.as_str()))
        {
            return Err(Failure::new(
                "INVALID_TRANSACTION",
                "恢复事务内容无效，已保留原数据",
            ));
        }
        let home = base.join("agent-data");
        safe_dir(&home)?;
        fs::create_dir_all(&home)?;
        let previous = base
            .join("restore-points")
            .join(&transaction.snapshot)
            .join("runtime-previous");
        safe_dir(&previous)?;
        for name in DATA_ENTRIES {
            if !transaction.workshop_data&&WORKSHOP_ENTRIES.contains(name){continue;}
            if transaction.settings_only&&*name!="settings.json"{continue;}
            let original = previous.join(name);
            let current = home.join(name);
            if original.exists() {
                remove_entry(&current)?;
                fs::rename(original, current)?;
            } else if !transaction.data_existing.iter().any(|s| s == name) {
                remove_entry(&current)?;
            }
        }
        let pending = base.join("data-next");
        safe_dir(&pending)?;
        if pending.exists() {
            fs::remove_dir_all(pending)?;
        }
    }
    let staging = base.join("managed-next");
    safe_dir(&staging)?;
    if staging.exists() {
        fs::remove_dir_all(staging)?;
    }
    fs::remove_file(journal)?;
    Ok(())
}
pub fn validate_restore(root: &Path, instance: &Instance, id: &str) -> Result<RestorePoint> {
    uuid(id)?;
    let point = points(root, instance)?
        .into_iter()
        .find(|p| p.id == id)
        .ok_or_else(|| Failure::new("SNAPSHOT_MISSING", "恢复点不存在"))?;
    if !crate::engine::installed(root, &point.recipe.engine, &point.recipe.profile_id) {
        return Err(Failure::new(
            "OFFLINE_ARTIFACT_MISSING",
            "恢复所需固定引擎工件不在缓存中，请先安装对应组合",
        ));
    }
    let source = directory(root, &instance.id)?
        .join("restore-points")
        .join(&point.id)
        .join("agent-data");
    if point.data_version >= 1 && !source.is_dir() {
        return Err(Failure::new(
            "SNAPSHOT_DATA_MISSING",
            "恢复点的数据目录缺失，未改动当前实例",
        ));
    }
    if point.data_version > 2 {
        return Err(Failure::new("SNAPSHOT_VERSION", "恢复点数据版本不受支持，未改动当前实例"));
    }
    if point.data_version >= 1 {
        safe_dir(&source)?;
        fn validate_tree(path: &Path) -> Result<()> {
            let metadata = check_entry(path)?;
            if metadata.is_dir() {
                for entry in fs::read_dir(path)? { validate_tree(&entry?.path())?; }
            } else if metadata.is_file() {
                fs::File::open(path)?;
            } else {
                return Err(Failure::new("SNAPSHOT_FILE", "恢复点包含不支持的文件类型"));
            }
            Ok(())
        }
        for name in DATA_ENTRIES {
            let path = source.join(name);
            match fs::symlink_metadata(&path) {
                Ok(_) => validate_tree(&path)?,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {},
                Err(error) => return Err(error.into()),
            }
        }
    }
    Ok(point)
}
pub fn restore(root: &Path, instance: &Instance, id: &str) -> Result<RestorePoint> {
    let point = validate_restore(root, instance, id)?;
    let source = directory(root, &instance.id)?.join("restore-points").join(&point.id).join("agent-data");
    transact(
        root,
        instance,
        &point.recipe,
        "恢复前自动备份",
        if point.data_version >= 1 {
            Some((&source,point.data_version))
        } else {
            None
        },
        None,
    )
}

pub fn extension_files(root: &Path, instance: &Instance, install: &Path) -> Result<Vec<PathBuf>> {
    let selection = recipe(root, instance)?;
    selection.validate()?;
    let destination = directory(root, &instance.id)?.join("config/managed-extensions");
    safe_dir(&destination)?;
    fs::create_dir_all(&destination)?;
    let mut files = vec![];
    for extension in selection
        .extensions
        .iter()
        .filter(|e| e.enabled && e.id.ends_with("-local-clock"))
    {
        let mut source = if instance.engine == "Pi" {
            include_str!("local-clock-pi.mjs").to_string()
        } else {
            let module = url::Url::from_file_path(
                install.join("node_modules/@deepseek-ai/dsh-tools/lib/index.js"),
            )
            .map_err(|_| Failure::new("EXTENSION_PATH", "扩展依赖路径无效"))?;
            include_str!("local-clock-dsh.mjs").replace("__DSH_TOOLS_URL__", module.as_str())
        };
        if extension.version == "1.0.0" {
            source = source.replace("Intl.DateTimeFormat().resolvedOptions().timeZone", "'UTC'");
        }
        let file = destination.join(format!("{}-{}.mjs", extension.id, extension.version));
        fs::write(&file, source)?;
        files.push(file);
    }
    Ok(files)
}

pub fn skill_roots(root: &Path, instance: &Instance) -> Result<Vec<PathBuf>> {
    let selected = recipe(root, instance)?;
    crate::local_skills::validate_selection(root,&selected)?;
    let base = directory(root, &instance.id)?.join("config/managed-skills");
    safe_dir(&base)?;
    let mut paths = vec![];
    for skill in selected.extensions.iter().filter(|e|e.enabled&&crate::local_skills::valid_id(&e.id)){
        let source=root.join("local-skills").join(&skill.id).join("content");
        if !source.join("SKILL.md").is_file(){return Err(Failure::new("SKILL_MISSING","本地 Skill 内容缺失，请重新导入"));}
        let parent=base.join(&skill.id);safe_dir(&parent)?;
        let target=parent.join("skill");safe_dir(&target)?;
        if !target.exists(){crate::local_skills::copy_atomic(&source,&target).map_err(|e|Failure::new("SKILL_COPY",&e))?;}
        paths.push(parent);
    }
    for skill in selected
        .extensions
        .iter()
        .filter(|e| e.enabled && e.id.ends_with("-review-checklist"))
    {
        let directory = base.join(format!("{}-{}", skill.id, skill.version));
        safe_dir(&directory)?;
        let bundle = directory.join("perch-review-checklist");
        safe_dir(&bundle)?;
        fs::create_dir_all(&bundle)?;
        fs::write(bundle.join("SKILL.md"), include_str!("review-checklist.md"))?;
        paths.push(directory);
    }
    Ok(paths)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn pi_package_adoption_updates_recipe_and_settings_together(){
        let root=std::env::temp_dir().join(format!("perch-package-adopt-{}",uuid::Uuid::new_v4()));
        let instance=Instance{schema_version:1,id:uuid::Uuid::new_v4().to_string(),name:"fixture".into(),engine:"Pi".into(),project_path:root.join("project").to_string_lossy().into(),connection_id:None,profile_id:crate::engine_profile::PI.id.into(),revision:1,created_at:0,updated_at:0};
        let base=root.join("instances").join(&instance.id);let home=base.join("agent-data");fs::create_dir_all(&home).unwrap();
        // Use an existing locked dependency as a declaration fixture; no package executes.
        let package:serde_json::Value=serde_json::from_str(crate::engine_profile::PI.package).unwrap();
        let mut selected=recipe(&root,&instance).unwrap();
        selected.extensions.push(crate::catalog::ExtensionRef{id:"npm:@agegr/pi-web".into(),version:package["dependencies"]["@agegr/pi-web"].as_str().unwrap().into(),enabled:true,resource_rules:Default::default(),disabled_resources:vec![]});
        apply(&root,&instance,&selected,"fixture").unwrap();
        let source=root.join("artifacts").join(&instance.profile_id).join("node_modules/@agegr/pi-web").to_string_lossy().to_string();
        let settings=serde_json::json!({"packages":["manual",{"source":source,"skills":[]}],"perchManagedPackages":[source],"theme":"custom"});
        write_json(&home.join("settings.json"),&settings).unwrap();fs::write(home.join("models.json"),"original models").unwrap();
        let preview=package_drift(&root,&instance).unwrap();assert!(preview.adoption_error.is_none());
        let point=adopt_package_selection(&root,&instance,&preview.token,&selected).unwrap();
        let adopted=recipe(&root,&instance).unwrap();assert_eq!(adopted.extensions[0].disabled_resources,vec!["skills"]);
        assert!(package_drift(&root,&instance).unwrap().changes.is_empty());
        assert_eq!(fs::read_to_string(home.join("models.json")).unwrap(),"original models");
        let saved:serde_json::Value=serde_json::from_slice(&fs::read(home.join("settings.json")).unwrap()).unwrap();assert_eq!(saved["packages"],settings["packages"]);assert_eq!(saved["theme"],"custom");
        assert_eq!(point.recipe,selected);
        let mut removed=saved.clone();removed["packages"]=serde_json::json!(["manual"]);write_json(&home.join("settings.json"),&removed).unwrap();
        let preview=package_drift(&root,&instance).unwrap();adopt_package_selection(&root,&instance,&preview.token,&adopted).unwrap();
        assert!(!recipe(&root,&instance).unwrap().extensions[0].enabled);
        let unsupported=serde_json::json!({"packages":[{"source":source,"skills":["a","!b"]}],"perchManagedPackages":[source]});
        write_json(&home.join("settings.json"),&unsupported).unwrap();
        assert!(package_drift(&root,&instance).unwrap().adoption_error.is_some());
        // Simulate interruption after both replacements but before journal removal.
        let before=recipe(&root,&instance).unwrap();
        let backup=snapshot(&root,&instance,"interrupted adoption fixture").unwrap();
        let point_dir=base.join("restore-points").join(&backup.id);
        fs::rename(base.join("managed"),point_dir.join("previous")).unwrap();
        fs::create_dir(base.join("managed")).unwrap();write_json(&base.join("managed/recipe.json"),&selected).unwrap();
        fs::create_dir(point_dir.join("runtime-previous")).unwrap();
        fs::rename(home.join("settings.json"),point_dir.join("runtime-previous/settings.json")).unwrap();
        write_json(&home.join("settings.json"),&serde_json::json!({"packages":[]})).unwrap();
        write_json(&base.join("managed-transaction.json"),&Transaction{workshop_data:false,snapshot:backup.id,had_previous:true,restore_data:true,settings_only:true,data_existing:vec!["settings.json".into(),"models.json".into()]}).unwrap();
        recover(&root,&instance.id).unwrap();
        assert_eq!(recipe(&root,&instance).unwrap(),before);
        assert_eq!(serde_json::from_slice::<serde_json::Value>(&fs::read(home.join("settings.json")).unwrap()).unwrap(),unsupported);
        assert_eq!(fs::read_to_string(home.join("models.json")).unwrap(),"original models");
    }
    #[test]
    fn pi_package_drift_restore_preserves_manual_settings_and_rejects_stale_preview(){
        let root=std::env::temp_dir().join(format!("perch-package-drift-{}",uuid::Uuid::new_v4()));
        let instance=Instance{schema_version:1,id:uuid::Uuid::new_v4().to_string(),name:"fixture".into(),engine:"Pi".into(),project_path:root.join("project").to_string_lossy().into(),connection_id:None,profile_id:crate::engine_profile::PI.id.into(),revision:1,created_at:0,updated_at:0};
        let home=root.join("instances").join(&instance.id).join("agent-data");fs::create_dir_all(&home).unwrap();
        let original=serde_json::json!({"packages":["manual",{"source":"managed","skills":[]}],"perchManagedPackages":["managed"],"theme":"custom"});
        write_json(&home.join("settings.json"),&original).unwrap();
        let preview=package_drift(&root,&instance).unwrap();assert_eq!(preview.changes.len(),1);
        assert_eq!(restore_package_selection(&root,&instance,"stale").unwrap_err().code,"SETTINGS_CHANGED");
        assert_eq!(serde_json::from_slice::<serde_json::Value>(&fs::read(home.join("settings.json")).unwrap()).unwrap(),original);
        let point=restore_package_selection(&root,&instance,&preview.token).unwrap();
        let restored:serde_json::Value=serde_json::from_slice(&fs::read(home.join("settings.json")).unwrap()).unwrap();
        assert_eq!(restored["packages"],serde_json::json!(["manual","managed"]));assert_eq!(restored["theme"],"custom");
        let backup=root.join("instances").join(&instance.id).join("restore-points").join(point.id).join("agent-data/settings.json");
        assert_eq!(serde_json::from_slice::<serde_json::Value>(&fs::read(backup).unwrap()).unwrap(),original);
        assert!(package_drift(&root,&instance).unwrap().changes.is_empty());
    }
    #[test]
    fn changes_keep_restore_points_and_recover_interrupted_swap() {
        let root = std::env::temp_dir().join(format!("perch-managed-{}", uuid::Uuid::new_v4()));
        let id = uuid::Uuid::new_v4().to_string();
        let instance = Instance {
            schema_version: 1,
            id: id.clone(),
            name: "test".into(),
            engine: "Pi".into(),
            project_path: root.join("project").to_string_lossy().into(),
            connection_id: Some(uuid::Uuid::new_v4().to_string()),
            profile_id: crate::engine_profile::PI.id.into(),
            revision: 1,
            created_at: 0,
            updated_at: 0,
        };
        fs::create_dir_all(root.join("project")).unwrap();
        fs::write(root.join("project/keep.txt"), "user source").unwrap();
        let data = root.join("instances").join(&id).join("agent-data");
        fs::create_dir_all(data.join("sessions")).unwrap();
        fs::write(data.join("sessions/test.jsonl"), "original history").unwrap();
        fs::write(data.join("auth.json"), "original credential").unwrap();
        let mut selected = crate::catalog::packs().remove(1);
        let first = apply(&root, &instance, &selected, "install").unwrap();
        assert!(first.recipe.extensions.is_empty());
        selected.extensions[0].enabled = false;
        let second = apply(&root, &instance, &selected, "disable").unwrap();
        assert!(second.recipe.extensions[0].enabled);
        assert!(!recipe(&root, &instance).unwrap().extensions[0].enabled);
        assert_eq!(
            restore(&root, &instance, &first.id).unwrap_err().code,
            "OFFLINE_ARTIFACT_MISSING"
        );
        let profile = &crate::engine_profile::PI;
        let artifact = root.join("artifacts").join(profile.id);
        fs::create_dir_all(artifact.join(profile.entry).parent().unwrap()).unwrap();
        fs::write(artifact.join(profile.entry), "test fixture").unwrap();
        fs::write(artifact.join("package-lock.json"), profile.lock).unwrap();
        fs::write(artifact.join("perch-install.json"), profile.marker()).unwrap();
        fs::write(data.join("sessions/test.jsonl"), "new history").unwrap();
        fs::write(data.join("auth.json"), "rotated credential").unwrap();
        let before_restore = restore(&root, &instance, &first.id).unwrap();
        assert_eq!(
            fs::read_to_string(data.join("sessions/test.jsonl")).unwrap(),
            "original history"
        );
        assert_eq!(
            fs::read_to_string(data.join("auth.json")).unwrap(),
            "rotated credential"
        );
        let backup_dir = root
            .join("instances")
            .join(&id)
            .join("restore-points")
            .join(before_restore.id);
        assert_eq!(
            fs::read_to_string(backup_dir.join("agent-data/sessions/test.jsonl")).unwrap(),
            "new history"
        );
        assert!(!backup_dir.join("agent-data/auth.json").exists());
        apply(&root, &instance, &selected, "prepare interrupted test").unwrap();
        let base = directory(&root, &id).unwrap();
        let backup = snapshot(&root, &instance, "interrupted").unwrap();
        fs::rename(
            base.join("managed"),
            base.join("restore-points")
                .join(&backup.id)
                .join("previous"),
        )
        .unwrap();
        fs::create_dir(base.join("managed-next")).unwrap();
        fs::write(
            base.join("managed-transaction.json"),
            serde_json::to_vec(&Transaction { workshop_data:false,
                snapshot: backup.id,
                had_previous: true,
                restore_data: false,
                settings_only: false,
                data_existing: vec![],
            })
            .unwrap(),
        )
        .unwrap();
        recover(&root, &id).unwrap();
        assert_eq!(recipe(&root, &instance).unwrap(), selected);
        let interrupted = snapshot(&root, &instance, "data restore interrupted").unwrap();
        let previous = base
            .join("restore-points")
            .join(&interrupted.id)
            .join("runtime-previous");
        fs::create_dir_all(&previous).unwrap();
        fs::rename(data.join("sessions"), previous.join("sessions")).unwrap();
        fs::create_dir(data.join("sessions")).unwrap();
        fs::write(data.join("sessions/test.jsonl"), "partial restore").unwrap();
        write_json(
            &base.join("managed-transaction.json"),
            &Transaction { workshop_data:false,
                snapshot: interrupted.id,
                had_previous: true,
                restore_data: true,
                settings_only: false,
                data_existing: vec!["sessions".into()],
            },
        )
        .unwrap();
        recover(&root, &id).unwrap();
        assert_eq!(
            fs::read_to_string(data.join("sessions/test.jsonl")).unwrap(),
            "original history"
        );
        assert_eq!(
            fs::read_to_string(data.join("auth.json")).unwrap(),
            "rotated credential"
        );
        assert_eq!(
            fs::read_to_string(root.join("project/keep.txt")).unwrap(),
            "user source"
        );
        assert!(points(&root, &instance).unwrap().len() >= 3);
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn workshop_snapshots_restore_new_scope_and_preserve_legacy_scope(){
        let root=std::env::temp_dir().join(format!("perch-workshop-snapshot-{}",uuid::Uuid::new_v4()));
        let instance=Instance{schema_version:1,id:uuid::Uuid::new_v4().to_string(),name:"fixture".into(),engine:"DSH".into(),project_path:root.to_string_lossy().into(),connection_id:None,profile_id:crate::engine_profile::DSH.id.into(),revision:1,created_at:0,updated_at:0};
        let base=directory(&root,&instance.id).unwrap();let home=base.join("agent-data");
        for name in WORKSHOP_ENTRIES{fs::create_dir_all(home.join(name)).unwrap();fs::write(home.join(name).join("fixture"),"before").unwrap();}
        let point=snapshot(&root,&instance,"resource fixture").unwrap();assert_eq!(point.data_version,2);
        let source=base.join("restore-points").join(&point.id).join("agent-data");
        for name in WORKSHOP_ENTRIES{assert_eq!(fs::read_to_string(source.join(name).join("fixture")).unwrap(),"before");fs::write(home.join(name).join("fixture"),"after").unwrap();}
        transact(&root,&instance,&point.recipe,"new restore",Some((&source,2)),None).unwrap();
        for name in WORKSHOP_ENTRIES{assert_eq!(fs::read_to_string(home.join(name).join("fixture")).unwrap(),"before");fs::write(home.join(name).join("fixture"),"keep current").unwrap();}
        transact(&root,&instance,&point.recipe,"legacy restore",Some((&source,1)),None).unwrap();
        for name in WORKSHOP_ENTRIES{assert_eq!(fs::read_to_string(home.join(name).join("fixture")).unwrap(),"keep current");}
        let backup=snapshot(&root,&instance,"legacy journal").unwrap();
        write_json(&base.join("managed-transaction.json"),&serde_json::json!({"snapshot":backup.id,"had_previous":true,"restore_data":true,"settings_only":false,"data_existing":[]})).unwrap();
        recover(&root,&instance.id).unwrap();
        for name in WORKSHOP_ENTRIES{assert_eq!(fs::read_to_string(home.join(name).join("fixture")).unwrap(),"keep current");}
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn share_guard_dsh_discovery_honors_disabled_roots_and_git_root(){
        let root=std::env::temp_dir().join(format!("perch-dsh-discovery-{}",uuid::Uuid::new_v4()));
        let project=root.join("project");fs::create_dir_all(project.join(".git")).unwrap();fs::create_dir_all(project.join("nested")).unwrap();
        let instance=Instance{schema_version:1,id:uuid::Uuid::new_v4().to_string(),name:"fixture".into(),engine:"DSH".into(),project_path:project.join("nested").to_string_lossy().into(),connection_id:None,profile_id:crate::engine_profile::DSH.id.into(),revision:1,created_at:0,updated_at:0};
        let home=directory(&root,&instance.id).unwrap().join("agent-data");fs::create_dir_all(home.join("skills/.system")).unwrap();fs::write(home.join("skills/.system/built-in"),"builtin").unwrap();
        let config=serde_json::json!({"skill-filesystem":{"agentsHome":root.join("agents").to_string_lossy()}});
        fs::write(home.join("settings.yaml"),serde_json::to_vec(&config).unwrap()).unwrap();
        assert!(validate_dsh_discovery(&root,&instance).is_ok());
        let skill=project.join(".agents/skills/test");fs::create_dir_all(&skill).unwrap();fs::write(skill.join("SKILL.md"),"keep").unwrap();
        assert_eq!(validate_dsh_discovery(&root,&instance).unwrap_err().code,"PACK_UNMANAGED_RESOURCES");
        let mut disabled=config;disabled["skill-filesystem"]["includeDefaultRoots"]=serde_json::json!(false);
        fs::write(home.join("settings.yaml"),serde_json::to_vec(&disabled).unwrap()).unwrap();
        assert!(validate_dsh_discovery(&root,&instance).is_ok());assert_eq!(fs::read_to_string(skill.join("SKILL.md")).unwrap(),"keep");
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn share_guard_pi_discovery_preserves_project_and_managed_resources(){
        let root=std::env::temp_dir().join(format!("perch-discovery-{}",uuid::Uuid::new_v4()));
        let instance=Instance{schema_version:1,id:uuid::Uuid::new_v4().to_string(),name:"fixture".into(),engine:"Pi".into(),project_path:root.join("project").to_string_lossy().into(),connection_id:None,profile_id:crate::engine_profile::PI.id.into(),revision:1,created_at:0,updated_at:0};
        let base=directory(&root,&instance.id).unwrap();
        let managed=base.join("config/managed-skills/fixture");fs::create_dir_all(&managed).unwrap();fs::write(managed.join("SKILL.md"),"managed").unwrap();
        let project=Path::new(&instance.project_path).join(".pi");fs::create_dir_all(project.join("skills")).unwrap();
        assert!(validate_pi_discovery(&root,&instance).is_ok());
        let skill=project.join("skills/SKILL.md");fs::write(&skill,"private project skill").unwrap();
        assert_eq!(validate_pi_discovery(&root,&instance).unwrap_err().code,"PACK_UNMANAGED_RESOURCES");
        assert_eq!(fs::read_to_string(&skill).unwrap(),"private project skill");fs::remove_file(skill).unwrap();
        let settings=project.join("settings.json");
        for text in [r#"{"packages":["npm:local"]}"#,r#"{"skills":{}}"#,"not json"] {
            fs::write(&settings,text).unwrap();assert!(validate_pi_discovery(&root,&instance).is_err());assert_eq!(fs::read_to_string(&settings).unwrap(),text);
        }
        fs::write(&settings,r#"{"skills":[],"apiKey":"not-exported"}"#).unwrap();assert!(validate_pi_discovery(&root,&instance).is_ok());
        let home=base.join("agent-data");fs::create_dir_all(&home).unwrap();fs::write(home.join("SYSTEM.md"),"private prompt").unwrap();
        assert_eq!(validate_pi_discovery(&root,&instance).unwrap_err().code,"PACK_UNMANAGED_RESOURCES");
        assert_eq!(fs::read_to_string(home.join("SYSTEM.md")).unwrap(),"private prompt");
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn share_guard_rejects_unmanaged_or_changed_pi_packages_without_writes(){
        let root=std::env::temp_dir().join(format!("perch-share-guard-{}",uuid::Uuid::new_v4()));
        let instance=Instance{schema_version:1,id:uuid::Uuid::new_v4().to_string(),name:"fixture".into(),engine:"Pi".into(),project_path:root.to_string_lossy().into(),connection_id:None,profile_id:crate::engine_profile::PI.id.into(),revision:1,created_at:0,updated_at:0};
        let home=directory(&root,&instance.id).unwrap().join("agent-data");fs::create_dir_all(&home).unwrap();
        let path=home.join("settings.json");
        for (value,code) in [
            (serde_json::json!({"packages":["manual"],"perchManagedPackages":[]}),Some("PACK_UNMANAGED_RESOURCES")),
            (serde_json::json!({"packages":[{"source":"owned","themes":[]}],"perchManagedPackages":["owned"]}),Some("PACK_RESOURCE_DRIFT")),
            (serde_json::json!({"packages":[],"perchManagedPackages":["owned"]}),Some("PACK_RESOURCE_DRIFT")),
            (serde_json::json!({"packages":["owned"],"perchManagedPackages":["owned"]}),None),
            (serde_json::json!({"prompts":["private/local-prompt.md"]}),Some("PACK_UNMANAGED_RESOURCES")),
            (serde_json::json!({"themes":["private/local-theme.json"]}),Some("PACK_UNMANAGED_RESOURCES")),
            (serde_json::json!({"prompts":{},"themes":[]}),Some("SETTINGS_INVALID")),
            (serde_json::json!({"prompts":[],"themes":[],"packages":[{"source":"owned","prompts":[],"themes":[]}],"perchManagedPackages":[{"source":"owned","prompts":[],"themes":[]}]}),None),
        ] {
            let bytes=serde_json::to_vec(&value).unwrap();fs::write(&path,&bytes).unwrap();
            let result=validate_shareable_packages(&root,&instance);
            assert_eq!(result.err().map(|e|e.code),code.map(str::to_string));
            assert_eq!(fs::read(&path).unwrap(),bytes);
        }
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn public_preset_capture_uses_actual_value_and_excludes_private_settings(){
        let root=std::env::temp_dir().join(format!("perch-preset-capture-{}",uuid::Uuid::new_v4()));
        for engine in ["DSH","Pi"] {
            let instance=Instance{schema_version:1,id:uuid::Uuid::new_v4().to_string(),name:"fixture".into(),engine:engine.into(),project_path:root.to_string_lossy().into(),connection_id:None,profile_id:if engine=="Pi"{crate::engine_profile::PI.id.into()}else{crate::engine_profile::DSH.id.into()},revision:1,created_at:0,updated_at:0};
            let mut recipe=crate::catalog::packs().into_iter().find(|recipe|recipe.engine==engine).unwrap();
            recipe.thinking_level=Some("low".into());
            let home=directory(&root,&instance.id).unwrap().join("agent-data");fs::create_dir_all(&home).unwrap();
            let path=home.join(if engine=="Pi"{"settings.json"}else{"settings.yaml"});
            let content=if engine=="Pi"{r#"{"defaultThinkingLevel":"high","apiKey":"fixture-private"}"#}else{"agent-default-model:\n  reasoningEffort: high\nprivate-key: fixture-private\n"};
            fs::write(&path,content).unwrap();
            capture_public_preset(&root,&instance,&mut recipe).unwrap();
            assert_eq!(recipe.thinking_level.as_deref(),Some("high"));
            assert!(!serde_json::to_string(&recipe).unwrap().contains("fixture-private"));
            assert_eq!(fs::read_to_string(&path).unwrap(),content);
            fs::write(&path,content.replace("high","unsupported")).unwrap();
            assert!(capture_public_preset(&root,&instance,&mut recipe).is_err());
            assert_eq!(recipe.thinking_level.as_deref(),Some("high"));
        }
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn workshop_install_preserves_other_assets_and_rejects_overwrite(){
        use sha2::{Digest,Sha256};
        let root=std::env::temp_dir().join(format!("perch-workshop-install-{}",uuid::Uuid::new_v4()));
        let instance=Instance{schema_version:1,id:uuid::Uuid::new_v4().to_string(),name:"fixture".into(),engine:"DSH".into(),project_path:root.to_string_lossy().into(),connection_id:None,profile_id:crate::engine_profile::DSH.id.into(),revision:1,created_at:0,updated_at:0};
        let base=directory(&root,&instance.id).unwrap();let home=base.join("agent-data");
        fs::create_dir_all(home.join("skins/old")).unwrap();fs::write(home.join("skins/old/skin.json"),"old").unwrap();
        let content=root.join("fixture");fs::create_dir_all(&content).unwrap();fs::write(content.join("skin.json"),"new").unwrap();
        fs::write(content.join("unlisted.js"),"must not install").unwrap();
        let receipt=serde_json::json!({"schemaVersion":1,"plan":{"version":"1","repository":"example/skins","commit":"a".repeat(40),"contentPath":"skins","directory":"skins/new"},"sha256":{"skin.json":format!("{:x}",Sha256::digest(b"new"))}});
        let mut bad=receipt.clone();bad["sha256"]["skin.json"]=serde_json::json!("b".repeat(64));
        assert!(install_workshop_files(&root,&instance,"skins/new",&content,&bad).is_err());
        assert!(!home.join("skins/new").exists());
        assert_eq!(fs::read_to_string(home.join("skins/old/skin.json")).unwrap(),"old");
        let point=install_workshop_files(&root,&instance,"skins/new",&content,&receipt).unwrap();
        assert!(!home.join("skins/new/unlisted.js").exists());
        assert_eq!(fs::read_to_string(home.join("skins/new/skin.json")).unwrap(),"new");
        assert_eq!(fs::read_to_string(home.join("skins/old/skin.json")).unwrap(),"old");
        assert!(install_workshop_files(&root,&instance,"skins/new",&content,&receipt).is_err());
        assert!(install_workshop_files(&root,&instance,"skins/../outside",&content,&serde_json::json!({"schemaVersion":1})).is_err());
        let source=base.join("restore-points").join(point.id).join("agent-data");
        transact(&root,&instance,&point.recipe,"undo install",Some((&source,2)),None).unwrap();
        assert!(!home.join("skins/new").exists());assert!(home.join("skins/old/skin.json").exists());
        fs::remove_dir_all(root).unwrap();
    }
}
