use crate::{
    credentials::{Secrets, SystemSecrets},
    engine::Engines,
    store::*,
};
use std::sync::Mutex;
use tauri::Manager;
pub struct DataState {
    pub _workspace_lock: Option<std::fs::File>,
    pub root: Result<std::path::PathBuf>,
    pub store: Mutex<Option<Store>>,
}
pub fn lock_workspace(root: &std::path::Path) -> Result<std::fs::File> {
    std::fs::create_dir_all(root)?;
    let mut options = std::fs::OpenOptions::new();
    options.create(true).truncate(false).read(true).write(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.share_mode(0);
    }
    options.open(root.join("workspace.lock")).map_err(|_| {
        Failure::new(
            "WORKSPACE_LOCKED",
            "工作空间已由另一栖点窗口使用，请关闭重复应用后重试",
        )
    })
}
async fn run<T: Send + 'static>(
    app: tauri::AppHandle,
    action: impl FnOnce(&mut Store) -> Result<T> + Send + 'static,
) -> Result<T> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<DataState>();
        let mut guard = state
            .store
            .lock()
            .map_err(|_| Failure::new("STATE_UNAVAILABLE", "数据服务不可用，请重新打开应用"))?;
        if guard.is_none() {
            let root = state.root.as_ref().map_err(Clone::clone)?;
            *guard = Some(Store::open(root.clone())?);
        }
        let store = guard.as_mut().expect("initialized store");
        store.recover(&SystemSecrets)?;
        action(store)
    })
    .await
    .map_err(|_| Failure::new("TASK_FAILED", "后台操作失败，请重试"))?
}
#[tauri::command]
pub async fn data_snapshot(app: tauri::AppHandle) -> Result<Snapshot> {
    let handle = app.clone();
    run(app, move |store| {
        let mut snapshot = store.snapshot()?;
        snapshot.runtime = handle
            .state::<Engines>()
            .views(&store.root(), &snapshot.instances);
        Ok(snapshot)
    })
    .await
}
#[tauri::command]
pub async fn fetch_models(
    app: tauri::AppHandle,
    input: crate::model_catalog::CatalogInput,
) -> Result<crate::model_catalog::CatalogResult> {
    crate::model_catalog::endpoint(&input.base_url, &input.protocol)?;
    let key = if let Some(key) = input.api_key.filter(|s| !s.is_empty()) {
        key
    } else {
        let connection_id = input
            .connection_id
            .ok_or_else(|| Failure::new("KEY_REQUIRED", "请先填写 API Key"))?;
        let base = input.base_url.clone();
        run(app, move |store| {
            store.discovery_key(&connection_id, &base, &SystemSecrets)
        })
        .await?
    };
    // Release the data mutex before network IO. Only the model list crosses IPC, never the stored key.
    tauri::async_runtime::spawn_blocking(move || {
        crate::model_catalog::fetch(&input.base_url, &input.protocol, &key)
    })
    .await
    .map_err(|_| Failure::new("TASK_FAILED", "获取模型失败，请重试"))?
}
#[tauri::command]
pub async fn save_instance(app: tauri::AppHandle, input: InstanceInput) -> Result<Snapshot> {
    let handle = app.clone();
    run(app, move |store| {
        if input
            .id
            .as_ref()
            .is_some_and(|id| handle.state::<Engines>().busy(id))
        {
            return Err(Failure::new("INSTANCE_BUSY", "请先停止实例，再修改配置"));
        }
        store.save_instance(input, &SystemSecrets)
    })
    .await
}

#[tauri::command]
pub async fn delete_instance(app: tauri::AppHandle, id: String, expected_revision: u32, operation_id: String) -> Result<Snapshot> {
    let handle = app.clone();
    run(app, move |store| {
        if handle.state::<Engines>().busy(&id) {
            return Err(Failure::new("INSTANCE_BUSY", "请先停止实例并等待当前操作结束，再删除"));
        }
        store.delete_instance(&id, expected_revision, &operation_id)
    }).await
}
#[tauri::command]
pub async fn preview_local_pack(app:tauri::AppHandle,id:String)->Result<crate::local_pack::Preview>{
    let handle=app.clone();
    run(app,move|store|{
        if handle.state::<Engines>().busy(&id){return Err(Failure::new("INSTANCE_BUSY","请先停止工作台并等待当前任务完成，再整理本地资源"));}
        crate::local_pack::preview(&store.root(),&store.instance(&id)?)
    }).await
}
#[tauri::command]
pub async fn save_local_pack(app:tauri::AppHandle,id:String,token:String)->Result<crate::pack_drafts::PackDraft>{
    let handle=app.clone();
    run(app,move|store|{
        if handle.state::<Engines>().busy(&id){return Err(Failure::new("INSTANCE_BUSY","请先停止工作台并等待当前任务完成，再整理本地资源"));}
        crate::local_pack::save(&store.root(),&store.instance(&id)?,&token)
    }).await
}

