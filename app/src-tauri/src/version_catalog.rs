use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{fs, time::Duration};
use tauri::Manager;

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VersionEntry {
    version: String,
    preview: bool,
    runtime: String,
    integrity: String,
    artifact: String,
    #[serde(default)]
    release_url: String,
    #[serde(default)]
    commit: String,
    #[serde(default)]
    pi_dependency: String,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VersionSource {
    id: String,
    label: String,
    package: String,
    entries: Vec<VersionEntry>,
    synced_at: u64,
    error: Option<String>,
    #[serde(default)]
    github_complete: bool,
}
const SOURCES: &[(&str, &str, &str)] = &[
    ("dsh", "DSH", "@deepseek-ai/dsh"),
    ("pi", "Pi", "@earendil-works/pi-coding-agent"),
    ("pi-legacy", "Pi 历史来源", "@mariozechner/pi-coding-agent"),
    ("pi-web", "Pi Web", "@agegr/pi-web"),
];

#[tauri::command]
pub async fn version_catalog(
    app: tauri::AppHandle,
    refresh: bool,
) -> Result<Vec<VersionSource>, String> {
    let root = app
        .state::<crate::data_commands::DataState>()
        .root
        .as_ref()
        .map_err(|_| "工作空间不可用")?
        .clone();
    tauri::async_runtime::spawn_blocking(move || {
        let dir = root.join("catalog-versions");
        fs::create_dir_all(&dir).map_err(|_| "目录缓存无法写入")?;
        let client = reqwest::blocking::Client::builder()
            .timeout(Duration::from_secs(25))
            .redirect(reqwest::redirect::Policy::none())
            .user_agent("Perch/0.1.0")
            .build()
            .map_err(|_| "无法初始化目录请求")?;
        let _guard=crate::resource_catalog::CacheRefresh::begin(&dir)?;
        let mut sources = Vec::new();
        for (id, label, package) in SOURCES {
            let path = dir.join(format!("{id}.json"));
            let cached: Option<VersionSource> = fs::read(&path)
                .ok()
                .and_then(|bytes| serde_json::from_slice(&bytes).ok());
            let mut source = cached.clone().unwrap_or(VersionSource {
                id: id.to_string(),
                label: label.to_string(),
                package: package.to_string(),
                entries: vec![],
                synced_at: 0,
                error: None,
                github_complete: false,
            });
            if refresh || cached.is_none() {
                let result = (|| -> Result<Vec<VersionEntry>, String> {
                    let doc=crate::resource_catalog::conditional_json_with(&root,&client,&format!("https://registry.npmjs.org/{}",package.replace('/',"%2F")),Some("application/vnd.npm.install-v1+json"),32*1024*1024)?;
                    let versions = doc["versions"].as_object().ok_or("来源没有版本目录")?;
                    let mut entries: Vec<_> = versions
                        .iter()
                        .map(|(version, item)| VersionEntry {
                            version: version.clone(),
                            preview: version.contains('-'),
                            runtime: item["engines"]["node"].as_str().unwrap_or("未声明").into(),
                            integrity: item["dist"]["integrity"].as_str().unwrap_or("").into(),
                            artifact: item["dist"]["tarball"].as_str().unwrap_or("").into(),
                            release_url: String::new(),
                            commit: String::new(),
                            pi_dependency: item["dependencies"]["@earendil-works/pi-coding-agent"]
                                .as_str()
                                .or_else(|| {
                                    item["dependencies"]["@mariozechner/pi-coding-agent"].as_str()
                                })
                                .unwrap_or("")
                                .into(),
                        })
                        .collect();
                    entries.sort_by(|a, b| version_cmp(&b.version, &a.version));
                    Ok(entries)
                })();
                match result {
                    Ok(mut entries) => {
                        let repository=match *id {"dsh"=>"deepseek-ai/deepseek-harness","pi"|"pi-legacy"=>"earendil-works/pi",_=>"agegr/pi-web"};
                        let github=enrich_github(&root,&client,repository,&mut entries);
                        let now=std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_secs();
                        apply_version_refresh(&mut source,entries,github,now);
                    }
                    Err(error) => source.error = Some(error),
                }
                if let Ok(bytes)=serde_json::to_vec(&source){if crate::resource_catalog::save_cache(&path,&bytes).is_err(){source.error=Some("目录缓存写入失败，原缓存保留".into());}}

            }
            sources.push(source);
        }
        Ok(sources)
    })
    .await
    .map_err(|_| "版本目录任务中断".to_string())?
}

fn apply_version_refresh(source:&mut VersionSource,mut entries:Vec<VersionEntry>,github:Result<(),String>,now:u64){
    entries.sort_by(|a,b|version_cmp(&b.version,&a.version));
    match github{
        Ok(())=>{source.entries=entries;source.github_complete=true;source.synced_at=now;source.error=None;}
        Err(error)=>{if source.entries.is_empty(){source.entries=entries;source.github_complete=false;}source.error=Some(format!("{error}；GitHub 目录未同步完整，已有目录保留"));}
    }
}
fn version_cmp(a: &str, b: &str) -> std::cmp::Ordering {
    fn parts(v: &str) -> (Vec<u64>, Option<&str>) {
        let (base, pre) = v
            .trim_start_matches('v')
            .split_once('-')
            .map_or((v.trim_start_matches('v'), None), |(a, b)| (a, Some(b)));
        (
            base.split('.').map(|n| n.parse().unwrap_or(0)).collect(),
            pre,
        )
    }
    let (a_num, a_pre) = parts(a);
    let (b_num, b_pre) = parts(b);
    a_num.cmp(&b_num).then_with(|| match (a_pre, b_pre) {
        (None, None) => std::cmp::Ordering::Equal,
        (None, Some(_)) => std::cmp::Ordering::Greater,
        (Some(_), None) => std::cmp::Ordering::Less,
        (Some(a), Some(b)) => a
            .split('.')
            .zip(b.split('.'))
            .find_map(|(a, b)| {
                let order = match (a.parse::<u64>(), b.parse::<u64>()) {
                    (Ok(a), Ok(b)) => a.cmp(&b),
                    _ => a.cmp(b),
                };
                (order != std::cmp::Ordering::Equal).then_some(order)
            })
            .unwrap_or_else(|| a.split('.').count().cmp(&b.split('.').count())),
    })
}
fn enrich_github(
    root: &std::path::Path,
    client: &reqwest::blocking::Client,
    repo: &str,
    entries: &mut Vec<VersionEntry>,
) -> Result<(), String> {
    for kind in ["releases", "tags"] {
        let mut finished = false;
        for page in 1..=20 {
            let doc=crate::resource_catalog::conditional_json(root,client,&format!("https://api.github.com/repos/{repo}/{kind}?per_page=100&page={page}"))?;
            let rows=doc.as_array().ok_or("GitHub 目录格式无法识别")?;
            for row in rows {
                let tag = row["tag_name"]
                    .as_str()
                    .or_else(|| row["name"].as_str())
                    .unwrap_or("");
                if tag.is_empty() {
                    continue;
                }
                let version = tag
                    .strip_prefix("dsh-v")
                    .unwrap_or(tag)
                    .trim_start_matches('v');
                let position = entries.iter().position(|entry| entry.version == version);
                let entry = if let Some(index) = position {
                    &mut entries[index]
                } else {
                    entries.push(VersionEntry {
                        version: version.into(),
                        preview: version.contains('-'),
                        runtime: "未声明".into(),
                        integrity: String::new(),
                        artifact: String::new(),
                        release_url: String::new(),
                        commit: String::new(),
                        pi_dependency: String::new(),
                    });
                    entries.last_mut().unwrap()
                };
                if let Some(url) = row["html_url"].as_str() {
                    entry.release_url = url.into();
                }
                if let Some(commit) = row["commit"]["sha"].as_str() {
                    entry.commit = commit.into();
                }
                if row["prerelease"].as_bool() == Some(true) {
                    entry.preview = true;
                }
            }
            if rows.len() < 100 {
                finished = true;
                break;
            }
        }
        if !finished {
            return Err("GitHub 分页未完成，请稍后刷新；不宣称全量目录".into());
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn incomplete_github_refresh_keeps_previous_versions_and_time(){
        let entry=|version:&str|VersionEntry{version:version.into(),preview:false,runtime:"test".into(),integrity:String::new(),artifact:String::new(),release_url:"https://github.com/example/repo/releases/tag/old".into(),commit:"old-commit".into(),pi_dependency:String::new()};
        let mut source=VersionSource{id:"pi".into(),label:"Pi".into(),package:"fixture".into(),entries:vec![entry("1.0.0")],synced_at:100,error:None,github_complete:true};
        apply_version_refresh(&mut source,vec![entry("2.0.0")],Err("HTTP 429".into()),200);
        assert_eq!(source.entries[0].version,"1.0.0");assert_eq!(source.entries[0].commit,"old-commit");assert_eq!(source.synced_at,100);assert!(source.error.as_ref().unwrap().contains("429"));
        apply_version_refresh(&mut source,vec![entry("2.0.0")],Ok(()),300);assert_eq!(source.entries[0].version,"2.0.0");assert_eq!(source.synced_at,300);assert!(source.error.is_none());
        source.entries.clear();source.synced_at=0;source.github_complete=false;
        apply_version_refresh(&mut source,vec![entry("3.0.0")],Err("offline".into()),400);assert_eq!(source.entries[0].version,"3.0.0");assert_eq!(source.synced_at,0);assert!(!source.github_complete);assert!(source.error.is_some());
    }
    #[test]
    fn version_order_respects_numbers_and_prereleases() {
        use std::cmp::Ordering::Greater;
        assert_eq!(version_cmp("0.10.0", "0.9.3"), Greater);
        assert_eq!(version_cmp("1.0.0", "1.0.0-rc.10"), Greater);
        assert_eq!(version_cmp("1.0.0-rc.10", "1.0.0-rc.2"), Greater);
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VersionPlan {
    pub(crate) engine: String,
    pub(crate) version: String,
    pub(crate) dependencies: Value,
    pub(crate) runtime: String,
    pub(crate) warnings: Vec<String>,
}
#[tauri::command]
pub fn resolve_version_plan(
    app: tauri::AppHandle,
    source_id: String,
    version: String,
) -> Result<VersionPlan, String> {
    let source = SOURCES
        .iter()
        .find(|(id, _, _)| *id == source_id)
        .ok_or("未知版本来源")?;
    let state = app.state::<crate::data_commands::DataState>();
    let root = state.root.as_ref().map_err(|_| "工作空间不可用")?;
    let bytes = fs::read(
        root.join("catalog-versions")
            .join(format!("{}.json", source.0)),
    )
    .map_err(|_| "请先同步版本目录")?;
    let catalog: VersionSource =
        serde_json::from_slice(&bytes).map_err(|_| "目录缓存无法读取，请刷新")?;
    let entry = catalog
        .entries
        .iter()
        .find(|entry| entry.version == version)
        .ok_or("此版本不在目录中，请刷新")?;
    if entry.artifact.is_empty() || entry.integrity.is_empty() {
        return Err("此条目缺少可校验的 npm 工件，暂不能生成安装计划".into());
    }
    let mut dependencies = serde_json::Map::new();
    dependencies.insert(source.2.into(), Value::String(version.clone()));
    let mut warnings = vec!["兼容性尚未实机验证；优先在克隆实例中试用，保留原实例。".into()];
    let engine = if source_id == "dsh" { "DSH" } else { "Pi" };
    if source_id == "pi" || source_id == "pi-legacy" {
        return Err("Pi 引擎需要匹配工作台，请从 Pi Web 版本中选择；不能单独替换 CLI。".into());
    }
    if source_id == "pi-web" {
        if entry.pi_dependency.is_empty() {
            return Err("此 Pi Web 未声明可识别的 Pi 依赖，暂不能自动组合".into());
        }
        let dep = &entry.pi_dependency;
        if dep.starts_with(['^', '~', '>', '<', '*']) || dep.contains(' ') {
            return Err("此工作台使用范围依赖，需要先解析并锁定 Pi 精确版本".into());
        }
        warnings.push(format!(
            "工作台要求 Pi {dep}，将随其依赖锁定，不能任意搭配其他 Pi 版本。"
        ));
    }
    if entry.preview {
        warnings.push("这是上游预览版，更新前保留恢复点。".into());
    }
    Ok(VersionPlan {
        engine: engine.into(),
        version,
        dependencies: Value::Object(dependencies),
        runtime: entry.runtime.clone(),
        warnings,
    })
}

#[tauri::command]
pub fn cancel_version_plan(operation_id:String)->Result<(),String>{
    let tasks=resolution_tasks().lock().map_err(|_|"解析状态不可用")?;
    if let Some(cancel)=tasks.get(&operation_id){cancel.store(true,std::sync::atomic::Ordering::SeqCst);}
    Ok(())
}
fn resolution_tasks()->&'static std::sync::Mutex<std::collections::HashMap<String,std::sync::Arc<std::sync::atomic::AtomicBool>>>{
    static TASKS:std::sync::OnceLock<std::sync::Mutex<std::collections::HashMap<String,std::sync::Arc<std::sync::atomic::AtomicBool>>>>=std::sync::OnceLock::new();
    TASKS.get_or_init(Default::default)
}
#[tauri::command]
pub async fn lock_version_plan(
    app: tauri::AppHandle,
    source_id: String,
    version: String,
    operation_id: String,
) -> Result<String, String> {
    uuid::Uuid::parse_str(&operation_id).map_err(|_|"解析任务标识无效")?;
    let plan = resolve_version_plan(app.clone(), source_id, version)?;
    let root = app
        .state::<crate::data_commands::DataState>()
        .root
        .as_ref()
        .map_err(|_| "工作空间不可用")?
        .clone();
    let cancel=std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    {let mut tasks=resolution_tasks().lock().map_err(|_|"解析状态不可用")?;if tasks.contains_key(&operation_id){return Err("解析任务已存在".into());}tasks.insert(operation_id.clone(),cancel.clone());}
    let result=tauri::async_runtime::spawn_blocking(move||lock_plan_cancellable(&root,plan,&cancel)).await;
    if let Ok(mut tasks)=resolution_tasks().lock(){tasks.remove(&operation_id);}
    result.map_err(|_|"依赖解析任务中断".to_string())?
}
pub(crate) fn lock_plan(root:&std::path::Path,plan:VersionPlan)->Result<String,String>{
    lock_plan_cancellable(root,plan,&std::sync::atomic::AtomicBool::new(false))
}
fn lock_plan_cancellable(root:&std::path::Path,plan:VersionPlan,cancel:&std::sync::atomic::AtomicBool)->Result<String,String>{
        use std::io::{Read,Write};
        if cancel.load(std::sync::atomic::Ordering::SeqCst){return Err("依赖解析已取消，原实例未修改".into());}
        let id=format!("resolved-{}",uuid::Uuid::new_v4());
        let staging=root.join("resolution-staging").join(&id);
        fs::create_dir_all(&staging).map_err(|_|"无法创建依赖解析目录")?;
        let _cleanup=ResolutionStaging(staging.clone());
        let package=serde_json::json!({"name":"perch-resolved-instance","version":"1.0.0","private":true,"dependencies":plan.dependencies}).to_string();
        fs::write(staging.join("package.json"),&package).map_err(|_|"无法写入解析清单")?;
        fs::write(staging.join("host.mjs"),include_str!("engine-host.mjs")).map_err(|_|"无法写入解析入口")?;
        let (node,npm)=crate::engine::node().map_err(|e|e.message)?;
        let mut command=crate::engine::clean_command(&node);
        command.arg(staging.join("host.mjs")).current_dir(&staging);
        let mut process=crate::owned_process::OwnedProcess::spawn(&mut command).map_err(|e|e.message)?;
        let stdout=process.child.stdout.take().unwrap();let stderr=process.child.stderr.take().unwrap();
        let drain=|mut pipe:Box<dyn Read+Send>|std::thread::spawn(move||{
            let mut tail=Vec::new();let mut chunk=[0u8;2048];
            while let Ok(size)=pipe.read(&mut chunk){if size==0{break;}tail.extend_from_slice(&chunk[..size]);if tail.len()>8192{tail.drain(..tail.len()-8192);}}
            String::from_utf8_lossy(&tail).into_owned()
        });
        let out=drain(Box::new(stdout));let err=drain(Box::new(stderr));
        let mut input=process.child.stdin.take().ok_or("无法连接解析进程")?;
        writeln!(input,"{}",serde_json::json!({"mode":"resolve","npm":npm})).map_err(|_|"解析进程已退出")?;
        let start=std::time::Instant::now();
        let success=loop{
            if cancel.load(std::sync::atomic::Ordering::SeqCst){process.terminate();break false;}
            if let Some(status)=process.child.try_wait().map_err(|_|"无法读取解析状态")?{break status.success();}
            if start.elapsed()>Duration::from_secs(180){process.terminate();break false;}
            std::thread::sleep(Duration::from_millis(100));
        };
        drop(input);drop(process);let _=out.join();let diagnostic=err.join().unwrap_or_default();
        if cancel.load(std::sync::atomic::Ordering::SeqCst){return Err("依赖解析已取消，原实例未修改".into());}
        if !success{return Err(resolution_error(&diagnostic).into());}
        let lock=fs::read_to_string(staging.join("package-lock.json")).map_err(|_|"没有生成依赖锁")?;
        let profile=crate::engine_profile::ResolvedProfile{
            id:id.clone(),engine:plan.engine.clone(),entry:if plan.engine=="DSH"{crate::engine_profile::DSH.entry}else{crate::engine_profile::PI.entry}.into(),
            package,lock,node:crate::engine_profile::DSH.node.into(),npm:crate::engine_profile::DSH.npm.into(),
        };
        if cancel.load(std::sync::atomic::Ordering::SeqCst){return Err("依赖解析已取消，原实例未修改".into());}
        crate::engine_profile::register(&root,profile).map_err(|e|e.message)?;
        Ok(id)
}
struct ResolutionStaging(std::path::PathBuf);
impl Drop for ResolutionStaging{
    fn drop(&mut self){
        let Ok(metadata)=fs::symlink_metadata(&self.0) else{return};
        if metadata.file_type().is_symlink(){return;}
        #[cfg(windows)]{
            use std::os::windows::fs::MetadataExt;
            if metadata.file_attributes()&0x400!=0{return;}
        }
        let Some(parent)=self.0.parent() else{return};
        let (Ok(parent),Ok(path))=(parent.canonicalize(),self.0.canonicalize()) else{return};
        if path.parent()==Some(parent.as_path()){let _=fs::remove_dir_all(path);}
    }
}
fn resolution_error(diagnostic:&str)->&'static str{
    if diagnostic.contains("EBADENGINE"){"依赖要求的 Node/npm 版本与当前固定运行时不兼容；请改选版本，原实例未修改"}
    else if diagnostic.contains("EBADPLATFORM"){"依赖不支持当前 Windows x64 平台；请改选版本，原实例未修改"}
    else if diagnostic.contains("ERESOLVE"){"依赖或宿主版本要求冲突，无法生成组合；请调整资源版本，原实例未修改"}
    else if diagnostic.contains("E404"){"指定包或版本在 npm 来源中不存在；请检查名称与版本，原实例未修改"}
    else {"依赖锁定失败或超时；原实例未修改，请检查网络及运行时要求"}
}


#[cfg(test)]
mod real_resolution {
    use super::*;
    #[test]
    fn cancelled_resolution_does_not_register_and_staging_is_cleaned(){
        let root=std::env::temp_dir().join(format!("perch-cancel-resolution-{}",uuid::Uuid::new_v4()));
        let plan=VersionPlan{engine:"Pi".into(),version:"test".into(),dependencies:serde_json::json!({}),runtime:"24".into(),warnings:vec![]};
        assert!(lock_plan_cancellable(&root,plan,&std::sync::atomic::AtomicBool::new(true)).unwrap_err().contains("已取消"));
        assert!(!root.exists());
        let path=root.join("resolution-staging").join("resolved-test");fs::create_dir_all(&path).unwrap();fs::write(path.join("package.json"),"{}").unwrap();
        {let _guard=ResolutionStaging(path.clone());}
        assert!(!path.exists());fs::remove_dir_all(root).unwrap();
    }
    #[test]
    #[ignore = "installs a real DSH bundle in an isolated directory"]
    fn external_dsh_bundle_uses_shared_installer(){
        let root=std::env::temp_dir().join(format!("perch-p8-dsh-bundle-{}",uuid::Uuid::new_v4()));fs::create_dir_all(&root).unwrap();
        let plan=VersionPlan{engine:"DSH".into(),version:"bundle".into(),dependencies:serde_json::json!({"@deepseek-ai/dsh":"0.1.5-rc.3","@morlay/session-branch":"0.0.8"}),runtime:"24.18.0".into(),warnings:vec![]};
        let id=lock_plan(&root,plan).expect("resolve DSH bundle");
        let mut recipe=crate::catalog::packs().remove(0);recipe.profile_id=id.clone();recipe.extensions.push(crate::catalog::ExtensionRef{resource_rules:Default::default(),disabled_resources:Vec::new(),id:"dsh-npm:@morlay/session-branch".into(),version:"0.0.8".into(),enabled:true});recipe.validate().unwrap();
        let instance=crate::store::Instance{schema_version:1,id:uuid::Uuid::new_v4().to_string(),name:"P8 DSH bundle".into(),engine:"DSH".into(),profile_id:id.clone(),project_path:root.to_string_lossy().into(),connection_id:None,revision:1,created_at:0,updated_at:0};
        let engines=crate::engine::Engines::default();engines.begin(&instance,"install").unwrap();engines.execute(&root,&instance,None,"install").unwrap();
        let package=root.join("artifacts").join(&id).join("node_modules/@morlay/session-branch");let manifest:Value=serde_json::from_slice(&fs::read(package.join("package.json")).unwrap()).unwrap();
        assert_eq!(manifest["version"],"0.0.8");assert!(package.join(manifest["dsh"]["bundle"]["patch"].as_str().unwrap()).is_file());
        println!("P8 DSH bundle installed in {}",root.display());
    }
    #[test]
    #[ignore = "installs a real community package in a temporary instance"]
    fn external_pi_package_uses_shared_installer(){
        let root=std::env::temp_dir().join(format!("perch-p8-external-{}",uuid::Uuid::new_v4()));fs::create_dir_all(&root).unwrap();
        let plan=VersionPlan{engine:"Pi".into(),version:"resource".into(),dependencies:serde_json::json!({"@agegr/pi-web":"0.9.3","pi-web-access":"0.31.0"}),runtime:"24.18.0".into(),warnings:vec![]};
        let id=lock_plan(&root,plan).expect("resolve community package");
        let mut recipe=crate::catalog::packs().into_iter().find(|pack|pack.engine=="Pi").unwrap();recipe.profile_id=id.clone();recipe.extensions.push(crate::catalog::ExtensionRef{resource_rules:Default::default(),disabled_resources:Vec::new(),id:"npm:pi-web-access".into(),version:"0.31.0".into(),enabled:true});recipe.validate().unwrap();
        let instance=crate::store::Instance{schema_version:1,id:uuid::Uuid::new_v4().to_string(),name:"P8 external".into(),engine:"Pi".into(),profile_id:id.clone(),project_path:root.to_string_lossy().into(),connection_id:None,revision:1,created_at:0,updated_at:0};
        let engines=crate::engine::Engines::default();engines.begin(&instance,"install").unwrap();engines.execute(&root,&instance,None,"install").unwrap();
        let package=root.join("artifacts").join(&id).join("node_modules/pi-web-access");let manifest:Value=serde_json::from_slice(&fs::read(package.join("package.json")).unwrap()).unwrap();
        assert_eq!(manifest["version"],"0.31.0");assert!(manifest["pi"]["extensions"].is_array());assert!(package.join("dist").is_dir());
        println!("P8 external package installed in {}",root.display());
    }
    #[test]
    #[ignore = "resolves real npm dependencies in a UUID temporary directory"]
    fn nondefault_pi_lock_is_installable_by_shared_installer(){
        let root=std::env::temp_dir().join(format!("perch-p8-version-{}",uuid::Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        let plan=VersionPlan{engine:"Pi".into(),version:"0.9.2".into(),dependencies:serde_json::json!({"@agegr/pi-web":"0.9.2"}),runtime:"declared upstream".into(),warnings:vec![]};
        let id=lock_plan(&root,plan).expect("resolve non-default Pi Web");
        let profile=crate::engine_profile::resolve("Pi",&id).unwrap();
        assert!(profile.lock.contains("0.9.2"));
        let instance=crate::store::Instance{schema_version:1,id:uuid::Uuid::new_v4().to_string(),name:"P8 test".into(),engine:"Pi".into(),profile_id:id,project_path:root.to_string_lossy().into(),connection_id:None,revision:1,created_at:0,updated_at:0};
        let engines=crate::engine::Engines::default();
        engines.begin(&instance,"install").unwrap();
        engines.execute(&root,&instance,None,"install").unwrap();
        assert!(crate::engine::installed(&root,"Pi",&instance.profile_id));
        println!("P8 non-default installation verified in {}",root.display());
    }
}
