use serde::{Deserialize,Serialize};
use serde_json::Value;
use tauri::Manager;
use std::{fs,io::Read,time::Duration};
// Pi and DSH share one cache document, so refreshes must not overwrite each other.
static ACTIVE_CACHES:std::sync::LazyLock<std::sync::Mutex<std::collections::HashSet<std::path::PathBuf>>>=std::sync::LazyLock::new(||std::sync::Mutex::new(std::collections::HashSet::new()));
pub(crate) struct CacheRefresh(std::path::PathBuf);
impl CacheRefresh{
 pub(crate) fn begin(path:&std::path::Path)->Result<Self,String>{
  let mut active=ACTIVE_CACHES.lock().map_err(|_|"目录同步状态不可用")?;
  if !active.insert(path.to_path_buf()){return Err("此目录正在同步，请等待当前同步完成后再刷新".into());}
  Ok(Self(path.to_path_buf()))
 }
}
impl Drop for CacheRefresh{fn drop(&mut self){if let Ok(mut active)=ACTIVE_CACHES.lock(){active.remove(&self.0);}}}
pub(crate) fn save_cache(path:&std::path::Path,bytes:&[u8])->Result<(),String>{
 use std::io::Write;
 let parent=path.parent().ok_or("缓存路径无效")?;fs::create_dir_all(parent).map_err(|_|"无法创建缓存目录")?;
 let pending=parent.join(format!(".catalog-{}.pending",uuid::Uuid::new_v4()));
 let result=(||{
  let mut file=fs::OpenOptions::new().write(true).create_new(true).open(&pending).map_err(|_|"无法创建临时缓存")?;
  file.write_all(bytes).and_then(|_|file.sync_all()).map_err(|_|"缓存写入失败，旧缓存保留")?;drop(file);
  fs::rename(&pending,path).map_err(|_|"无法替换缓存，旧缓存保留".to_string())
 })();
 if result.is_err(){let _=fs::remove_file(&pending);}result
}
#[derive(Serialize,Deserialize)]
struct CachedResponse {url:String,#[serde(default)]accept:Option<String>,etag:Option<String>,last_modified:Option<String>,body:Value}
static SOURCE_COOLDOWNS:std::sync::LazyLock<std::sync::Mutex<std::collections::HashMap<String,std::time::Instant>>>=std::sync::LazyLock::new(Default::default);
fn source_retry_seconds(headers:&reqwest::header::HeaderMap,now:u64)->Option<u64>{
 let number=|name:&str|headers.get(name).and_then(|value|value.to_str().ok()).and_then(|value|value.parse::<u64>().ok());
 let retry=number("retry-after").or_else(||{
  let date=headers.get("retry-after")?.to_str().ok()?;
  let deadline=httpdate::parse_http_date(date).ok()?.duration_since(std::time::UNIX_EPOCH).ok()?.as_secs();
  Some(deadline.saturating_sub(now))
 });
 retry.or_else(||{
  (number("x-ratelimit-remaining")==Some(0)).then(||number("x-ratelimit-reset").map(|reset|reset.saturating_sub(now))).flatten()
 }).filter(|seconds|*seconds>0)
}
fn source_cooldown(origin:&str)->Result<(),String>{
 let mut cooldowns=SOURCE_COOLDOWNS.lock().map_err(|_|"来源等待状态不可用")?;
 if let Some(until)=cooldowns.get(origin){
  if let Some(remaining)=until.checked_duration_since(std::time::Instant::now()){
   return Err(format!("来源请求需等待，请在 {} 秒后重试；本次未再次请求服务器",remaining.as_secs().saturating_add(1)));
  }
  cooldowns.remove(origin);
 }
 Ok(())
}
pub(crate) fn conditional_json(root:&std::path::Path,client:&reqwest::blocking::Client,address:&str)->Result<Value,String>{
 conditional_json_with(root,client,address,None,8*1024*1024)
}
pub(crate) fn conditional_json_with(root:&std::path::Path,client:&reqwest::blocking::Client,address:&str,accept:Option<&str>,limit:u64)->Result<Value,String>{
 conditional_body(root,client,address,accept,limit,false)
}
pub(crate) fn conditional_text(root:&std::path::Path,client:&reqwest::blocking::Client,address:&str)->Result<String,String>{
 conditional_body(root,client,address,Some("text/html"),2*1024*1024,true)?.as_str().map(str::to_string).ok_or("来源页面缓存格式无效".into())
}
fn conditional_body(root:&std::path::Path,client:&reqwest::blocking::Client,address:&str,accept:Option<&str>,limit:u64,plain:bool)->Result<Value,String>{
 let origin=reqwest::Url::parse(address).map_err(|_|"来源地址无效")?.origin().ascii_serialization();
 source_cooldown(&origin)?;
 let identity=accept.map_or_else(||address.to_string(),|accept|format!("{address}\n{accept}"));
 let path=root.join("source-http-cache").join(format!("{}.json",manifest_cache_key(&identity)));
 let _guard=CacheRefresh::begin(&path)?;
 let cached=fs::File::open(&path).ok().and_then(|file|{let mut bytes=Vec::new();file.take(limit*2+4097).read_to_end(&mut bytes).ok()?;if bytes.len() as u64>limit*2+4096{return None;}serde_json::from_slice::<CachedResponse>(&bytes).ok()}).filter(|value|value.url==address&&value.accept.as_deref()==accept);
 let mut request=client.get(address);
 if let Some(accept)=accept{request=request.header(reqwest::header::ACCEPT,accept);}
 if let Some(value)=&cached{
  if let Some(etag)=&value.etag{request=request.header(reqwest::header::IF_NONE_MATCH,etag);}
  else if let Some(date)=&value.last_modified{request=request.header(reqwest::header::IF_MODIFIED_SINCE,date);}
 }
 let response=request.send().map_err(|_|"无法访问来源，请检查网络后重试")?;
 if response.status()==reqwest::StatusCode::NOT_MODIFIED{return cached.map(|value|value.body).ok_or("来源返回 304，但本机没有对应缓存，请重新刷新".into());}
 if !response.status().is_success(){
  let now=std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_secs();
  let retry=source_retry_seconds(response.headers(),now).or_else(||(response.status()==reqwest::StatusCode::TOO_MANY_REQUESTS).then_some(60));
  if let Some(until)=retry.and_then(|seconds|std::time::Instant::now().checked_add(Duration::from_secs(seconds))){
   if let Ok(mut cooldowns)=SOURCE_COOLDOWNS.lock(){cooldowns.insert(origin,until);}
  }
  return Err(match retry{Some(seconds)=>format!("来源请求受到限流（HTTP {}），请在 {seconds} 秒后重试",response.status().as_u16()),None if response.status()==reqwest::StatusCode::FORBIDDEN=>"来源拒绝访问（HTTP 403）；可能是访问策略或限流，请稍后重试或打开来源页面".into(),None=>format!("HTTP {}",response.status().as_u16())});
 }
 let header=|name:reqwest::header::HeaderName|response.headers().get(name).and_then(|v|v.to_str().ok()).filter(|v|v.len()<=1024).map(str::to_string);
 let etag=header(reqwest::header::ETAG);let last_modified=header(reqwest::header::LAST_MODIFIED);
 let mut bytes=Vec::new();response.take(limit+1).read_to_end(&mut bytes).map_err(|_|"来源内容读取失败")?;
 if bytes.len() as u64>limit{return Err(format!("来源内容超过 {} MB 读取上限",limit/1024/1024));}
 let body=if plain{Value::String(String::from_utf8(bytes).map_err(|_|"来源页面编码无效")?)}else{serde_json::from_slice(&bytes).map_err(|_|"来源 JSON 格式无效，旧缓存保留")?};
 let stored=CachedResponse{url:address.into(),accept:accept.map(str::to_string),etag,last_modified,body};
 save_cache(&path,&serde_json::to_vec(&stored).map_err(|_|"来源缓存编码失败")?)?;
 Ok(stored.body)
}
#[derive(Serialize,Deserialize,Clone)]
#[serde(rename_all="camelCase",default,deny_unknown_fields)]
pub struct SourcePreferences {pub npm_pi:bool,pub npm_dsh:bool,pub workshop:bool,pub dsh_topic:bool,pub pi_official:bool}
impl Default for SourcePreferences{fn default()->Self{Self{npm_pi:true,npm_dsh:true,workshop:true,dsh_topic:true,pi_official:true}}}
pub(crate) fn preferences(root:&std::path::Path)->Result<SourcePreferences,String>{
 match fs::read(root.join("resource-sources.json")){Ok(bytes)=>serde_json::from_slice(&bytes).map_err(|_|"来源设置无法读取，原文件保留".into()),Err(error) if error.kind()==std::io::ErrorKind::NotFound=>Ok(SourcePreferences::default()),Err(_)=>Err("来源设置不可读".into())}
}
#[tauri::command]
pub fn resource_source_preferences(app:tauri::AppHandle)->Result<SourcePreferences,String>{let state=app.state::<crate::data_commands::DataState>();preferences(state.root.as_ref().map_err(|_|"工作空间不可用")?)}
#[tauri::command]
pub fn set_resource_source_enabled(app:tauri::AppHandle,source:String,enabled:bool)->Result<SourcePreferences,String>{
 let state=app.state::<crate::data_commands::DataState>();let root=state.root.as_ref().map_err(|_|"工作空间不可用")?;let path=root.join("resource-sources.json");let _guard=CacheRefresh::begin(&path)?;let mut settings=preferences(root)?;
 match source.as_str(){"npmPi"=>settings.npm_pi=enabled,"npmDsh"=>settings.npm_dsh=enabled,"workshop"=>settings.workshop=enabled,"dshTopic"=>settings.dsh_topic=enabled,"piOfficial"=>settings.pi_official=enabled,_=>return Err("未知资源来源".into())}
 save_cache(&path,&serde_json::to_vec(&settings).map_err(|_|"来源设置编码失败")?)?;Ok(settings)
}
#[derive(Serialize,Deserialize,Clone)]
#[serde(rename_all="camelCase")]
pub struct Resource {
 pub id:String,pub name:String,pub description:String,pub version:String,pub engine:String,
 pub source:String,pub homepage:String,pub kinds:Vec<String>,
 #[serde(default)]pub package_name:Option<String>,#[serde(default)]pub requirements:Value,
}
#[derive(Serialize,Deserialize,Default)]
#[serde(rename_all="camelCase")]
pub struct ResourceCatalog {#[serde(default)]pub bundled:Option<String>,pub entries:Vec<Resource>,pub errors:Vec<String>,pub synced_at:u64,
 #[serde(default)]pub source_times:std::collections::BTreeMap<String,u64>}
fn apply_source_result(catalog:&mut ResourceCatalog,engine:&str,result:Result<Vec<Resource>,String>,now:u64){
 match result{Ok(entries)=>{catalog.bundled=None;catalog.entries.retain(|item|item.engine!=engine);catalog.entries.extend(entries);catalog.source_times.insert(engine.into(),now);},Err(error)=>catalog.errors.push(format!("{engine}：{error}，保留上次目录"))}
}
#[tauri::command]
pub async fn resource_catalog(app:tauri::AppHandle,refresh:bool,source:Option<String>)->Result<ResourceCatalog,String>{
 let selected=match source.as_deref(){None=>None,Some("npmPi")=>Some("Pi"),Some("npmDsh")=>Some("DSH"),_=>return Err("未知 npm 目录来源".into())};
 let root=app.state::<crate::data_commands::DataState>().root.as_ref().map_err(|_|"工作空间不可用")?.clone();
 tauri::async_runtime::spawn_blocking(move||{
  let path=root.join("resource-catalog.json");
  if !refresh{crate::catalog_seed::ensure(&path)?;}
  if !refresh{if let Ok(bytes)=fs::read(&path){if let Ok(cached)=serde_json::from_slice::<ResourceCatalog>(&bytes){return Ok(cached);}}}
  let _guard=CacheRefresh::begin(&path)?;
  let mut cached:ResourceCatalog=fs::read(&path).ok().and_then(|bytes|serde_json::from_slice(&bytes).ok()).unwrap_or_default();
  if !refresh && (cached.synced_at>0||!cached.entries.is_empty()||!cached.errors.is_empty()){return Ok(cached);}
  let client=reqwest::blocking::Client::builder().user_agent("Perch-resource-catalog").timeout(Duration::from_secs(25)).redirect(reqwest::redirect::Policy::none()).build().map_err(|_|"目录网络初始化失败")?;
  let settings=preferences(&root)?;
  if let Some(engine)=selected{cached.errors.retain(|message|!message.starts_with(&format!("{engine}："))&&!message.starts_with(&format!("{engine} 发现目录")));}else{cached.errors.clear();}
  let now=std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_secs();
  for (engine,keyword) in [("Pi","pi-package"),("DSH","dsh-plugin")] {
   if selected.is_some_and(|selected|selected!=engine){continue;}
   if !(if engine=="Pi"{settings.npm_pi}else{settings.npm_dsh}){continue;}
   let mut page_error=None;
   let result=npm_pages(engine,|offset|{
    if offset>0{std::thread::sleep(Duration::from_millis(600));}
    let result=conditional_json(&root,&client,&format!("https://registry.npmjs.org/-/v1/search?text=keywords%3A{keyword}&size=250&from={offset}"));
    if let Err(error)=&result{page_error=Some(error.clone());}result
   });
   let result=result.map(|(mut entries,incomplete)|{if incomplete{
    cached.errors.push(format!("{engine} 发现目录未同步完整：{}；已取得的条目可浏览，旧条目保留",page_error.as_deref().unwrap_or("达到 5000 项读取上限或分页内容发生变化")));
    let ids:std::collections::HashSet<_>=entries.iter().map(|item|item.id.clone()).collect();
    entries.extend(cached.entries.iter().filter(|item|item.engine==engine&&!ids.contains(&item.id)).cloned());
   }entries});
   apply_source_result(&mut cached,engine,result,now);
  }
  cached.synced_at=cached.source_times.values().copied().min().unwrap_or(0);
  save_cache(&path,&serde_json::to_vec(&cached).map_err(|_|"目录编码失败")?)?;
  Ok(cached)
 }).await.map_err(|_|"资源目录任务中断".to_string())?
}

#[derive(Serialize,Deserialize)]
#[serde(rename_all="camelCase")]
pub struct ResourceManifest {
    name:String,version:String,kinds:Vec<String>,declarations:Value,
    dependencies:Value,peer_dependencies:Value,scripts:Vec<String>,integrity:String,artifact:String,
    package_info:Value,
}
fn manifest_cache_key(name:&str)->String{use sha2::{Digest,Sha256};format!("{:x}",Sha256::digest(name.as_bytes()))}
// Deliberately reject unsupported syntax rather than silently treating it as compatible.
fn require_dsh_version(requirement:&Value,version:&str)->Result<(),String>{
 if requirement.is_null(){return Ok(());}
 let text=requirement.as_str().ok_or("DSH 版本要求不是支持的文本范围，无法确认兼容")?.trim();
 if text.is_empty(){return Err("DSH 版本要求为空，无法确认兼容".into());}
 let target=semver::Version::parse(version).map_err(|_|"目标 DSH 版本不是精确版本")?;
 let mut matches=false;
 for range in text.split("||") {
  let range=range.trim();
  let normalized=if semver::Version::parse(range).is_ok()||(!range.is_empty()&&range.bytes().all(|c|c.is_ascii_digit()||c==b'.')){format!("={range}")}else{range.split_whitespace().collect::<Vec<_>>().join(",").replace(",,",",")};
  let parsed=semver::VersionReq::parse(&normalized).map_err(|_|format!("暂不支持 DSH 版本范围 {text}，请选用声明明确兼容的资源"))?;
  matches|=parsed.matches(&target);
 }
 if matches{Ok(())}else{Err(format!("资源要求 DSH {text}，目标版本为 {version}；请选择兼容的资源或引擎版本"))}
}
fn require_plan_dsh(plan:&Value,version:&str)->Result<(),String>{
 let requires=&plan["requires"];
 let requirement=if requires.is_string(){requires}else if requires.as_object().is_some_and(|value|value.is_empty())||requires.is_null(){&Value::Null}else if requires.is_object(){requires.get("dsh").or_else(||requires.get("@deepseek-ai/dsh")).or_else(||requires.get("engines").and_then(|v|v.get("dsh"))).ok_or("工坊 requires 未包含可识别的 DSH 版本声明，无法确认兼容")?}else{return Err("工坊 requires 声明格式不受支持，无法确认兼容".into());};
 require_dsh_version(requirement,version)
}
pub(crate) fn validate_dsh_artifacts(directory:&std::path::Path,profile:&crate::engine_profile::ResolvedProfile)->Result<(),String>{
 if profile.engine!="DSH"{return Ok(());}
 let package:Value=serde_json::from_str(&profile.package).map_err(|_|"版本组合无效")?;
 let version=package["dependencies"]["@deepseek-ai/dsh"].as_str().ok_or("缺少 DSH 精确版本")?;
 for name in package["dependencies"].as_object().ok_or("缺少依赖")?.keys(){
  if name=="@deepseek-ai/dsh"{continue;}
  let doc:Value=serde_json::from_slice(&fs::read(directory.join("node_modules").join(name).join("package.json")).map_err(|_|format!("无法读取 {name} 的兼容声明"))?).map_err(|_|"包清单无效")?;
  require_dsh_version(&doc["dsh"]["engines"]["dsh"],version)?;
  require_dsh_version(&doc["peerDependencies"]["@deepseek-ai/dsh"],version)?;
 }
 Ok(())
}
fn platform_allows(value:&Value,current:&str)->Result<bool,String>{
 if value.is_null(){return Ok(true);}
 let entries=if let Some(single)=value.as_str(){vec![single]}else{value.as_array().ok_or("平台声明必须是字符串或列表")?.iter().map(|value|value.as_str().ok_or("平台声明包含非文本项")).collect::<Result<Vec<_>,_>>()?};
 if entries==["any"]{return Ok(true);}
 if entries.iter().any(|entry|entry.strip_prefix('!')==Some(current)){return Ok(false);}
 Ok(entries.iter().all(|entry|entry.starts_with('!'))||entries.contains(&current))
}
fn validate_resource_platform(manifest:&ResourceManifest)->Result<(),String>{
 for (field,current) in [("os","win32"),("cpu","x64")] {
  if !platform_allows(&manifest.package_info[field],current)?{return Err(format!("{} {} 的 {field} 声明不支持当前固定运行时 {current}；请选择兼容版本",manifest.name,manifest.version));}
 }
 Ok(())
}
pub(crate) fn expected_manifest(doc:Value,name:&str,version:&str)->Result<ResourceManifest,String>{
 if doc["name"]!=name||(version!="latest"&&doc["version"]!=version){return Err("清单的包名或版本与请求不一致，未使用该缓存或响应".into());}
 parse_manifest(doc)
}
fn parse_manifest(doc:Value)->Result<ResourceManifest,String>{
    let name=doc["name"].as_str().ok_or("包缺少名称")?.to_string();
    let version=doc["version"].as_str().ok_or("包缺少版本")?.to_string();
    let mut kinds=Vec::new();let mut declarations=serde_json::Map::new();
    for kind in ["extensions","skills","prompts","themes"] {
        if let Some(value)=doc["pi"].get(kind){kinds.push(kind.to_string());declarations.insert(kind.into(),value.clone());}
    }
    if let Some(bundle)=doc["dsh"].get("bundle"){kinds.push("dsh-bundle".into());declarations.insert("dsh-bundle".into(),bundle.clone());}
    Ok(ResourceManifest{name,version,kinds,declarations:Value::Object(declarations),dependencies:doc["dependencies"].clone(),peer_dependencies:doc["peerDependencies"].clone(),scripts:doc["scripts"].as_object().map(|items|items.keys().cloned().collect()).unwrap_or_default(),integrity:doc["dist"]["integrity"].as_str().unwrap_or("").into(),artifact:doc["dist"]["tarball"].as_str().unwrap_or("").into(),package_info:serde_json::json!({"license":doc["license"],"engines":doc["engines"],"dshEngines":doc["dsh"]["engines"],"os":doc["os"],"cpu":doc["cpu"],"optionalDependencies":doc["optionalDependencies"],"peerDependenciesMeta":doc["peerDependenciesMeta"]})})
}
#[tauri::command]
pub async fn inspect_resource(app:tauri::AppHandle,name:String,version:String)->Result<ResourceManifest,String>{
    if name.is_empty()||name.len()>214||!name.bytes().all(|c|c.is_ascii_alphanumeric()||b"@/._-".contains(&c))||name.contains("..")||version.is_empty()||!version.bytes().all(|c|c.is_ascii_alphanumeric()||b".-+".contains(&c)){return Err("请输入 npm 包名和精确版本".into());}
    let root=app.state::<crate::data_commands::DataState>().root.as_ref().map_err(|_|"工作空间不可用")?.clone();
    tauri::async_runtime::spawn_blocking(move||{
        let cache=root.join("resource-manifests-v2").join(manifest_cache_key(&name)).join(format!("{version}.json"));
        let _guard=CacheRefresh::begin(&cache)?;
        if version!="latest"{
            let legacy=root.join("resource-manifests").join(name.replace('/',"__").replace('@',"_")).join(format!("{version}.json"));
            for path in [&cache,&legacy]{if let Ok(bytes)=fs::read(path){if let Ok(doc)=serde_json::from_slice(&bytes){if let Ok(manifest)=expected_manifest(doc,&name,&version){return Ok(manifest);}}}}
        }
        let client=reqwest::blocking::Client::builder().timeout(Duration::from_secs(25)).redirect(reqwest::redirect::Policy::none()).build().map_err(|_|"请求初始化失败")?;
        let response=client.get(format!("https://registry.npmjs.org/{}/{}",name.replace('/',"%2F"),version)).send().map_err(|_|"无法读取包清单，检查网络后重试")?;
        if !response.status().is_success(){return Err(format!("包清单请求失败 HTTP {}",response.status().as_u16()));}
        let mut bytes=Vec::new();response.take(2*1024*1024+1).read_to_end(&mut bytes).map_err(|_|"包清单读取失败")?;
        if bytes.len()>2*1024*1024{return Err("包清单过大".into());}
        let doc:Value=serde_json::from_slice(&bytes).map_err(|_|"包清单格式无效")?;
        let parsed=expected_manifest(doc,&name,&version)?;
        if parsed.version.is_empty()||!parsed.version.as_bytes()[0].is_ascii_digit()||!parsed.version.bytes().all(|c|c.is_ascii_alphanumeric()||b".-+".contains(&c)){return Err("来源未返回有效精确版本".into());}
        fs::create_dir_all(cache.parent().unwrap()).map_err(|_|"无法创建清单缓存")?;
        let cache=cache.with_file_name(format!("{}.json",parsed.version));
        save_cache(&cache,&bytes)?;
        Ok(parsed)
    }).await.map_err(|_|"包清单任务中断".to_string())?
}

#[tauri::command]
pub async fn resolve_resource_pack(app:tauri::AppHandle,name:String,version:String,base_recipe:Option<crate::catalog::Recipe>)->Result<crate::catalog::Recipe,String>{
 let manifest=inspect_resource(app.clone(),name.clone(),version.clone()).await?;
 validate_resource_platform(&manifest)?;
 let version=manifest.version.clone();
 let engine=if manifest.kinds.iter().any(|kind|kind=="dsh-bundle"){"DSH"}else{"Pi"};
 if !crate::engine_profile::valid_package_name(&name)||manifest.kinds.is_empty(){return Err("包未声明 Pi 资源或 DSH bundle，无法装配".into());}
 if engine=="DSH"&&manifest.kinds.len()!=1{return Err("包同时声明两种生态，请选择明确适配单一宿主的版本".into());}
 if manifest.dependencies.as_object().is_some_and(|deps|deps.keys().any(|key|key.starts_with("@earendil-works/pi-")||key.starts_with("@mariozechner/pi-"))){return Err("包把 Pi 宿主列入运行依赖，可能产生重复宿主；请使用声明 peerDependencies 的版本".into());}
 let root=app.state::<crate::data_commands::DataState>().root.as_ref().map_err(|_|"工作空间不可用")?.clone();
 tauri::async_runtime::spawn_blocking(move||{
  let new_instance=base_recipe.is_none();
  let mut recipe=base_recipe.unwrap_or(crate::catalog::packs().into_iter().find(|pack|pack.engine==engine).ok_or("缺少基础组合")?);
  recipe.validate().map_err(|e|e.message)?;
  if recipe.engine!=engine{return Err("所选实例与资源的引擎不兼容".into());}
  let base_profile=crate::engine_profile::resolve(engine,&recipe.profile_id).map_err(|e|e.message)?;
  let base:Value=serde_json::from_str(&base_profile.package).map_err(|_|"基础依赖无效")?;
  if engine=="DSH" {let target=base["dependencies"]["@deepseek-ai/dsh"].as_str().ok_or("缺少 DSH 精确版本")?;require_dsh_version(&manifest.package_info["dshEngines"]["dsh"],target)?;require_dsh_version(&manifest.peer_dependencies["@deepseek-ai/dsh"],target)?;}

  let mut dependencies=base["dependencies"].as_object().ok_or("基础依赖缺失")?.clone();
  let resource_id=format!("{}{name}",if engine=="DSH"{"dsh-npm:"}else{"npm:"});
  if dependencies.contains_key(&name)&&!recipe.extensions.iter().any(|entry|entry.id==resource_id){return Err("引擎本身不能作为扩展添加".into());}
  dependencies.insert(name.clone(),Value::String(version.clone()));
  let plan=crate::version_catalog::VersionPlan{engine:engine.into(),version:"resource-combination".into(),dependencies:Value::Object(dependencies),runtime:crate::engine_profile::PI.node.into(),warnings:vec![]};
  recipe.profile_id=crate::version_catalog::lock_plan(&root,plan)?;
  if new_instance { recipe.name=format!("{engine} · {}",name).chars().take(80).collect(); }
  update_resource(&mut recipe.extensions,resource_id,version);
  recipe.validate().map_err(|e|e.message)?;
  Ok(recipe)
 }).await.map_err(|_|"资源组合解析中断".to_string())?
}

fn update_resource(entries:&mut Vec<crate::catalog::ExtensionRef>,id:String,version:String){
 if let Some(entry)=entries.iter_mut().find(|entry|entry.id==id){
  // Updating a package must not reorder DSH patches or reset Pi selections.
  entry.version=version;
 }else{
  entries.push(crate::catalog::ExtensionRef{resource_rules:Default::default(),disabled_resources:Vec::new(),id,version,enabled:true});
 }
}
fn workshop_input<'a>(lock:&'a Value,name:&str)->Result<(&'a str,&'a str),String>{
 if lock["version"].as_u64()!=Some(2){return Err("工坊来源锁格式已变化，暂不能解析".into());}
 let input=&lock["inputs"][name];
 let submodule=input["submodule"].as_str().ok_or("工坊来源缺少子模块映射")?;
 let content=input["path"].as_str().ok_or("工坊来源缺少内容目录")?;
 let safe=|path:&str|!path.is_empty()&&path.split('/').all(|part|!part.is_empty()&&part!="."&&part!=".."&&part.bytes().all(|c|c.is_ascii_alphanumeric()||b"._-".contains(&c)));
 if !safe(submodule)||(content!="."&&!safe(content)){return Err("工坊来源包含不支持的目录映射".into());}
 Ok((submodule,content))
}
fn npm_pages(engine:&str,mut fetch:impl FnMut(usize)->Result<Value,String>)->Result<(Vec<Resource>,bool),String>{
 let mut entries=Vec::new();let mut seen=std::collections::HashSet::new();let mut offset=0usize;let mut overlapping=false;
 for _ in 0..20 {
  let doc=match fetch(offset){Ok(doc)=>doc,Err(_) if !entries.is_empty()=>return Ok((entries,true)),Err(error)=>return Err(error)};
  let rows=doc["objects"].as_array().ok_or("目录缺少资源列表")?;
  let total=doc["total"].as_u64().ok_or("目录缺少总数，无法确认分页是否完整")?;
  if rows.len()>250{return Err("目录返回条目数超过分页约定".into());}
  if rows.is_empty()&&(offset as u64)<total{return Err("目录分页提前结束，未替换已有缓存".into());}
  for row in rows {
   let p=&row["package"];let name=p["name"].as_str().filter(|name|crate::engine_profile::valid_package_name(name)).ok_or("目录资源名称无效")?;
   let version=p["version"].as_str().ok_or("目录资源缺少版本")?;
   if !seen.insert(name.to_string()){overlapping=true;continue;}
   entries.push(Resource{id:format!("{}{name}",if engine=="DSH"{"dsh-npm:"}else{"npm:"}),name:name.into(),description:p["description"].as_str().unwrap_or("").into(),version:version.into(),engine:engine.into(),source:"npm".into(),homepage:p["links"]["repository"].as_str().or(p["links"]["npm"].as_str()).unwrap_or("").into(),kinds:vec!["待读取包清单".into()],package_name:Some(name.into()),requirements:serde_json::json!({"publishedAt":p["date"],"publisher":p["publisher"]["username"]})});
  }
  offset+=rows.len();
  if offset as u64>=total{return Ok((entries,overlapping));}
 }
 Ok((entries,true))
}
fn workshop_pin(module:&Value,content:&str)->Result<Value,String>{
 let sha=module["sha"].as_str().filter(|sha|sha.len()==40&&sha.bytes().all(|c|c.is_ascii_hexdigit())).ok_or("工坊子模块缺少固定提交")?;
 let address=module["submodule_git_url"].as_str().ok_or("工坊输入不再是子模块")?;
 let repo=address.strip_prefix("https://github.com/").ok_or("工坊子模块暂只支持 GitHub HTTPS 来源")?.trim_end_matches(".git");
 let parts=repo.split('/').collect::<Vec<_>>();
 if parts.len()!=2||parts.iter().any(|part|part.is_empty()||*part=="."||*part==".."||!part.bytes().all(|c|c.is_ascii_alphanumeric()||b"._-".contains(&c))){return Err("工坊子模块仓库地址无效".into());}
 Ok(serde_json::json!({"repository":format!("https://github.com/{repo}"),"commit":sha,"contentPath":content,"browseUrl":format!("https://github.com/{repo}/tree/{sha}/{}",if content=="."{""}else{content})}))
}
fn workshop(root:&std::path::Path,refresh:bool)->Result<ResourceCatalog,String>{
 let path=root.join("workshop-catalog.json");
 if !refresh{crate::catalog_seed::ensure(&path)?;}
 if !refresh{if let Ok(bytes)=fs::read(&path){if let Ok(cached)=serde_json::from_slice::<ResourceCatalog>(&bytes){return Ok(cached);}}}
 let _guard=CacheRefresh::begin(&path)?;
 let cached:Option<ResourceCatalog>=fs::read(&path).ok().and_then(|bytes|serde_json::from_slice(&bytes).ok());
 if !preferences(root)?.workshop{return Ok(cached.unwrap_or_default());}
 if !refresh{if let Some(cached)=cached{return Ok(cached);}}
 let result=(||{
  let client=reqwest::blocking::Client::builder().user_agent("Perch-resource-catalog").timeout(Duration::from_secs(25)).redirect(reqwest::redirect::Policy::none()).build().map_err(|_|"工坊请求初始化失败")?;
  let get=|url:String|conditional_json(root,&client,&url);
  let head=get("https://api.github.com/repos/zhu1090093659/dsh-web/commits/dev".into())?;
  let commit=head["sha"].as_str().filter(|sha|sha.len()==40&&sha.bytes().all(|c|c.is_ascii_hexdigit())).ok_or("工坊来源缺少提交标识")?;
  let inputs=get(format!("https://raw.githubusercontent.com/zhu1090093659/dsh-web/{commit}/market-inputs.lock.json"))?;
  let mut catalog=ResourceCatalog::default();
  // Generated manifests are the public catalog contract. Content provenance is
  // resolved from the same commit's lock mapping and GitHub submodule gitlink.
  for (kind,input) in [("plugins","community"),("skins","skins"),("pets","pet"),("presets","presets")]{
   let (submodule,content)=workshop_input(&inputs,input)?;
   let module=get(format!("https://api.github.com/repos/zhu1090093659/dsh-web/contents/{submodule}?ref={commit}"))?;
   let pin=workshop_pin(&module,content)?;
   let doc=get(format!("https://raw.githubusercontent.com/zhu1090093659/dsh-web/{commit}/market/dist/manifest/{kind}.json"))?;
   let items=doc["items"].as_array().ok_or("工坊清单缺少 items")?;
   for item in items{
    let id=item["id"].as_str().ok_or("工坊资源缺少 ID")?;
    let package=item["npm"].as_str().filter(|name|crate::engine_profile::valid_package_name(name));
    catalog.entries.push(Resource{id:format!("workshop:{kind}:{id}"),name:item["name"].as_str().or(item["displayName"].as_str()).unwrap_or(id).into(),description:item["description"].as_str().unwrap_or("").into(),version:item["version"].as_str().unwrap_or("待解析").into(),engine:"DSH".into(),source:format!("DSH 工坊 · {commit}"),homepage:item["repo"].as_str().or(pin["browseUrl"].as_str()).unwrap_or("https://dsh-market.com/").into(),kinds:vec![kind.into()],package_name:package.map(str::to_string),requirements:serde_json::json!({"requires":item["requires"],"facets":item["facets"],"contributes":item["contributes"],"files":item["files"],"category":item["category"],"author":item["author"],"contentSource":pin,"manifestSource":format!("https://raw.githubusercontent.com/zhu1090093659/dsh-web/{commit}/market/dist/manifest/{kind}.json")})});
   }
  }
  catalog.synced_at=std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_secs();save_cache(&path,&serde_json::to_vec(&catalog).map_err(|_|"工坊缓存编码失败")?)?;Ok(catalog)
 })();
 match result{Ok(catalog)=>Ok(catalog),Err(error)=>if let Some(mut cached)=cached{cached.errors=vec![format!("{error}；保留上次工坊目录")];save_cache(&path,&serde_json::to_vec(&cached).map_err(|_|"工坊缓存编码失败")?)?;Ok(cached)}else{Err(error)}}
}
#[tauri::command]
pub async fn workshop_catalog(app:tauri::AppHandle,refresh:bool)->Result<ResourceCatalog,String>{
 let root=app.state::<crate::data_commands::DataState>().root.as_ref().map_err(|_|"工作空间不可用")?.clone();
 tauri::async_runtime::spawn_blocking(move||workshop(&root,refresh)).await.map_err(|_|"工坊目录同步中断".to_string())?
}

fn workshop_asset_plan(item:&Resource)->Result<Value,String>{
 let parts=item.id.split(':').collect::<Vec<_>>();
 if parts.len()!=3||parts[0]!="workshop"{return Err("不是工坊静态资源".into());}
 let (directory,required)=match parts[1]{"skins"=>("skins","skin.json"),"pets"=>("pets","pet.json"),"presets"=>("agent-presets","preset.yml"),_=>return Err("此类型通过 npm 插件安装流程处理".into())};
 let id=parts[2];
 if !crate::pack_archive::safe_name(id)||id.contains('/') {return Err("资源标识不是安全目录名".into());}
 let pin=&item.requirements["contentSource"];
 let repo=pin["repository"].as_str().and_then(|s|s.strip_prefix("https://github.com/")).ok_or("资源缺少 GitHub 固定来源")?;
 let segments=repo.split('/').collect::<Vec<_>>();
 if segments.len()!=2||segments.iter().any(|s|s.is_empty()||*s=="."||*s==".."||!s.bytes().all(|b|b.is_ascii_alphanumeric()||b"._-".contains(&b))){return Err("资源仓库地址无效".into());}
 let commit=pin["commit"].as_str().filter(|s|s.len()==40&&s.bytes().all(|b|b.is_ascii_hexdigit())).ok_or("资源未固定到提交")?;
 let content=pin["contentPath"].as_str().ok_or("资源缺少仓库子目录")?;
 if content!="."&&!crate::pack_archive::safe_name(content){return Err("仓库子目录无效".into());}
 let files=item.requirements["files"].as_array().ok_or("缓存缺少资源文件清单，请刷新 DSH 工坊目录后重新预览；已有实例不受影响")?;
 if files.is_empty()||files.len()>2000{return Err("资源文件清单为空或超过 2000 项，当前无法安装；可查看来源确认资源内容".into());}
 let mut seen=std::collections::HashSet::new();let mut downloads=Vec::new();
 for file in files{
  let relative=file.as_str().filter(|s|crate::pack_archive::safe_name(s)).ok_or("资源文件包含越界或不支持的路径")?;
  if relative.eq_ignore_ascii_case(".perch-resource.json"){return Err("资源文件占用栖点来源记录名称".into());}
  if !seen.insert(relative.to_lowercase()){return Err("资源文件路径重复".into());}
  let path=if content=="."{format!("{id}/{relative}")}else{format!("{content}/{id}/{relative}")};
  let mut url=url::Url::parse(&format!("https://raw.githubusercontent.com/{repo}/{commit}/")).map_err(|_|"资源下载地址无效")?;
  url.path_segments_mut().map_err(|_|"资源下载地址无效")?.pop_if_empty().extend(path.split('/'));
  downloads.push(serde_json::json!({"path":relative,"url":url.to_string()}));
 }
 if !seen.contains(required){return Err(format!("资源缺少 {required}"));}
 Ok(serde_json::json!({"id":item.id,"name":item.name,"version":item.version,"directory":format!("{directory}/{id}"),"commit":commit,"repository":repo,"contentPath":content,"files":downloads,"requires":item.requirements["requires"],"facets":item.requirements["facets"],"activation":if parts[1]=="presets"{"下载到预设库后仍需启用"}else{"需要对应的 DSH 资源宿主插件并在工作台中选择"}}))
}
#[tauri::command]
pub fn preview_workshop_asset(app:tauri::AppHandle,resource_id:String)->Result<Value,String>{
 let state=app.state::<crate::data_commands::DataState>();let root=state.root.as_ref().map_err(|_|"工作空间不可用")?;
 let catalog:ResourceCatalog=serde_json::from_slice(&fs::read(root.join("workshop-catalog.json")).map_err(|_|"请先同步工坊目录")?).map_err(|_|"工坊目录无效，请刷新")?;
 let mut plan=workshop_asset_plan(catalog.entries.iter().find(|item|item.id==resource_id).ok_or("资源已不在缓存目录中，请刷新")?)?;
 plan["hostPackage"]=serde_json::json!(workshop_host(plan["directory"].as_str())?.0);Ok(plan)
}

fn workshop_content_identity(plan:&Value)->Value {
 let mut files=plan["files"].as_array().cloned().unwrap_or_default();
 files.sort_by_key(|file|file["path"].as_str().unwrap_or("").to_owned());
 serde_json::json!({"id":plan["id"],"version":plan["version"],"repository":plan["repository"],"commit":plan["commit"],"contentPath":plan["contentPath"],"directory":plan["directory"],"files":files})
}
fn workshop_content_key(plan:&Value)->String {manifest_cache_key(&workshop_content_identity(plan).to_string())}
fn existing_workshop_cache(root:&std::path::Path,plan:&Value)->Result<Option<(Value,std::path::PathBuf)>,String>{
 let parent=root.join("workshop-assets");let key=workshop_content_key(plan);let direct=parent.join(&key);
 if direct.exists(){return Ok(Some((verify_workshop_cache(&direct,plan,&key)?,direct)));}
 // Keep old receipts and directories intact; reuse only matching, digest-verified content.
 if let Ok(entries)=fs::read_dir(&parent){for entry in entries.flatten(){
  let name=entry.file_name().to_string_lossy().to_string();if name.len()!=64||!name.bytes().all(|b|b.is_ascii_hexdigit()){continue;}
  let receipt_path=entry.path().join("receipt.json");if fs::metadata(&receipt_path).map(|m|m.len()>2*1024*1024).unwrap_or(true){continue;}
  let Ok(bytes)=fs::read(receipt_path) else{continue;};let Ok(receipt)=serde_json::from_slice::<Value>(&bytes) else{continue;};
  if workshop_content_identity(&receipt["plan"])==workshop_content_identity(plan){return Ok(Some((verify_workshop_cache(&entry.path(),plan,&name)?,entry.path())));}
 }}Ok(None)
}
fn verify_workshop_cache(directory:&std::path::Path,plan:&Value,key:&str)->Result<Value,String>{
 use sha2::{Digest,Sha256};
 let regular=|path:&std::path::Path|->Result<std::fs::Metadata,String>{
  let metadata=fs::symlink_metadata(path).map_err(|_|"资源缓存缺少文件，请修复后重试")?;
  #[cfg(windows)]{use std::os::windows::fs::MetadataExt;if metadata.file_attributes()&0x400!=0{return Err("资源缓存含链接，不能复用".into());}}
  if metadata.file_type().is_symlink(){return Err("资源缓存含链接，不能复用".into());}Ok(metadata)
 };
 regular(directory)?;
 let receipt_path=directory.join("receipt.json");
 if regular(&receipt_path)?.len()>2*1024*1024{return Err("资源缓存收据过大".into());}
 let mut receipt:Value=serde_json::from_slice(&fs::read(receipt_path).map_err(|_|"无法读取资源缓存收据")?).map_err(|_|"资源缓存收据无效")?;
 if receipt["schemaVersion"]!=1||receipt["cacheId"]!=key||workshop_content_identity(&receipt["plan"])!=workshop_content_identity(plan){return Err("资源缓存与固定计划不一致，不能复用".into());}
 let mut total=0u64;
 for entry in plan["files"].as_array().ok_or("资源计划无效")?{
  let relative=entry["path"].as_str().ok_or("资源路径无效")?;
  let mut path=directory.join("content");regular(&path)?;
  for part in relative.split('/'){path.push(part);regular(&path)?;}
  let metadata=regular(&path)?;if !metadata.is_file()||metadata.len()>200*1024*1024{return Err("资源缓存文件类型或大小无效".into());}
  let mut file=fs::File::open(path).map_err(|_|"无法读取资源缓存")?.take(200*1024*1024+1);
  let mut digest=Sha256::new();let size=std::io::copy(&mut file,&mut digest).map_err(|_|"资源缓存读取不完整")?;
  total=total.checked_add(size).ok_or("资源缓存过大")?;
  if size>200*1024*1024||total>512*1024*1024||receipt["sha256"][relative].as_str()!=Some(format!("{:x}",digest.finalize()).as_str()){return Err("资源缓存内容已改变，不能复用；原文件保留".into());}
 }
 if receipt["bytes"].as_u64()!=Some(total){return Err("资源缓存总大小不一致".into());}
 if !plan["requires"].is_null(){receipt["plan"]["requires"]=plan["requires"].clone();}
 Ok(receipt)
}
pub(crate) fn cached_workshop_asset(root:&std::path::Path,id:&str)->Result<(Value,std::path::PathBuf),String>{
 let catalog:ResourceCatalog=serde_json::from_slice(&fs::read(root.join("workshop-catalog.json")).map_err(|_|"请先同步工坊目录")?).map_err(|_|"工坊目录无效")?;
 let item=catalog.entries.iter().find(|item|item.id==id).ok_or("资源不在缓存目录中，请重新预览")?;
 let plan=workshop_asset_plan(item)?;let (receipt,directory)=existing_workshop_cache(root,&plan)?.ok_or("缺少工坊资源缓存，请先下载或验证缓存")?;
 Ok((receipt,directory.join("content")))
}
#[tauri::command]
pub fn workshop_asset_reference(app:tauri::AppHandle,resource_id:String,expected_commit:String)->Result<crate::catalog::WorkshopRef,String>{
 let state=app.state::<crate::data_commands::DataState>();let root=state.root.as_ref().map_err(|_|"工作空间不可用")?;
 let (receipt,_)=cached_workshop_asset(root,&resource_id)?;let plan=&receipt["plan"];
 if plan["commit"].as_str()!=Some(expected_commit.as_str()){return Err("来源提交已变化，请重新预览并校验资源".into());}
 let parts=resource_id.split(':').collect::<Vec<_>>();
 if parts.len()!=3||parts[0]!="workshop"{return Err("工坊资源标识无效".into());}
 let reference=crate::catalog::WorkshopRef{requires:plan["requires"].clone(),
  kind:parts[1].into(),id:parts[2].into(),version:plan["version"].as_str().ok_or("资源版本缺失")?.into(),
  repository:plan["repository"].as_str().ok_or("资源仓库缺失")?.into(),commit:expected_commit,
  content_path:plan["contentPath"].as_str().ok_or("资源目录缺失")?.into(),
  files:serde_json::from_value(receipt["sha256"].clone()).map_err(|_|"资源摘要无效")?
 };
 reference.validate().map_err(|error|error.message)?;Ok(reference)
}
pub(crate) fn require_workshop_host(root:&std::path::Path,instance:&crate::store::Instance,plan:&Value)->Result<(),String>{
 let recipe=crate::managed::recipe(root,instance).map_err(|error|error.message)?;
 require_workshop_recipe_host(root,&recipe,plan)
}
fn workshop_host(directory:Option<&str>)->Result<(&'static str,&'static str),String>{
 Ok(match directory.and_then(|value|value.split('/').next()){
  Some("skins")=>("@linxin666/dsh-client-ui-skin-center","skin-center"),
  Some("pets")=>("@linxin666/dsh-pet","pet"),
  Some("agent-presets")=>("@linxin666/dsh-client-ui-preset-center","preset-center"),
  _=>return Err("资源目标类型不受支持".into())
 })
}
pub(crate) fn require_workshop_recipe_host(root:&std::path::Path,recipe:&crate::catalog::Recipe,plan:&Value)->Result<(),String>{
 if recipe.engine!="DSH"{return Err("请选择 DSH 实例".into());}
 let profile=crate::engine_profile::resolve("DSH",&recipe.profile_id).map_err(|e|e.message)?;
 let package_doc:Value=serde_json::from_str(&profile.package).map_err(|_|"版本组合无效")?;
 require_plan_dsh(plan,package_doc["dependencies"]["@deepseek-ai/dsh"].as_str().ok_or("缺少 DSH 精确版本")?)?;
 let (package,feature)=workshop_host(plan["directory"].as_str())?;
 for name in [package,"@linxin666/dsh-web-all"]{
  if !recipe.extensions.iter().any(|item|item.enabled&&item.id==format!("dsh-npm:{name}")){continue;}
  let path=root.join("artifacts").join(&recipe.profile_id).join("node_modules").join(name).join("package.json");
  let manifest:Value=fs::read(path).ok().and_then(|bytes|serde_json::from_slice(&bytes).ok()).unwrap_or(Value::Null);
  if manifest["name"]!=name||manifest["dsh"]["bundle"].is_null(){continue;}
  if name==package||!manifest["exports"][format!("./{feature}")].is_null(){return Ok(());}
 }
 Err(format!("缺少已安装并启用的资源宿主 {package}；请先通过扩展安装流程添加兼容版本"))
}
fn cache_workshop_asset(root:&std::path::Path,item:&Resource,mut fetch:impl FnMut(&str)->Result<Vec<u8>,String>)->Result<Value,String>{
 use sha2::{Digest,Sha256};
 let plan=workshop_asset_plan(item)?;
 let key=workshop_content_key(&plan);
 let parent=root.join("workshop-assets");let destination=parent.join(&key);
 let _guard=CacheRefresh::begin(&destination)?;
 if let Some((receipt,_))=existing_workshop_cache(root,&plan)?{return Ok(receipt);}
 fs::create_dir_all(&parent).map_err(|_|"无法创建资源缓存目录")?;
 let staging=parent.join(format!(".download-{}",uuid::Uuid::new_v4()));fs::create_dir(&staging).map_err(|_|"无法创建资源下载目录")?;
 let result=(||{
  let mut total=0usize;let mut digests=serde_json::Map::new();
  for file in plan["files"].as_array().ok_or("资源下载计划无效")?{
   let path=file["path"].as_str().ok_or("资源文件名无效")?;
   let data=fetch(file["url"].as_str().ok_or("资源地址无效")?)?;
   total=total.checked_add(data.len()).ok_or("资源大小超过限制")?;
   if data.len()>200*1024*1024||total>512*1024*1024{return Err("资源超过单文件 200 MB 或总计 512 MB 限制".into());}
   let target=staging.join("content").join(path);
   fs::create_dir_all(target.parent().ok_or("资源路径无效")?).map_err(|_|"无法创建资源子目录")?;
   fs::write(target,&data).map_err(|_|"无法写入资源缓存")?;
   digests.insert(path.into(),Value::String(format!("{:x}",Sha256::digest(&data))));
  }
  let receipt=serde_json::json!({"schemaVersion":1,"cacheId":key,"plan":plan,"sha256":digests,"bytes":total});
  fs::write(staging.join("receipt.json"),serde_json::to_vec_pretty(&receipt).map_err(|_|"资源摘要编码失败")?).map_err(|_|"资源摘要写入失败")?;
  fs::rename(&staging,&destination).map_err(|_|"无法启用完整资源缓存，已有缓存保持不变")?;
  Ok(receipt)
 })();
 if result.is_err(){
  if let (Ok(base),Ok(path))=(parent.canonicalize(),staging.canonicalize()){
   if path.parent()==Some(base.as_path()){let _=fs::remove_dir_all(path);}
  }
 }
 result
}
#[tauri::command]
pub fn cancel_workshop_download(operation_id:String)->Result<(),String>{
 let tasks=workshop_downloads().lock().map_err(|_|"下载状态不可用")?;
 let cancel=tasks.get(&operation_id).ok_or("此下载已结束或尚未开始")?;
 cancel.store(true,std::sync::atomic::Ordering::Relaxed);Ok(())
}
fn workshop_downloads()->&'static std::sync::Mutex<std::collections::HashMap<String,std::sync::Arc<std::sync::atomic::AtomicBool>>>{
 static TASKS:std::sync::OnceLock<std::sync::Mutex<std::collections::HashMap<String,std::sync::Arc<std::sync::atomic::AtomicBool>>>>=std::sync::OnceLock::new();
 TASKS.get_or_init(Default::default)
}
struct WorkshopDownload(String);
#[cfg(test)]
#[test]
fn workshop_download_cancel_is_scoped_and_released(){
 let first=uuid::Uuid::new_v4().to_string();let second=uuid::Uuid::new_v4().to_string();
 let (guard,a)=WorkshopDownload::begin(first.clone()).unwrap();
 let (_other,b)=WorkshopDownload::begin(second).unwrap();
 assert!(WorkshopDownload::begin(first.clone()).is_err());
 cancel_workshop_download(first.clone()).unwrap();
 assert!(a.load(std::sync::atomic::Ordering::Relaxed));
 assert!(!b.load(std::sync::atomic::Ordering::Relaxed));
 drop(guard);assert!(cancel_workshop_download(first).is_err());
 assert!(WorkshopDownload::begin("invalid".into()).is_err());
}
impl Drop for WorkshopDownload{fn drop(&mut self){if let Ok(mut tasks)=workshop_downloads().lock(){tasks.remove(&self.0);}}}
impl WorkshopDownload{
 fn begin(id:String)->Result<(Self,std::sync::Arc<std::sync::atomic::AtomicBool>),String>{
  uuid::Uuid::parse_str(&id).map_err(|_|"下载操作标识无效")?;
  let mut tasks=workshop_downloads().lock().map_err(|_|"下载状态不可用")?;
  if tasks.contains_key(&id){return Err("此下载操作正在进行".into());}
  let cancel=std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));tasks.insert(id.clone(),cancel.clone());
  Ok((Self(id),cancel))
 }
}
#[tauri::command]
pub async fn download_workshop_asset(app:tauri::AppHandle,resource_id:String,operation_id:String)->Result<Value,String>{
 let root=app.state::<crate::data_commands::DataState>().root.as_ref().map_err(|_|"工作空间不可用")?.clone();
 let (guard,cancel)=WorkshopDownload::begin(operation_id)?;
 tauri::async_runtime::spawn_blocking(move||{
  let _guard=guard;
  let mut check=||if cancel.load(std::sync::atomic::Ordering::Relaxed){Err("已取消资源下载，已有缓存保留".to_string())}else{Ok(())};
  check()?;
  let catalog:ResourceCatalog=serde_json::from_slice(&fs::read(root.join("workshop-catalog.json")).map_err(|_|"请先同步工坊目录")?).map_err(|_|"工坊目录无效，请刷新")?;
  let item=catalog.entries.iter().find(|item|item.id==resource_id).ok_or("资源不在缓存目录中")?;
  let client=reqwest::blocking::Client::builder().user_agent("Perch-workshop-assets").timeout(Duration::from_secs(30)).redirect(reqwest::redirect::Policy::none()).build().map_err(|_|"资源下载初始化失败")?;
  cache_workshop_asset(&root,item,|url|{
   check()?;
   let response=client.get(url).send().map_err(|_|"资源下载失败，请检查网络")?;
   check()?;
   if !response.status().is_success(){return Err(format!("资源下载返回 HTTP {}",response.status().as_u16()));}
   if response.content_length().is_some_and(|size|size>200*1024*1024){return Err("资源文件超过 200 MB 限制".into());}
   read_workshop_bytes(response,&mut check)
  })
 }).await.map_err(|_|"资源下载任务中断".to_string())?
}