#[tauri::command]
pub async fn removed_instances(app: tauri::AppHandle) -> Result<Vec<RemovedInstance>> {
    run(app, |store| store.removed_instances()).await
}
#[tauri::command]
pub async fn restore_removed_instance(app:tauri::AppHandle,id:String,operation_id:String)->Result<Snapshot>{
    let handle=app.clone();
    run(app,move|store|{
        if handle.state::<Engines>().busy(&id){return Err(Failure::new("INSTANCE_BUSY","实例仍在执行操作，请稍后恢复"));}
        store.restore_removed_instance(&id,&operation_id)
    }).await
}
#[tauri::command]
pub async fn engine_action(
    app: tauri::AppHandle,
    id: String,
    action: String,
    operation_id: String,
) -> Result<()> {
    if !["install", "start", "stop"].contains(&action.as_str()) {
        return Err(Failure::new("INVALID_ACTION", "不支持的引擎操作"));
    }
    let handle = app.clone();
    let action_copy = action.clone();
    let op = operation_id.clone();
    let prepared = run(app.clone(), move |store| {
        let i = store.instance(&id)?;
        let c = if action_copy == "start" {
            let connection_id = i
                .connection_id
                .as_ref()
                .ok_or_else(|| Failure::new("CONNECTION_REQUIRED", "请先配置模型连接并绑定实例"))?;
            let (c, reference) = store.connection(connection_id)?;
            compatible(&i.engine, &c.protocol)?;
            Some((c, SystemSecrets.get(&reference)?))
        } else {
            None
        };
        if !store.engine_task(&op, &id, &action_copy)? {
            return Ok(None);
        }
        let reserved = if action_copy == "stop" {
            handle.state::<Engines>().begin_stop(&id)
        } else {
            handle.state::<Engines>().begin(&i, &action_copy)
        };
        if let Err(e) = reserved {
            store.finish_engine_task_with_error(&op, Some(&e))?;
            return Err(e);
        }
        Ok(Some((store.root(), i, c)))
    })
    .await?;
    let Some((root, instance, connection)) = prepared else {
        return Ok(());
    };
    tauri::async_runtime::spawn(async move {
        let handle = app.clone();
        let id = instance.id.clone();
        let result = tauri::async_runtime::spawn_blocking(move || {
            let engines = handle.state::<Engines>();
            if action == "stop" {
                engines.stop(&instance.id)
            } else {
                engines.execute(&root, &instance, connection, &action)
            }
        })
        .await
        .unwrap_or_else(|_| Err(Failure::new("ENGINE_TASK", "引擎任务异常中断")));
        if let Err(ref error) = result {
            app.state::<Engines>().error(&id, error.clone());
        }
        let _ = run(app, move |store| {
            store.finish_engine_task_with_error(&operation_id, result.as_ref().err())
        })
        .await;
    });
    Ok(())
}
#[tauri::command]
pub async fn save_connection(app: tauri::AppHandle, input: ConnectionInput) -> Result<Snapshot> {
    run(app, move |store| {
        store.save_connection(input, &SystemSecrets)
    })
    .await
}
#[tauri::command]
pub async fn delete_connection(
    app: tauri::AppHandle,
    id: String,
    operation_id: String,
) -> Result<Snapshot> {
    run(app, move |store| {
        store.delete_connection(&id, &operation_id, &SystemSecrets)
    })
    .await
}
#[tauri::command]
pub async fn pick_project() -> Result<Option<String>> {
    tauri::async_runtime::spawn_blocking(|| {
        rfd::FileDialog::new()
            .set_title("选择项目目录")
            .pick_folder()
            .map(|path| path.to_string_lossy().into_owned())
    })
    .await
    .map_err(|_| Failure::new("PICKER_FAILED", "目录选择器未能打开，可直接输入绝对路径"))
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    #[test]
    fn workspace_lock_excludes_second_owner_and_releases() {
        let root = std::env::temp_dir().join(format!("perch-lock-{}", uuid::Uuid::new_v4()));
        let owner = lock_workspace(&root).unwrap();
        assert!(lock_workspace(&root).is_err());
        drop(owner);
        drop(lock_workspace(&root).unwrap());
        std::fs::remove_dir_all(root).unwrap();
    }
}

#[tauri::command]
pub async fn package_catalog() -> Result<serde_json::Value> {
    Ok(
        serde_json::json!({"extensions":crate::catalog::extensions(),"packs":crate::catalog::packs()}),
    )
}
#[tauri::command]
pub async fn instance_package(app: tauri::AppHandle, id: String) -> Result<serde_json::Value> {
    run(app,move|store|{
        let instance=store.instance(&id)?;
        crate::managed::recover(&store.root(),&id)?;
        let workshop=crate::managed::workshop_inventory(&store.root(),&instance);
        Ok(serde_json::json!({"recipe":crate::managed::recipe(&store.root(),&instance)?,"restorePoints":crate::managed::points(&store.root(),&instance)?,"workshopResources":workshop.as_ref().ok(),"workshopError":workshop.err().map(|error|error.message)}))
    }).await
}
#[tauri::command]
pub async fn change_package(
    app: tauri::AppHandle,
    id: String,
    recipe: crate::catalog::Recipe,
    offline: bool,
    expected_recipe: Option<crate::catalog::Recipe>,
) -> Result<serde_json::Value> {
    recipe.validate()?;
    let handle = app.clone();
    run(app, move |store| {
        let instance = store.instance(&id)?;
        if let Some(expected)=&expected_recipe {if &crate::managed::recipe(&store.root(),&instance)?!=expected{return Err(Failure::new("RECIPE_CHANGED","扩展组合已变化，请刷新后重新确认"));}}

        if handle.state::<Engines>().busy(&id) {
            return Err(Failure::new("INSTANCE_BUSY", "请先停止实例，再更改组合"));
        }
        if offline && !crate::engine::installed(&store.root(), &recipe.engine, &recipe.profile_id) {
            return Err(Failure::new(
                "OFFLINE_ARTIFACT_MISSING",
                "本地尚无此固定引擎工件，联网安装后才能离线使用",
            ));
        }
        if let Some(connection) = &instance.connection_id {
            let (connection, _) = store.connection(connection)?;
            if !recipe.model_protocols.contains(&connection.protocol) {
                return Err(Failure::new(
                    "CONNECTION_INCOMPATIBLE",
                    "当前连接不符合整合包协议要求，请先选择兼容连接",
                ));
            }
        } else {
            return Err(Failure::new(
                "CONNECTION_REQUIRED",
                "请先为实例选择本机模型连接",
            ));
        }
        let point = crate::managed::apply(&store.root(), &instance, &recipe, "更改组合前自动备份")?;
        Ok(serde_json::json!({"recipe":recipe,"backup":point}))
    })
    .await
}
#[tauri::command]
pub async fn pi_package_drift(app:tauri::AppHandle,id:String)->Result<crate::managed::PackageDrift>{
    run(app,move|store|{let instance=store.instance(&id)?;crate::managed::package_drift(&store.root(),&instance)}).await
}
#[tauri::command]
pub async fn save_instance_pack(app:tauri::AppHandle,id:String)->Result<crate::pack_drafts::PackDraft>{
    let handle=app.clone();run(app,move|store|{
        if handle.state::<Engines>().busy(&id){return Err(Failure::new("INSTANCE_BUSY","请先停止实例，再保存完整组合"));}
        let instance=store.instance(&id)?;let root=store.root();let mut recipe=crate::managed::recipe(&root,&instance)?;
        recipe.validate()?;crate::local_skills::validate_selection(&root,&recipe)?;
        crate::managed::validate_shareable_packages(&root,&instance)?;
        crate::managed::capture_public_preset(&root,&instance,&mut recipe)?;
        recipe.workshop=crate::managed::workshop_references(&root,&instance)?;recipe.validate()?;
        for resource in &recipe.workshop {
            let directory=format!("{}/{}",if resource.kind=="presets"{"agent-presets"}else{&resource.kind},resource.id);
            crate::resource_catalog::require_workshop_recipe_host(&root,&recipe,&serde_json::json!({"directory":directory}))
                .map_err(|message|Failure::new("WORKSHOP_HOST",&message))?;
        }
        crate::pack_drafts::write_draft(&root.join("pack-drafts"),None,None,recipe,true).map_err(|message|Failure::new("PACK_SAVE",&message))
    }).await
}
#[tauri::command]
pub async fn install_workshop_asset(app:tauri::AppHandle,id:String,resource_id:String,expected_commit:String)->Result<crate::managed::RestorePoint>{
    let handle=app.clone();
    run(app,move|store|{
        if handle.state::<Engines>().busy(&id){return Err(Failure::new("INSTANCE_BUSY","请先停止实例，再安装工坊资源"));}
        let instance=store.instance(&id)?;let root=store.root();
        if !crate::engine::installed(&root,&instance.engine,&instance.profile_id){return Err(Failure::new("WORKSHOP_HOST","请先安装实例的固定引擎与资源宿主"));}
        let (receipt,content)=crate::resource_catalog::cached_workshop_asset(&root,&resource_id).map_err(|message|Failure::new("WORKSHOP_CACHE",&message))?;
        let plan=&receipt["plan"];
        if plan["commit"].as_str()!=Some(expected_commit.as_str()){return Err(Failure::new("WORKSHOP_CHANGED","来源提交已变化，请重新预览后安装"));}
        crate::resource_catalog::require_workshop_host(&root,&instance,&plan).map_err(|message|Failure::new("WORKSHOP_HOST",&message))?;
        crate::managed::install_workshop_files(&root,&instance,plan["directory"].as_str().ok_or_else(||Failure::new("WORKSHOP_PATH","资源目录无效"))?,&content,&receipt)
    }).await
}
#[tauri::command]
pub async fn adopt_pi_package_selection(app:tauri::AppHandle,id:String,token:String,expected_recipe:crate::catalog::Recipe)->Result<crate::managed::RestorePoint>{
    let handle=app.clone();
    run(app,move|store|{
        if handle.state::<Engines>().busy(&id){return Err(Failure::new("INSTANCE_BUSY","请先停止实例，再采纳工作台包选择"));}
        let instance=store.instance(&id)?;
        crate::managed::adopt_package_selection(&store.root(),&instance,&token,&expected_recipe)
    }).await
}
#[tauri::command]
pub async fn restore_pi_package_selection(app:tauri::AppHandle,id:String,token:String)->Result<crate::managed::RestorePoint>{
    let handle=app.clone();
    run(app,move|store|{
        if handle.state::<Engines>().busy(&id){return Err(Failure::new("INSTANCE_BUSY","请先停止实例，再恢复工作台包选择"));}
        let instance=store.instance(&id)?;
        crate::managed::restore_package_selection(&store.root(),&instance,&token)
    }).await
}
#[tauri::command]
pub async fn restore_package(
    app: tauri::AppHandle,
    id: String,
    point_id: Option<String>,
) -> Result<serde_json::Value> {
    let handle = app.clone();
    run(app, move |store| {
        if handle.state::<Engines>().busy(&id) {
            return Err(Failure::new("INSTANCE_BUSY", "请先停止实例，再备份或恢复"));
        }
        let instance = store.instance(&id)?;
        let point = if let Some(point_id) = point_id {
            let selected=crate::managed::points(&store.root(),&instance)?.into_iter().find(|point|point.id==point_id).ok_or_else(||Failure::new("SNAPSHOT_MISSING","恢复点不存在"))?;
            if selected.recipe.profile_id!=instance.profile_id{
                store.commit_version(&instance.id,instance.revision,&selected.recipe,Some(&point_id),&SystemSecrets)?;selected
            }else{crate::managed::restore(&store.root(), &instance, &point_id)?}
        } else {
            crate::managed::snapshot(&store.root(), &instance, "手动恢复点")?
        };
        Ok(serde_json::to_value(point)?)
    })
    .await
}