fn cache_workshop_reference(root:&std::path::Path,reference:&crate::catalog::WorkshopRef,mut fetch:impl FnMut(&str)->Result<Vec<u8>,String>)->Result<Value,String>{
 use sha2::{Digest,Sha256};
 reference.validate().map_err(|error|error.message)?;
 let resource=Resource{id:format!("workshop:{}:{}",reference.kind,reference.id),name:reference.id.clone(),description:String::new(),version:reference.version.clone(),engine:"DSH".into(),source:String::new(),homepage:String::new(),kinds:vec![reference.kind.clone()],package_name:None,requirements:serde_json::json!({"requires":reference.requires,"contentSource":{"repository":format!("https://github.com/{}",reference.repository),"commit":reference.commit,"contentPath":reference.content_path},"files":reference.files.keys().collect::<Vec<_>>()})};
 let plan=workshop_asset_plan(&resource)?;
 let expected=plan["files"].as_array().ok_or("资源计划无效")?.iter().map(|file|Ok((file["url"].as_str().ok_or("资源地址无效")?.to_owned(),reference.files[file["path"].as_str().ok_or("资源路径无效")?].clone()))).collect::<Result<std::collections::HashMap<_,_>,String>>()?;
 let receipt=cache_workshop_asset(root,&resource,|url|{
  let bytes=fetch(url)?;let hash=format!("{:x}",Sha256::digest(&bytes));
  if expected.get(url)!=Some(&hash){return Err("下载内容与分享组合的锁定摘要不一致，未启用缓存".into());}Ok(bytes)
 })?;
 let hashes:std::collections::BTreeMap<String,String>=serde_json::from_value(receipt["sha256"].clone()).map_err(|_|"缓存摘要无效")?;
 if hashes!=reference.files{return Err("已有缓存与组合锁定摘要不一致，原缓存保留".into());}
 Ok(receipt)
}
#[tauri::command]
pub async fn prepare_workshop_resources(app:tauri::AppHandle,recipe:crate::catalog::Recipe,offline:bool)->Result<Vec<Value>,String>{
 let root=app.state::<crate::data_commands::DataState>().root.as_ref().map_err(|_|"工作空间不可用")?.clone();
 tauri::async_runtime::spawn_blocking(move||prepare_workshop_recipe(&root,&recipe,offline)).await.map_err(|_|"组合资源准备任务中断".to_string())?
}
pub(crate) fn prepare_workshop_recipe(root:&std::path::Path,recipe:&crate::catalog::Recipe,offline:bool)->Result<Vec<Value>,String>{
 prepare_workshop_recipe_controlled(root,recipe,offline,||Ok(()),|_,_|Ok(()))
}
fn read_workshop_bytes(mut reader:impl Read,check:&mut impl FnMut()->Result<(),String>)->Result<Vec<u8>,String>{
 let mut bytes=Vec::new();let mut buffer=[0u8;64*1024];
 loop {
  check()?;
  let count=reader.read(&mut buffer).map_err(|_|"资源读取中断")?;
  check()?;
  if count==0{break;}
  if bytes.len()+count>200*1024*1024{return Err("资源文件超过 200 MB".into());}
  bytes.extend_from_slice(&buffer[..count]);
 }
 Ok(bytes)
}
pub(crate) fn prepare_workshop_recipe_controlled(root:&std::path::Path,recipe:&crate::catalog::Recipe,offline:bool,mut check:impl FnMut()->Result<(),String>,mut progress:impl FnMut(usize,usize)->Result<(),String>)->Result<Vec<Value>,String>{
 check()?;
 recipe.validate_metadata().map_err(|error|error.message)?;
 recipe.validate_workshop_hosts().map_err(|error|error.message)?;
 if recipe.engine!="DSH"&&!recipe.workshop.is_empty(){return Err("工坊资源只能用于 DSH 组合".into());}
 if recipe.workshop.is_empty(){return Ok(Vec::new());}
  let client=reqwest::blocking::Client::builder().user_agent("Perch-workshop-rebuild").timeout(Duration::from_secs(30)).redirect(reqwest::redirect::Policy::none()).build().map_err(|_|"下载初始化失败")?;
  let mut receipts=Vec::new();
  for (index,reference) in recipe.workshop.iter().enumerate(){
   check()?;progress(index+1,recipe.workshop.len())?;
   let receipt=cache_workshop_reference(&root,reference,|url|{
   check()?;
   if offline{return Err(format!("离线缓存缺少工坊资源 {} / {}",reference.kind,reference.id));}
   let response=client.get(url).send().map_err(|_|"锁定资源下载失败")?;
   check()?;
   if !response.status().is_success(){return Err(format!("锁定资源返回 HTTP {}",response.status().as_u16()));}
   if response.content_length().is_some_and(|size|size>200*1024*1024){return Err("资源文件超过 200 MB".into());}
   read_workshop_bytes(response,&mut check)
  })?;
   check()?;receipts.push(receipt);
  }
  Ok(receipts)
}
pub(crate) fn cached_workshop_reference(root:&std::path::Path,reference:&crate::catalog::WorkshopRef)->Result<(Value,std::path::PathBuf),String>{
 let receipt=cache_workshop_reference(root,reference,|_|Err(format!("缺少锁定工坊资源 {} / {}，请先准备资源",reference.kind,reference.id)))?;
 let key=receipt["cacheId"].as_str().ok_or("资源缓存标识无效")?;
 let content=root.join("workshop-assets").join(key).join("content");
 Ok((receipt,content))
}