fn clone_configuration(
    store: &mut Store,
    id: &str,
    name: String,
    project_path: String,
    secrets: &dyn Secrets,
) -> Result<Snapshot> {
    clone_configuration_version(store,id,name,project_path,None,secrets)
}
fn clone_configuration_version(store:&mut Store,id:&str,name:String,project_path:String,profile_id:Option<String>,secrets:&dyn Secrets)->Result<Snapshot>{
    let original = store.instance(id)?;
    let mut recipe = crate::managed::recipe(&store.root(), &original)?;
    if let Some(profile)=profile_id {recipe.profile_id=profile;}
    recipe.validate()?;
    crate::local_skills::validate_selection(&store.root(),&recipe)?;
    let operation = uuid::Uuid::new_v4().to_string();
    let snapshot = store.save_instance(
        InstanceInput {
            profile_id: Some(recipe.profile_id.clone()),
            operation_id: operation.clone(),
            id: None,
            expected_revision: None,
            name,
            engine: original.engine,
            project_path,
            connection_id: original.connection_id,
        },
        secrets,
    )?;
    let new_id = snapshot
        .tasks
        .iter()
        .find(|task| task.id == operation)
        .map(|task| task.entity_id.clone())
        .ok_or_else(|| {
            Failure::new(
                "CLONE_FAILED",
                "克隆配置已创建，但未找到任务记录，请刷新实例列表",
            )
        })?;
    let new_instance = store.instance(&new_id)?;
    crate::managed::apply(&store.root(), &new_instance, &recipe, "克隆初始组合")?;
    store.snapshot()
}
#[tauri::command]
pub async fn clone_instance(
    app: tauri::AppHandle,
    id: String,
    name: String,
    project_path: String,
) -> Result<Snapshot> {
    let handle = app.clone();
    run(app, move |store| {
        if handle.state::<Engines>().busy(&id) {
            return Err(Failure::new("INSTANCE_BUSY", "请先停止源实例，再克隆配置"));
        }
        clone_configuration(store, &id, name, project_path, &SystemSecrets)
    })
    .await
}

#[tauri::command]
pub async fn clone_version_trial(app:tauri::AppHandle,id:String,name:String,project_path:String,profile_id:String)->Result<Snapshot>{
    run(app,move|store|clone_configuration_version(store,&id,name,project_path,Some(profile_id),&SystemSecrets)).await
}