#[cfg(test)]
mod workshop_tests{
 use super::*;
 #[test]
 #[ignore="one explicitly authorized real workshop acceptance; network and isolated processes"]
 fn real_workshop_share_acceptance(){
  let root=std::env::temp_dir().join(format!("perch-workshop-acceptance-{}",uuid::Uuid::new_v4()));
  fs::create_dir_all(&root).unwrap();println!("ACCEPTANCE_ROOT {}",root.display());
  let catalog=workshop(&root,true).expect("real workshop catalog");
  assert!(catalog.errors.is_empty(),"catalog errors: {:?}",catalog.errors);
  let item=catalog.entries.iter().filter(|item|item.kinds.contains(&"skins".to_string())&&item.requirements["files"].is_array()).min_by_key(|item|item.requirements["files"].as_array().unwrap().len()).expect("skin with complete file declaration");
  println!("RESOURCE {} {}",item.id,item.version);
  let client=reqwest::blocking::Client::builder().user_agent("Perch-workshop-acceptance").timeout(Duration::from_secs(30)).redirect(reqwest::redirect::Policy::none()).build().unwrap();
  let cancel_root=root.join("cancel-check");
  let operation=uuid::Uuid::new_v4().to_string();let (cancel_guard,cancelled)=WorkshopDownload::begin(operation.clone()).unwrap();
  let stopped=cache_workshop_asset(&cancel_root,item,|url|{
   let response=client.get(url).send().map_err(|e|e.to_string())?.error_for_status().map_err(|e|e.to_string())?;
   let mut checks=0;read_workshop_bytes(response,&mut ||{checks+=1;if checks==2{cancel_workshop_download(operation.clone())?;}if cancelled.load(std::sync::atomic::Ordering::Relaxed){Err("已取消".into())}else{Ok(())}})
  });
  assert!(stopped.unwrap_err().contains("已取消"));drop(cancel_guard);
  assert!(fs::read_dir(cancel_root.join("workshop-assets")).unwrap().next().is_none());println!("REAL_DOWNLOAD_CANCELLED staging clean");
  let receipt=cache_workshop_asset(&root,item,|url|{let response=client.get(url).send().map_err(|e|e.to_string())?;if !response.status().is_success(){return Err(format!("HTTP {}",response.status()));}read_workshop_bytes(response,&mut ||Ok(()))}).expect("real resource download");
  let plan=&receipt["plan"];let parts=item.id.split(':').collect::<Vec<_>>();
  let reference=crate::catalog::WorkshopRef{requires:plan["requires"].clone(),kind:parts[1].into(),id:parts[2].into(),version:item.version.clone(),repository:plan["repository"].as_str().unwrap().into(),commit:plan["commit"].as_str().unwrap().into(),content_path:plan["contentPath"].as_str().unwrap().into(),files:serde_json::from_value(receipt["sha256"].clone()).unwrap()};
  println!("DOWNLOAD_OK {} files",reference.files.len());
  let host="@linxin666/dsh-client-ui-skin-center";
  let manifest:Value=client.get(format!("https://registry.npmjs.org/{}/0.4.3",host.replace('/',"%2F"))).send().unwrap().error_for_status().unwrap().json().unwrap();
  let version=manifest["version"].as_str().unwrap();assert!(!manifest["dsh"]["bundle"].is_null(),"host must declare DSH bundle");
  let mut recipe=crate::catalog::packs().remove(0);
  let base=crate::engine_profile::resolve("DSH",&recipe.profile_id).unwrap();let package:Value=serde_json::from_str(&base.package).unwrap();let mut dependencies=package["dependencies"].as_object().unwrap().clone();dependencies.insert(host.into(),Value::String(version.into()));dependencies.insert("@deepseek-ai/dsh".into(),Value::String("0.1.7-rc.1".into()));
  recipe.profile_id=crate::version_catalog::lock_plan(&root,crate::version_catalog::VersionPlan{engine:"DSH".into(),version:"workshop-acceptance".into(),dependencies:Value::Object(dependencies),runtime:crate::engine_profile::PI.node.into(),warnings:vec![]}).expect("lock actual host");
  recipe.extensions.push(crate::catalog::ExtensionRef{id:format!("dsh-npm:{host}"),version:version.into(),enabled:true,disabled_resources:vec![],resource_rules:Default::default()});recipe.workshop.push(reference);
  let connection=crate::store::ModelConnection{id:uuid::Uuid::new_v4().to_string(),name:"isolated acceptance".into(),provider:"test".into(),protocol:"deepseek".into(),base_url:"http://127.0.0.1:9/v1".into(),default_model:"test".into(),models:vec![],has_key:true,revision:1};
  let engines=crate::engine::Engines::default();
  let outcome=std::panic::catch_unwind(std::panic::AssertUnwindSafe(||{
   for phase in ["source","rebuilt"]{
    let target=if phase=="source"{root.clone()}else{root.join("rebuilt")};fs::create_dir_all(&target).unwrap();
    let selected=if phase=="source"{recipe.clone()}else{let bytes=crate::pack_drafts::export_bytes(&recipe).unwrap();assert!(!String::from_utf8_lossy(&bytes).contains(&connection.id));crate::pack_drafts::import_bytes(&target,&bytes).expect("import shared recipe")};
    let instance=crate::store::Instance{schema_version:1,id:uuid::Uuid::new_v4().to_string(),name:phase.into(),engine:"DSH".into(),project_path:target.to_string_lossy().into(),connection_id:Some(connection.id.clone()),profile_id:selected.profile_id.clone(),revision:1,created_at:0,updated_at:0};
    engines.begin(&instance,"install").unwrap();engines.execute(&target,&instance,None,"install").expect("install actual host");
    prepare_workshop_recipe(&target,&selected,false).expect("prepare locked resource");crate::managed::apply(&target,&instance,&selected,"workshop acceptance").expect("apply resource");
    assert_eq!(crate::managed::workshop_references(&target,&instance).unwrap(),selected.workshop);
    engines.begin(&instance,"start").unwrap();engines.execute(&target,&instance,Some((connection.clone(),"fake-workshop-test".into())),"start").expect("start actual host");
    // open() verifies DSH's authentication redirect, cookie and rendered boot page.
    // The initial token URL deliberately returns 302, not 200.
    engines.open(&instance.id).expect("authenticated host page is healthy");
    let log=fs::read_to_string(target.join("instances").join(&instance.id).join("logs/engine.log")).unwrap();assert!(log.contains(&format!("PERCH_BUNDLE_MODULE_LOADED: {host}")),"host module not loaded");
    println!("HOST_STARTED {phase} instance={} resource={}",instance.id,selected.workshop[0].id);
    engines.stop(&instance.id).unwrap();
   }
  }));engines.shutdown();if let Err(error)=outcome{std::panic::resume_unwind(error);}
  println!("SHARE_REBUILD_OK; visual theme activation still requires host UI confirmation");
 }
 #[test]
 fn workshop_transfer_cancellation_discards_partial_resource(){
  use sha2::{Digest,Sha256};use std::cell::Cell;
  let root=std::env::temp_dir().join(format!("perch-workshop-cancel-{}",uuid::Uuid::new_v4()));
  fs::create_dir_all(&root).unwrap();fs::write(root.join("existing.txt"),"keep").unwrap();
  let recipe=crate::catalog::packs().remove(0);
  let result=prepare_workshop_recipe_controlled(&root,&recipe,false,||Err("cancelled".into()),|_,_|panic!("cancelled before progress"));
  assert_eq!(result.unwrap_err(),"cancelled");
  struct CancelOnRead<'a>(&'a Cell<bool>);
  impl Read for CancelOnRead<'_>{fn read(&mut self,buffer:&mut [u8])->std::io::Result<usize>{buffer[..2].copy_from_slice(b"{}");self.0.set(true);Ok(2)}}
  let cancelled=Cell::new(false);
  let hash=format!("{:x}",Sha256::digest(b"{}"));
  let reference=crate::catalog::WorkshopRef{requires:Default::default(),kind:"skins".into(),id:"sample".into(),version:"1".into(),repository:"example/skins".into(),commit:"0123456789012345678901234567890123456789".into(),content_path:"skins".into(),files:std::collections::BTreeMap::from([("skin.json".into(),hash.clone()),("z.css".into(),hash)])};
  let result=cache_workshop_reference(&root,&reference,|url|{
   if url.ends_with("skin.json"){return Ok(b"{}".to_vec());}
   read_workshop_bytes(CancelOnRead(&cancelled),&mut ||if cancelled.get(){Err("cancelled".into())}else{Ok(())})
  });
  assert_eq!(result.unwrap_err(),"cancelled");
  assert_eq!(fs::read_dir(root.join("workshop-assets")).unwrap().count(),0);
  assert_eq!(fs::read_to_string(root.join("existing.txt")).unwrap(),"keep");
  fs::remove_dir_all(root).unwrap();
 }
 #[test]
 fn workshop_recipe_transaction_preserves_live_data_on_conflict(){
  use sha2::{Digest,Sha256};
  let root=std::env::temp_dir().join(format!("perch-workshop-rebuild-{}",uuid::Uuid::new_v4()));
  let name="@linxin666/dsh-client-ui-skin-center";
  let mut profile=crate::engine_profile::ResolvedProfile::from(&crate::engine_profile::DSH);
  profile.id=format!("resolved-{}",uuid::Uuid::new_v4());
  let mut package:Value=serde_json::from_str(&profile.package).unwrap();package["dependencies"][name]=Value::String("1.0.0".into());
  let mut lock:Value=serde_json::from_str(&profile.lock).unwrap();lock["packages"][""]["dependencies"]=package["dependencies"].clone();
  lock["packages"][format!("node_modules/{name}")]=serde_json::json!({"version":"1.0.0","resolved":"https://registry.npmjs.org/fixture/-/fixture-1.0.0.tgz","integrity":"sha512-fixture"});
  profile.package=package.to_string();profile.lock=lock.to_string();crate::engine_profile::register(&root,profile.clone()).unwrap();
  let instance=crate::store::Instance{schema_version:1,id:uuid::Uuid::new_v4().to_string(),name:"fixture".into(),engine:"DSH".into(),project_path:root.to_string_lossy().into(),connection_id:None,profile_id:profile.id.clone(),revision:1,created_at:0,updated_at:0};
  let mut recipe=crate::managed::recipe(&root,&instance).unwrap();
  recipe.extensions.push(serde_json::from_value(serde_json::json!({"id":format!("dsh-npm:{name}"),"version":"1.0.0","enabled":true})).unwrap());
  let reference=crate::catalog::WorkshopRef{requires:Default::default(),kind:"skins".into(),id:"sample".into(),version:"1".into(),repository:"example/skins".into(),commit:"0123456789012345678901234567890123456789".into(),content_path:"skins".into(),files:std::collections::BTreeMap::from([("skin.json".into(),format!("{:x}",Sha256::digest(b"{}")))])};
  let receipt=cache_workshop_reference(&root,&reference,|_|Ok(b"{}".to_vec())).unwrap();
  let cache=root.join("workshop-assets").join(receipt["cacheId"].as_str().unwrap()).join("content");fs::write(cache.join("unlisted.txt"),"must not install").unwrap();
  recipe.workshop.push(reference.clone());
  let base=root.join("instances").join(&instance.id);let home=base.join("agent-data");
  let host=root.join("artifacts").join(&profile.id).join("node_modules").join(name);fs::create_dir_all(&host).unwrap();
  fs::write(host.join("package.json"),serde_json::json!({"name":name,"dsh":{"bundle":"bundle.yml"}}).to_string()).unwrap();
  crate::managed::apply(&root,&instance,&recipe,"install fixture").unwrap();
  assert_eq!(crate::managed::workshop_references(&root,&instance).unwrap(),vec![reference]);
  let portable=crate::pack_drafts::export_bytes(&recipe).unwrap();
  let imported=crate::pack_drafts::import_bytes(&root,&portable).unwrap();
  assert_eq!(imported.workshop,recipe.workshop);
  assert_ne!(imported.profile_id,recipe.profile_id);
  crate::managed::apply(&root,&instance,&recipe,"unchanged fixture").unwrap();
  let mut conflict=recipe.clone();conflict.workshop[0].files.insert("skin.json".into(),"a".repeat(64));
  assert!(crate::managed::apply(&root,&instance,&conflict,"bad cache").is_err());
  assert_eq!(crate::managed::recipe(&root,&instance).unwrap(),recipe);
  assert_eq!(crate::managed::workshop_references(&root,&instance).unwrap(),recipe.workshop);
  fs::write(home.join("skins/sample/skin.json"),"user edit").unwrap();
  assert!(crate::managed::apply(&root,&instance,&recipe,"reject drift").is_err());
  assert_eq!(fs::read_to_string(home.join("skins/sample/skin.json")).unwrap(),"user edit");
  fs::write(home.join("skins/sample/skin.json"),"{}").unwrap();
  let mut removed=recipe.clone();removed.workshop.clear();crate::managed::apply(&root,&instance,&removed,"remove fixture").unwrap();
  assert!(crate::managed::workshop_references(&root,&instance).unwrap().is_empty());
  fs::remove_dir_all(root).unwrap();
 }
 #[test]
 fn locked_workshop_reference_checks_expected_hash_and_offline_cache(){
  use sha2::{Digest,Sha256};
  let root=std::env::temp_dir().join(format!("perch-reference-cache-{}",uuid::Uuid::new_v4()));
  let reference=crate::catalog::WorkshopRef{requires:Default::default(),kind:"skins".into(),id:"sample".into(),version:"1".into(),repository:"example/skins".into(),commit:"0123456789012345678901234567890123456789".into(),content_path:"skins".into(),files:std::collections::BTreeMap::from([("skin.json".into(),format!("{:x}",Sha256::digest(b"{}")))])};
  assert!(cache_workshop_reference(&root,&reference,|_|Err("offline".into())).is_err());
  assert!(cache_workshop_reference(&root,&reference,|_|Ok(b"changed".to_vec())).is_err());
  assert_eq!(fs::read_dir(root.join("workshop-assets")).unwrap().count(),0);
  let saved=cache_workshop_reference(&root,&reference,|_|Ok(b"{}".to_vec())).unwrap();
  assert_eq!(cache_workshop_reference(&root,&reference,|_|panic!("offline reuse must not fetch")).unwrap(),saved);
  let mut changed=reference.clone();changed.files.insert("skin.json".into(),"b".repeat(64));
  assert!(cache_workshop_reference(&root,&changed,|_|panic!("do not overwrite cache")).is_err());
  fs::remove_dir_all(root).unwrap();
 }
 #[test]
 fn workshop_download_commits_complete_set_and_discards_failure(){
  let root=std::env::temp_dir().join(format!("perch-workshop-download-{}",uuid::Uuid::new_v4()));
  let item=Resource{id:"workshop:skins:sample".into(),name:"Sample".into(),description:String::new(),version:"1".into(),engine:"DSH".into(),source:String::new(),homepage:String::new(),kinds:vec![],package_name:None,requirements:serde_json::json!({"contentSource":{"repository":"https://github.com/example/skins","commit":"0123456789012345678901234567890123456789","contentPath":"skins"},"files":["skin.json","skin.css"]})};
  let mut calls=0;
  assert!(cache_workshop_asset(&root,&item,|_|{calls+=1;if calls==2{Err("offline".into())}else{Ok(b"{}".to_vec())}}).is_err());
  assert_eq!(fs::read_dir(root.join("workshop-assets")).unwrap().count(),0);
  let receipt=cache_workshop_asset(&root,&item,|_|Ok(b"fixture".to_vec())).unwrap();
  let directory=root.join("workshop-assets").join(receipt["cacheId"].as_str().unwrap());
  assert_eq!(fs::read(directory.join("content/skin.css")).unwrap(),b"fixture");
  assert_eq!(receipt["sha256"].as_object().unwrap().len(),2);
  let before=fs::read(directory.join("receipt.json")).unwrap();
  assert!(cache_workshop_asset(&root,&item,|_|panic!("must not download again")).is_ok());
  assert_eq!(fs::read(directory.join("receipt.json")).unwrap(),before);
  fs::write(directory.join("content/skin.css"),"changed").unwrap();
  assert!(cache_workshop_asset(&root,&item,|_|panic!("must not overwrite changed cache")).is_err());
  assert_eq!(fs::read_to_string(directory.join("content/skin.css")).unwrap(),"changed");
  fs::remove_dir_all(root).unwrap();
 }
 #[test]
 fn workshop_asset_plan_pins_paths_and_rejects_invalid_files(){
  let mut item=Resource{id:"workshop:presets:review".into(),name:"Review".into(),description:String::new(),version:"1".into(),engine:"DSH".into(),source:String::new(),homepage:String::new(),kinds:vec![],package_name:None,requirements:serde_json::json!({"contentSource":{"repository":"https://github.com/example/presets","commit":"0123456789012345678901234567890123456789","contentPath":"presets"},"files":["preset.yml","agent.cordis.yml"]})};
  let plan=workshop_asset_plan(&item).unwrap();assert_eq!(plan["directory"],"agent-presets/review");assert!(plan["files"][0]["url"].as_str().unwrap().ends_with("/0123456789012345678901234567890123456789/presets/review/preset.yml"));
  for files in [serde_json::json!(["../escape"]),serde_json::json!(["preset.yml","PRESET.yml"]),serde_json::json!(["agent.cordis.yml"])]{item.requirements["files"]=files;assert!(workshop_asset_plan(&item).is_err());}
 }
 #[test]
 fn resource_update_preserves_position_and_loading_choices(){
  let mut entries=vec![crate::catalog::ExtensionRef{id:"npm:first".into(),version:"1.0.0".into(),enabled:false,resource_rules:Default::default(),disabled_resources:vec!["themes".into()]},crate::catalog::ExtensionRef{id:"npm:second".into(),version:"1.0.0".into(),enabled:true,resource_rules:Default::default(),disabled_resources:vec![]}];
  update_resource(&mut entries,"npm:first".into(),"2.0.0".into());
  assert_eq!(entries[0].id,"npm:first");assert_eq!(entries[0].version,"2.0.0");assert!(!entries[0].enabled);assert_eq!(entries[0].disabled_resources,vec!["themes"]);assert_eq!(entries[1].id,"npm:second");
  update_resource(&mut entries,"npm:third".into(),"1.0.0".into());
  assert_eq!(entries.len(),3);assert_eq!(entries[2].id,"npm:third");assert!(entries[2].enabled);
 }
 #[test]
 fn source_rate_limit_headers_and_cooldown_are_scoped(){
  use reqwest::header::{HeaderMap,HeaderValue};
  let mut headers=HeaderMap::new();
  headers.insert("x-ratelimit-remaining",HeaderValue::from_static("0"));
  headers.insert("x-ratelimit-reset",HeaderValue::from_static("1120"));
  assert_eq!(source_retry_seconds(&headers,1000),Some(120));
  headers.insert("retry-after",HeaderValue::from_static("30"));
  assert_eq!(source_retry_seconds(&headers,1000),Some(30));
  headers.remove("retry-after");headers.insert("x-ratelimit-remaining",HeaderValue::from_static("1"));
  assert_eq!(source_retry_seconds(&headers,1000),None);
  headers.insert("retry-after",HeaderValue::from_static("Thu, 01 Jan 1970 00:20:00 GMT"));
  assert_eq!(source_retry_seconds(&headers,1000),Some(200));
  headers.insert("retry-after",HeaderValue::from_static("Thu, 01 Jan 1970 00:10:00 GMT"));
  assert_eq!(source_retry_seconds(&headers,1000),None);
  headers.insert("retry-after",HeaderValue::from_static("not-a-date"));
  assert_eq!(source_retry_seconds(&headers,1000),None);
  let origin=format!("https://{}.invalid",uuid::Uuid::new_v4());
  SOURCE_COOLDOWNS.lock().unwrap().insert(origin.clone(),std::time::Instant::now()+Duration::from_secs(30));
  assert!(source_cooldown(&origin).unwrap_err().contains("未再次请求"));
  assert!(source_cooldown("https://unrelated-source.invalid").is_ok());
  SOURCE_COOLDOWNS.lock().unwrap().insert(origin.clone(),std::time::Instant::now()-Duration::from_secs(1));
  assert!(source_cooldown(&origin).is_ok());
  assert!(!SOURCE_COOLDOWNS.lock().unwrap().contains_key(&origin));
 }
 #[test]
 fn conditional_sources_reuse_304_and_keep_cache_after_error(){
  use std::io::Write;
  let root=std::env::temp_dir().join(format!("perch-http-cache-{}",uuid::Uuid::new_v4()));
  let listener=std::net::TcpListener::bind("127.0.0.1:0").unwrap();let address=format!("http://{}/catalog",listener.local_addr().unwrap());
  let server=std::thread::spawn(move||{
   let responses=[("200 OK","ETag: \"one\"\r\n","{\"version\":1}"),("304 Not Modified","",""),("503 Unavailable","Retry-After: 12\r\n",""),("200 OK","Last-Modified: Wed, 23 Sep 2026 12:00:00 GMT\r\n","{\"version\":2}"),("304 Not Modified","",""),("200 OK","ETag: \"alternate\"\r\n","{\"version\":3}"),("304 Not Modified","","")];
   for (index,(status,headers,body)) in responses.into_iter().enumerate(){
    let (mut socket,_)=listener.accept().unwrap();socket.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
    let mut request=Vec::new();let mut byte=[0];while !request.ends_with(b"\r\n\r\n"){socket.read_exact(&mut byte).unwrap();request.push(byte[0]);assert!(request.len()<8192);}
    let request=String::from_utf8(request).unwrap().to_lowercase();
    if (1..=3).contains(&index){assert!(request.contains("if-none-match: \"one\""));}
    if index==5{assert!(request.contains("accept: application/vnd.npm.install-v1+json"));assert!(!request.contains("if-none-match"));assert!(!request.contains("if-modified-since"));}
    if index==4||index==6{assert!(request.contains("if-modified-since: wed, 23 sep 2026 12:00:00 gmt"));assert!(!request.contains("if-none-match"));}
    write!(socket,"HTTP/1.1 {status}\r\n{headers}Content-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len()).unwrap();
   }
  });
  let client=reqwest::blocking::Client::builder().no_proxy().timeout(Duration::from_secs(5)).build().unwrap();
  assert_eq!(conditional_json(&root,&client,&address).unwrap()["version"],1);
  let path=root.join("source-http-cache").join(format!("{}.json",manifest_cache_key(&address)));let saved=fs::read(&path).unwrap();
  assert_eq!(conditional_json(&root,&client,&address).unwrap()["version"],1);
  assert!(conditional_json(&root,&client,&address).unwrap_err().contains("12 秒"));assert_eq!(fs::read(&path).unwrap(),saved);
  assert!(conditional_json(&root,&client,&address).unwrap_err().contains("未再次请求"));
  // Advance only this fixture's deadline without sleeping or issuing another request.
  let origin=reqwest::Url::parse(&address).unwrap().origin().ascii_serialization();
  SOURCE_COOLDOWNS.lock().unwrap().insert(origin,std::time::Instant::now()-Duration::from_secs(1));
  assert_eq!(conditional_json(&root,&client,&address).unwrap()["version"],2);
  assert_eq!(conditional_json(&root,&client,&address).unwrap()["version"],2);
  assert_eq!(conditional_json_with(&root,&client,&address,Some("application/vnd.npm.install-v1+json"),32*1024*1024).unwrap()["version"],3);
  assert_eq!(conditional_json(&root,&client,&address).unwrap()["version"],2);
  server.join().unwrap();fs::remove_dir_all(root).unwrap();
 }
 #[test]
 fn cache_refresh_excludes_duplicates_and_failed_publish_preserves_content(){
  let root=std::env::temp_dir().join(format!("perch-source-cache-{}",uuid::Uuid::new_v4()));fs::create_dir_all(&root).unwrap();let path=root.join("catalog.json");
  save_cache(&path,b"old").unwrap();
  let first=CacheRefresh::begin(&path).unwrap();let duplicate=path.clone();
  assert!(std::thread::spawn(move||CacheRefresh::begin(&duplicate).is_err()).join().unwrap());
  let other=CacheRefresh::begin(&root.join("other.json")).unwrap();drop(other);drop(first);
  let retry=CacheRefresh::begin(&path).unwrap();save_cache(&path,b"new").unwrap();assert_eq!(fs::read(&path).unwrap(),b"new");drop(retry);
  // A sharing violation on Windows must not truncate the previous cache.
  #[cfg(windows)]{
   use std::os::windows::fs::OpenOptionsExt;
   let held=fs::OpenOptions::new().read(true).share_mode(1).open(&path).unwrap();
   assert!(save_cache(&path,b"replacement").is_err());assert_eq!(fs::read(&path).unwrap(),b"new");drop(held);
  }
  assert!(fs::read_dir(&root).unwrap().all(|entry|entry.unwrap().path().extension().is_some_and(|ext|ext=="json")));
  fs::remove_dir_all(root).unwrap();
 }
 #[test]
 fn explicit_platform_conflicts_are_rejected_before_resolution(){
  for (rules,current,allowed) in [(serde_json::json!(["win32"]),"win32",true),(serde_json::json!(["linux"]),"win32",false),(serde_json::json!(["!win32","win32"]),"win32",false),(serde_json::json!(["!arm64"]),"x64",true),(serde_json::json!("any"),"x64",true),(serde_json::json!([]),"x64",true)]{assert_eq!(platform_allows(&rules,current).unwrap(),allowed);}
  let manifest=parse_manifest(serde_json::json!({"name":"fixture","version":"1.0.0","os":["linux"],"pi":{"extensions":["index.js"]}})).unwrap();
  assert!(validate_resource_platform(&manifest).unwrap_err().contains("win32"));
  assert!(platform_allows(&serde_json::json!([123]),"x64").is_err());
 }
 #[test]
 fn manifest_cache_identity_and_package_requirements(){
  assert_ne!(manifest_cache_key("@scope/name"),manifest_cache_key("_scope__name"));
  let doc=serde_json::json!({"name":"@scope/name","version":"1.2.3","license":"MIT","engines":{"node":">=24"},"os":["win32"],"cpu":["x64"],"pi":{"skills":["skills"]}});
  assert!(expected_manifest(doc.clone(),"_scope__name","1.2.3").is_err());
  assert!(expected_manifest(doc.clone(),"@scope/name","1.2.4").is_err());
  let parsed=expected_manifest(doc,"@scope/name","1.2.3").unwrap();
  assert_eq!(parsed.package_info["license"],"MIT");assert_eq!(parsed.package_info["os"],serde_json::json!(["win32"]));assert_eq!(parsed.package_info["engines"]["node"],">=24");assert!(parsed.kinds.contains(&"skills".into()));
 }
 #[test]
 fn npm_pagination_reads_remaining_pages_and_preserves_cache_on_failure(){
  let row=|n:usize|serde_json::json!({"package":{"name":format!("fixture-{n}"),"version":"1.0.0"}});
  let mut offsets=Vec::new();
  let (entries,incomplete)=npm_pages("Pi",|offset|{offsets.push(offset);Ok(serde_json::json!({"total":251,"objects":if offset==0{(0..250).map(&row).collect::<Vec<_>>()}else{vec![row(250)]}}))}).unwrap();
  assert_eq!(offsets,vec![0,250]);assert_eq!(entries.len(),251);assert!(!incomplete);
  let mut catalog=ResourceCatalog::default();apply_source_result(&mut catalog,"Pi",Ok(entries),123);
  let result=npm_pages("Pi",|offset|{if offset>0{return Err("HTTP 429，请在 60 秒后重试".into());}Ok(serde_json::json!({"total":251,"objects":(0..250).map(&row).collect::<Vec<_>>()}))}).unwrap();
  assert_eq!(result.0.len(),250);assert!(result.1);
  apply_source_result(&mut catalog,"Pi",Err("HTTP 429".into()),456);assert_eq!(catalog.entries.len(),251);assert_eq!(catalog.source_times["Pi"],123);assert!(catalog.errors[0].contains("429"));
  assert!(npm_pages("Pi",|_|Ok(serde_json::json!({"total":5,"objects":[]}))).is_err());
  let (items,partial)=npm_pages("Pi",|offset|Ok(serde_json::json!({"total":251,"objects":if offset==0{(0..250).map(&row).collect::<Vec<_>>()}else{vec![row(0)]}}))).unwrap();assert_eq!(items.len(),250);assert!(partial);
  let (items,partial)=npm_pages("Pi",|offset|Ok(serde_json::json!({"total":5001,"objects":(offset..offset+250).map(&row).collect::<Vec<_>>()}))).unwrap();assert_eq!(items.len(),5000);assert!(partial);
 }
 #[test]
 fn workshop_mapping_preserves_fixed_source_and_rejects_changed_contract(){
  let mut lock=serde_json::json!({"version":2,"inputs":{"skins":{"submodule":"new-place/skins","path":"new-content/themes"}}});
  assert_eq!(workshop_input(&lock,"skins").unwrap(),("new-place/skins","new-content/themes"));
  let module=serde_json::json!({"sha":"0123456789012345678901234567890123456789","submodule_git_url":"https://github.com/example/themes.git"});
  let pin=workshop_pin(&module,"new-content/themes").unwrap();
  assert_eq!(pin["repository"],"https://github.com/example/themes");assert_eq!(pin["contentPath"],"new-content/themes");
  lock["inputs"]["skins"]["path"]=serde_json::json!("../escape");assert!(workshop_input(&lock,"skins").is_err());
  lock["version"]=serde_json::json!(3);assert!(workshop_input(&lock,"skins").is_err());
  assert!(workshop_pin(&serde_json::json!({"sha":"main","submodule_git_url":"https://github.com/example/themes.git"}),".").is_err());
 }
 #[test]
 fn disabled_workshop_reads_cache_without_sync(){
  let root=std::env::temp_dir().join(format!("perch-source-disabled-{}",uuid::Uuid::new_v4()));fs::create_dir_all(&root).unwrap();
  assert!(preferences(&root).unwrap().workshop);
  fs::write(root.join("resource-sources.json"),r#"{"workshop":false}"#).unwrap();
  let settings=preferences(&root).unwrap();assert!(!settings.workshop);assert!(settings.npm_pi&&settings.npm_dsh);
  let mut cached=ResourceCatalog::default();cached.synced_at=123;cached.errors.push("previous failure".into());
  fs::write(root.join("workshop-catalog.json"),serde_json::to_vec(&cached).unwrap()).unwrap();
  let result=workshop(&root,true).unwrap();assert_eq!(result.synced_at,123);assert_eq!(result.errors,cached.errors);
  fs::write(root.join("resource-sources.json"),"invalid").unwrap();assert!(workshop(&root,true).is_err());
  fs::remove_dir_all(root).unwrap();
 }

 #[test]
 fn failed_source_retains_entries_and_success_time(){
  let mut catalog=ResourceCatalog::default();
  let item=Resource{id:"npm:fixture".into(),name:"fixture".into(),description:String::new(),version:"1.0.0".into(),engine:"Pi".into(),source:"npm".into(),homepage:String::new(),kinds:vec![],package_name:None,requirements:Value::Null};
  apply_source_result(&mut catalog,"Pi",Ok(vec![item]),100);
  apply_source_result(&mut catalog,"Pi",Err("offline".into()),200);
  assert_eq!(catalog.entries.len(),1);assert_eq!(catalog.source_times["Pi"],100);assert!(catalog.errors[0].contains("offline"));
  apply_source_result(&mut catalog,"DSH",Ok(vec![]),200);
  assert_eq!(catalog.source_times["DSH"],200);assert_eq!(catalog.entries.len(),1);
  let restored:ResourceCatalog=serde_json::from_slice(&serde_json::to_vec(&catalog).unwrap()).unwrap();assert_eq!(restored.source_times["Pi"],100);assert_eq!(restored.errors,catalog.errors);
 }

 #[test]
 #[ignore="reads public workshop manifests"]
 fn actual_workshop_catalog(){
  let root=std::env::temp_dir().join(format!("perch-workshop-{}",uuid::Uuid::new_v4()));fs::create_dir_all(&root).unwrap();
  let catalog=workshop(&root,true).unwrap();
  for kind in ["plugins","skins","pets","presets"]{assert!(catalog.entries.iter().any(|item|item.kinds.contains(&kind.to_string())));}
  assert!(catalog.entries.iter().any(|item|item.package_name.is_some()));
  assert!(catalog.entries.iter().all(|item|item.requirements["contentSource"]["commit"].as_str().is_some_and(|sha|sha.len()==40)));
  assert_eq!(workshop(&root,false).unwrap().synced_at,catalog.synced_at);
  println!("Workshop contains {} entries; cache {}",catalog.entries.len(),root.display());
 }
}

#[cfg(test)]
mod review_1312_fixes {
 use super::*;
 #[test]
 fn review_1312_content_identity_ignores_presentation_but_not_content(){
  let plan=serde_json::json!({"id":"workshop:skins:sample","version":"1","repository":"owner/repo","commit":"fixed","contentPath":"skins","directory":"skins/sample","name":"展示名称","requires":{"dsh":">=1.0.0"},"files":[{"path":"z.css","url":"https://example/z"},{"path":"skin.json","url":"https://example/skin"}]});
  let mut other=plan.clone();other["name"]="sample".into();other["requires"]=Value::Null;other["files"].as_array_mut().unwrap().reverse();
  assert_eq!(workshop_content_key(&plan),workshop_content_key(&other));
  other["commit"]="different".into();assert_ne!(workshop_content_key(&plan),workshop_content_key(&other));
 }
 #[test]
 fn review_1312_dsh_ranges_fail_closed(){
  for (range,version,allowed) in [(">=0.1.5-rc.3","0.1.5-rc.3",true),(">=0.2.0","0.1.5-rc.3",false),("1.2.3","1.2.4",false),(">=1.0.0 <2.0.0","1.8.0",true),("^1.0.0 || ^2.0.0","2.1.0",true),("not-a-range","1.0.0",false),("1.2","1.3.0",false),("1.2","1.2.9",true)]{
   assert_eq!(require_dsh_version(&Value::String(range.into()),version).is_ok(),allowed,"{range} / {version}");
  }
  assert!(require_dsh_version(&serde_json::json!({"unknown":true}),"1.0.0").is_err());
 }
}