#[tauri::command]
pub async fn install_package(
    app: tauri::AppHandle,
    id: String,
    recipe: crate::catalog::Recipe,
    offline: bool,
    expected_recipe: Option<crate::catalog::Recipe>,
) -> Result<()> {
    recipe.validate()?;
    let prepare = app.clone();
    let selected = recipe.clone();
    let operation = uuid::Uuid::new_v4().to_string();
    let op = operation.clone();
    let (root, instance) = run(app.clone(), move |store| {
        let instance = store.instance(&id)?;
        if let Some(expected)=&expected_recipe {
            if &crate::managed::recipe(&store.root(),&instance)?!=expected {
                return Err(Failure::new("RECIPE_CHANGED","实例扩展已变化，请重新解析后安装"));
            }
        }
        if instance.engine != selected.engine {
            return Err(Failure::new(
                "RECIPE_ENGINE",
                "请选择与整合包引擎相同的实例",
            ));
        }
        let connection = instance
            .connection_id
            .as_ref()
            .ok_or_else(|| Failure::new("CONNECTION_REQUIRED", "请先选择本机模型连接"))?;
        let (connection, _) = store.connection(connection)?;
        if !selected.model_protocols.contains(&connection.protocol) {
            return Err(Failure::new(
                "CONNECTION_INCOMPATIBLE",
                "实例连接协议不符合此整合包要求",
            ));
        }
        if offline
            && !crate::engine::installed(&store.root(), &selected.engine, &selected.profile_id)
        {
            return Err(Failure::new(
                "OFFLINE_ARTIFACT_MISSING",
                "缺少固定引擎工件，无法离线安装",
            ));
        }
        store.engine_task(&op, &id, "install")?;
        if let Err(error) = prepare.state::<Engines>().begin(&instance, "install") {
            store.finish_engine_task_with_error(&op, Some(&error))?;
            return Err(error);
        }
        if let Err(error) = crate::managed::snapshot(&store.root(), &instance, "安装前自动备份")
        {
            prepare.state::<Engines>().error(&id, error.clone());
            store.finish_engine_task_with_error(&op, Some(&error))?;
            return Err(error);
        }
        Ok((store.root(), instance))
    })
    .await?;
    let worker = app.clone();
    let mut target = instance.clone();
    target.profile_id=recipe.profile_id.clone();
    let id = instance.id.clone();
    let resource_recipe=recipe.clone();
    let installed = tauri::async_runtime::spawn_blocking(move || {
        worker
            .state::<Engines>()
            .execute(&root, &target, None, "prepare")?;
        let engines=worker.state::<Engines>();
        let prepared=crate::resource_catalog::prepare_workshop_recipe_controlled(&root,&resource_recipe,offline,
            ||engines.check_cancelled(&target.id).map_err(|error|error.message),
            |index,total|engines.resource_progress(&target.id,index,total).map_err(|error|error.message));
        // Keep the existing cancellation code instead of reporting it as a download failure.
        engines.check_cancelled(&target.id)?;
        prepared.map_err(|message|Failure::new("WORKSHOP_PREPARE",&message))?;
        Ok(())
    })
    .await
    .unwrap_or_else(|_| {
        Err(Failure::new(
            "INSTALL_FAILED",
            "安装任务异常中断，原组合保留",
        ))
    });
    let result = match installed {
        Err(error) => Err(error),
        Ok(()) => {
            let final_handle = app.clone();
            run(app.clone(), move |store| {
                final_handle.state::<Engines>().begin_commit(&instance.id)?;
                let current = store.instance(&instance.id)?;
                if current.revision != instance.revision {
                    return Err(Failure::new(
                        "REVISION_CONFLICT",
                        "安装期间实例配置已变化，请重新预览组合",
                    ));
                }
                if current.profile_id!=recipe.profile_id{store.commit_version(&current.id,current.revision,&recipe,None,&SystemSecrets)?;}
                else{crate::managed::apply(&store.root(), &current, &recipe, "安装组合前备份")?;}
                Ok(())
            })
            .await
        }
    };
    if let Err(error) = &result {
        app.state::<Engines>().error(&id, error.clone());
    } else {
        app.state::<Engines>().finish_install(&id);
    }
    let failure = result.as_ref().err().cloned();
    run(app, move |store| {
        store.finish_engine_task_with_error(&operation, failure.as_ref())
    })
    .await?;
    result
}

#[tauri::command]
pub async fn artifact_cache(app: tauri::AppHandle, clean: bool) -> Result<serde_json::Value> {
    let handle = app.clone();
    run(app,move|store|{
        let instances:Vec<Instance>=store.snapshot()?.instances.into_iter().map(|v|v.instance).collect();
        if clean {
            if instances.iter().any(|i|handle.state::<Engines>().busy(&i.id)){return Err(Failure::new("CACHE_BUSY","请停止所有实例并等待安装任务结束后再清理"));}
            let removed=crate::artifact_cache::clean(&store.root(),&instances)?;
            Ok(serde_json::json!({"entries":crate::artifact_cache::list(&store.root(),&instances)?,"removed":removed}))
        }else{Ok(serde_json::json!({"entries":crate::artifact_cache::list(&store.root(),&instances)?,"removed":[]}))}
    }).await
}

#[tauri::command]
pub async fn cancel_engine(app: tauri::AppHandle, id: String) -> Result<()> {
    app.state::<Engines>().cancel(&id)
}

#[cfg(test)]
mod package_tests {
    use super::*;
    struct Vault(std::sync::atomic::AtomicUsize);
    impl Secrets for Vault {
        fn put(&self, _reference: &str, _value: &str) -> Result<()> {
            self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Ok(())
        }
        fn delete(&self, _reference: &str) -> Result<()> {
            Ok(())
        }
    }
    #[test]
    fn workshop_creation_defers_files_and_retries_same_instance(){
        let root=std::env::temp_dir().join(format!("perch-create-workshop-{}",uuid::Uuid::new_v4()));
        let project=root.join("项目 空格");std::fs::create_dir_all(&project).unwrap();std::fs::write(project.join("keep"),"source").unwrap();
        let vault=Vault(std::sync::atomic::AtomicUsize::new(0));let mut store=Store::open(root.join("data")).unwrap();
        let connection=store.save_connection(ConnectionInput{operation_id:uuid::Uuid::new_v4().to_string(),id:None,expected_revision:None,name:"shared".into(),provider:"test".into(),protocol:"openai-chat".into(),base_url:"https://example.com/v1".into(),default_model:"test".into(),models:vec![],api_key:Some("fake-test-key".into())},&vault).unwrap().connections.remove(0).id;
        let name="@linxin666/dsh-client-ui-skin-center";
        let mut profile=crate::engine_profile::ResolvedProfile::from(&crate::engine_profile::DSH);profile.id=format!("resolved-{}",uuid::Uuid::new_v4());
        let mut package:serde_json::Value=serde_json::from_str(&profile.package).unwrap();package["dependencies"][name]="1.0.0".into();
        let mut lock:serde_json::Value=serde_json::from_str(&profile.lock).unwrap();lock["packages"][""]["dependencies"]=package["dependencies"].clone();lock["packages"][format!("node_modules/{name}")]=serde_json::json!({"version":"1.0.0","resolved":"https://registry.npmjs.org/fixture/-/fixture-1.0.0.tgz","integrity":"sha512-fixture"});
        profile.package=package.to_string();profile.lock=lock.to_string();crate::engine_profile::register(&store.root(),profile.clone()).unwrap();
        let mut recipe=crate::catalog::packs().remove(0);recipe.profile_id=profile.id.clone();recipe.extensions=vec![crate::catalog::ExtensionRef{id:format!("dsh-npm:{name}"),version:"1.0.0".into(),enabled:true,resource_rules:Default::default(),disabled_resources:vec![]}];
        recipe.workshop=vec![crate::catalog::WorkshopRef{requires:Default::default(),kind:"skins".into(),id:"sample".into(),version:"1".into(),repository:"example/skins".into(),commit:"a".repeat(40),content_path:"skins".into(),files:std::collections::BTreeMap::from([("skin.json".into(),"b".repeat(64))])}];
        let operation=uuid::Uuid::new_v4().to_string();
        let input=||InstanceInput{profile_id:None,operation_id:operation.clone(),id:None,expected_revision:None,name:"new pack".into(),engine:"DSH".into(),project_path:project.to_string_lossy().into(),connection_id:Some(connection.clone())};
        let (instance,pending)=create_pack_target(&mut store,input(),&recipe,&vault).unwrap();
        assert!(pending.is_some());assert!(pending.as_ref().unwrap().workshop.is_empty());
        assert!(!store.root().join("artifacts").exists());
        assert!(crate::managed::workshop_inventory(&store.root(),&instance).unwrap().is_empty());
        let (retry,again)=create_pack_target(&mut store,input(),&recipe,&vault).unwrap();
        assert_eq!(retry.id,instance.id);assert_eq!(again,pending);assert_eq!(store.snapshot().unwrap().instances.len(),1);
        assert_eq!(retry.connection_id,Some(connection));assert_eq!(vault.0.load(std::sync::atomic::Ordering::SeqCst),1);
        assert_eq!(std::fs::read_to_string(project.join("keep")).unwrap(),"source");
        drop(store);std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn clone_reuses_connection_without_copying_key_or_external_workspace() {
        let root = std::env::temp_dir().join(format!("perch-clone-{}", uuid::Uuid::new_v4()));
        let project = root.join("project");
        std::fs::create_dir_all(&project).unwrap();
        std::fs::write(project.join("keep"), "source").unwrap();
        let vault = Vault(std::sync::atomic::AtomicUsize::new(0));
        let mut store = Store::open(root.join("data")).unwrap();
        let snapshot = store
            .save_connection(
                ConnectionInput {
                    operation_id: uuid::Uuid::new_v4().to_string(),
                    id: None,
                    expected_revision: None,
                    name: "shared".into(),
                    provider: "test".into(),
                    protocol: "openai-chat".into(),
                    base_url: "https://example.com/v1".into(),
                    default_model: "test".into(),
                    models: vec![],
                    api_key: Some("fake-clone-key".into()),
                },
                &vault,
            )
            .unwrap();
        let connection = snapshot.connections[0].id.clone();
        let snapshot = store
            .save_instance(
                InstanceInput {
            profile_id: None,
                    operation_id: uuid::Uuid::new_v4().to_string(),
                    id: None,
                    expected_revision: None,
                    name: "original".into(),
                    engine: "Pi".into(),
                    project_path: project.to_string_lossy().into(),
                    connection_id: Some(connection.clone()),
                },
                &vault,
            )
            .unwrap();
        let original = snapshot.instances[0].instance.clone();
        let selected = crate::catalog::packs().remove(1);
        crate::managed::apply(&store.root(), &original, &selected, "source").unwrap();
        let result = clone_configuration(
            &mut store,
            &original.id,
            "copy".into(),
            project.to_string_lossy().into(),
            &vault,
        )
        .unwrap();
        let cloned = result
            .instances
            .iter()
            .find(|i| i.instance.name == "copy")
            .unwrap();
        assert_ne!(cloned.instance.id, original.id);
        assert_eq!(cloned.instance.connection_id, Some(connection));
        assert_eq!(vault.0.load(std::sync::atomic::Ordering::SeqCst), 1);
        assert_eq!(
            crate::managed::recipe(&store.root(), &cloned.instance).unwrap(),
            selected
        );
        assert_eq!(
            std::fs::read_to_string(project.join("keep")).unwrap(),
            "source"
        );
        assert!(!serde_json::to_string(&cloned.instance)
            .unwrap()
            .contains("fake-clone-key"));
        let mut trial_profile:crate::engine_profile::ResolvedProfile=(&crate::engine_profile::PI).into();
        trial_profile.id=format!("resolved-{}",uuid::Uuid::new_v4());
        let trial_id=trial_profile.id.clone();crate::engine_profile::register(&store.root(),trial_profile).unwrap();
        let trial=clone_configuration_version(&mut store,&original.id,"trial".into(),project.to_string_lossy().into(),Some(trial_id.clone()),&vault).unwrap();
        let trial_instance=&trial.instances.iter().find(|item|item.instance.name=="trial").unwrap().instance;
        assert_eq!(trial_instance.profile_id,trial_id);assert_eq!(trial_instance.connection_id,original.connection_id);
        assert_eq!(store.instance(&original.id).unwrap().profile_id,original.profile_id);
        assert_eq!(crate::managed::recipe(&store.root(),&original).unwrap(),selected);
        assert_eq!(vault.0.load(std::sync::atomic::Ordering::SeqCst),1);
        let count=trial.instances.len();
        assert!(clone_configuration_version(&mut store,&original.id,"invalid".into(),project.to_string_lossy().into(),Some("resolved-missing".into()),&vault).is_err());
        assert_eq!(store.snapshot().unwrap().instances.len(),count);
        let point = crate::managed::snapshot(&store.root(), &original, "restart recovery").unwrap();
        let base = store.root().join("instances").join(&original.id);
        std::fs::rename(
            base.join("managed"),
            base.join("restore-points").join(&point.id).join("previous"),
        )
        .unwrap();
        std::fs::create_dir_all(base.join("managed-next")).unwrap();
        std::fs::write(
            base.join("managed-transaction.json"),
            serde_json::to_vec(&serde_json::json!({"snapshot":point.id,"had_previous":true}))
                .unwrap(),
        )
        .unwrap();
        drop(store);
        let reopened = Store::open(root.join("data")).unwrap();
        assert_eq!(
            crate::managed::recipe(&reopened.root(), &original).unwrap(),
            selected
        );
        assert!(!base.join("managed-transaction.json").exists());
        assert_eq!(
            std::fs::read_to_string(project.join("keep")).unwrap(),
            "source"
        );
        drop(reopened);
        std::fs::remove_dir_all(root).unwrap();
    }
}

#[tauri::command]
pub async fn export_diagnostics(app: tauri::AppHandle) -> Result<Option<String>> {
    let snapshot = data_snapshot(app).await?;
    let report = diagnostic_report(&snapshot);
    tauri::async_runtime::spawn_blocking(move || {
        let Some(path) = rfd::FileDialog::new()
            .set_title("导出诊断报告")
            .set_file_name("perch-diagnostics.json")
            .add_filter("JSON", &["json"])
            .save_file()
        else {
            return Ok(None);
        };
        std::fs::write(&path, serde_json::to_vec_pretty(&report)?)?;
        Ok(Some(path.to_string_lossy().into_owned()))
    })
    .await
    .map_err(|_| Failure::new("EXPORT_FAILED", "诊断报告导出失败，请重试"))?
}

fn diagnostic_report(snapshot: &Snapshot) -> serde_json::Value {
    serde_json::json!({
        "schemaVersion":1, "appVersion":env!("CARGO_PKG_VERSION"),
        "os":std::env::consts::OS, "arch":std::env::consts::ARCH,
        "instances":snapshot.instances.iter().map(|i|{
            let runtime=snapshot.runtime.iter().find(|r|r.id==i.instance.id);
            serde_json::json!({"id":i.instance.id,"engine":i.instance.engine,"profile":i.instance.profile_id,
                "revision":i.instance.revision,"projectExists":i.project_exists,
                "status":runtime.map(|r|&r.status),"errorCode":runtime.and_then(|r|r.error.as_ref()).map(|e|&e.code),
                "logMarkers":runtime.map(|r|r.logs.iter().filter_map(|line| {
                    let marker=line.split(|c:char| !c.is_ascii_uppercase() && c!='_').next()?;
                    (marker.starts_with("PERCH_") && marker.len()<80).then_some(marker)
                }).collect::<Vec<_>>())})
        }).collect::<Vec<_>>(),
        "tasks":snapshot.tasks,
        "excluded":"No names, project paths, model URLs, keys, configuration files or conversation text. Logs include Perch event markers only."
    })
}

#[cfg(test)]
mod diagnostic_tests {
    use super::*;
    #[test]
    fn report_excludes_private_details_and_raw_logs() {
        let instance: Instance=serde_json::from_value(serde_json::json!({"schemaVersion":1,"id":"test","name":"PRIVATE_NAME","engine":"Pi","projectPath":"PRIVATE_PATH","connectionId":null,"profileId":"pi-web-0.9.3-pi-0.87.1","revision":1,"createdAt":0,"updatedAt":0})).unwrap();
        let snapshot = Snapshot {
            mode: "desktop",
            instances: vec![InstanceView {
                instance,
                status: "running",
                project_exists: true,
            }],
            connections: vec![],
            tasks: vec![],
            data_root: "PRIVATE_ROOT".into(),
            runtime: vec![crate::engine::RuntimeView {
                id: "test".into(),
                logs: vec![
                    "PERCH_BOOT_FAILED: PRIVATE_KEY".into(),
                    "private conversation".into(),
                ]
                .into(),
                ..Default::default()
            }],
        };
        let text = diagnostic_report(&snapshot).to_string();
        assert!(text.contains("PERCH_BOOT_FAILED"));
        for private in [
            "PRIVATE_NAME",
            "PRIVATE_PATH",
            "PRIVATE_ROOT",
            "PRIVATE_KEY",
            "private conversation",
        ] {
            assert!(!text.contains(private));
        }
    }
}

#[tauri::command]
pub async fn create_pack_instance(app:tauri::AppHandle,input:InstanceInput,recipe:crate::catalog::Recipe)->Result<Snapshot>{
    let selected=recipe.clone();
    let handle=app.clone();
    let (instance,pending)=run(app.clone(),move|store|{
        if let Some(task)=store.snapshot()?.tasks.iter().find(|task|task.id==input.operation_id){
            if handle.state::<Engines>().busy(&task.entity_id){return Err(Failure::new("INSTANCE_BUSY","此实例已有任务，请等待完成后重试"));}
        }
        create_pack_target(store,input,&selected,&SystemSecrets)
    }).await?;
    if let Some(expected)=pending {
        if let Err(error)=install_package(app.clone(),instance.id.clone(),recipe,false,Some(expected)).await {
            return Err(Failure::new(&error.code,&format!("实例“{}”已创建，组合尚未安装完成：{}。保留当前表单重试会继续使用该实例；也可在我的实例中查看任务。",instance.name,error.message)));
        }
    }
    data_snapshot(app).await
}
/// Returns a pending installation instead of applying resource files before their host exists.
fn create_pack_target(store:&mut Store,mut input:InstanceInput,recipe:&crate::catalog::Recipe,secrets:&dyn Secrets)->Result<(Instance,Option<crate::catalog::Recipe>)>{
    recipe.validate()?;
    recipe.validate_workshop_hosts()?;
    if input.id.is_some()||input.engine!=recipe.engine{return Err(Failure::new("PACK_TARGET","请为此组合创建同引擎的新实例"));}
    input.profile_id=Some(recipe.profile_id.clone());
        crate::local_skills::validate_selection(&store.root(),&recipe)?;
        let connection=input.connection_id.as_ref().ok_or_else(||Failure::new("CONNECTION_REQUIRED","请选择已有模型连接，或先添加一次连接"))?;
        let (connection,_)=store.connection(connection)?;
        if !recipe.model_protocols.contains(&connection.protocol){return Err(Failure::new("CONNECTION_INCOMPATIBLE","模型连接不符合此组合要求"));}
        let operation=input.operation_id.clone();
        let snapshot=store.save_instance(input,secrets)?;
        let id=snapshot.tasks.iter().find(|task|task.id==operation).map(|task|task.entity_id.clone()).ok_or_else(||Failure::new("PACK_CREATE","无法定位刚创建的实例"))?;
        let instance=store.instance(&id)?;
        let current=crate::managed::recipe(&store.root(),&instance)?;
        if &current!=recipe {
            if !recipe.workshop.is_empty(){return Ok((instance,Some(current)));}
            crate::managed::apply(&store.root(),&instance,recipe,"应用创建时选择的整合包").map_err(|_|Failure::new("PACK_APPLY",&format!("实例已创建，但组合未应用。请重试本次保存，或在我的实例中查看 {}；已有实例未改动",instance.name)))?;
        }
        Ok((instance,None))
}
