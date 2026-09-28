use crate::credentials::Secrets;
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};
use uuid::Uuid;

pub type Result<T> = std::result::Result<T, Failure>;
#[derive(Debug, Clone, Serialize)]
pub struct Failure {
    pub code: String,
    pub message: String,
}
impl Failure {
    pub fn new(code: &str, message: &str) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
        }
    }
}
impl From<rusqlite::Error> for Failure {
    fn from(_: rusqlite::Error) -> Self {
        Self::new("DATABASE", "数据读取或写入失败，请重试；原数据已保留")
    }
}
impl From<std::io::Error> for Failure {
    fn from(_: std::io::Error) -> Self {
        Self::new(
            "FILESYSTEM",
            "无法写入实例文件，请检查磁盘空间与目录权限后重试",
        )
    }
}
impl From<serde_json::Error> for Failure {
    fn from(_: serde_json::Error) -> Self {
        Self::new("INVALID_DOCUMENT", "数据格式不受支持，请检查文件版本和字段")
    }
}
fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
fn id() -> String {
    Uuid::new_v4().to_string()
}
fn uuid(value: &str) -> Result<()> {
    Uuid::parse_str(value)
        .map(|_| ())
        .map_err(|_| Failure::new("INVALID_ID", "标识不正确"))
}
fn label(value: &str, max: usize) -> Result<()> {
    if value.trim().is_empty() || value.chars().count() > max || value.chars().any(char::is_control)
    {
        return Err(Failure::new(
            "INVALID_FIELD",
            "名称或模型字段为空、过长或包含控制字符",
        ));
    }
    Ok(())
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Instance {
    pub schema_version: u32,
    pub id: String,
    pub name: String,
    pub engine: String,
    pub project_path: String,
    pub connection_id: Option<String>,
    pub profile_id: String,
    pub revision: u32,
    pub created_at: u64,
    pub updated_at: u64,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ModelConnection {
    pub id: String,
    pub name: String,
    pub provider: String,
    pub protocol: String,
    pub base_url: String,
    pub default_model: String,
    #[serde(default)]
    pub models: Vec<ModelSpec>,
    pub has_key: bool,
    pub revision: u32,
}
#[derive(Clone, Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ModelSpec {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub context_window: Option<u32>,
    #[serde(default)]
    pub max_tokens: Option<u32>,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InstanceView {
    #[serde(flatten)]
    pub instance: Instance,
    pub status: &'static str,
    pub project_exists: bool,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RemovedInstance {
    pub instance: Instance,
    pub removed_at: u64,
    pub connection_available: bool,
    pub data_exists: bool,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Task {
    pub failure_code: Option<String>,
    pub id: String,
    pub kind: String,
    pub entity_id: String,
    pub state: String,
    pub created_at: u64,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    pub runtime: Vec<crate::engine::RuntimeView>,
    pub mode: &'static str,
    pub instances: Vec<InstanceView>,
    pub connections: Vec<ModelConnection>,
    pub tasks: Vec<Task>,
    pub data_root: String,
}
#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct InstanceInput {
    #[serde(default)]
    pub profile_id: Option<String>,
    pub operation_id: String,
    pub id: Option<String>,
    pub expected_revision: Option<u32>,
    pub name: String,
    pub engine: String,
    pub project_path: String,
    pub connection_id: Option<String>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ConnectionInput {
    pub operation_id: String,
    pub id: Option<String>,
    pub expected_revision: Option<u32>,
    pub name: String,
    pub provider: String,
    pub protocol: String,
    pub base_url: String,
    pub default_model: String,
    #[serde(default)]
    pub models: Vec<ModelSpec>,
    pub api_key: Option<String>,
}

pub struct Store {
    db: Connection,
    root: PathBuf,
}
impl Store {
    pub fn root(&self) -> PathBuf {
        self.root.clone()
    }
    pub fn engine_task(&self, operation: &str, instance: &str, action: &str) -> Result<bool> {
        uuid(operation)?;
        let request = serde_json::to_string(&serde_json::json!({"id":instance,"action":action}))?;
        if self.duplicate(operation, &request)? {
            return Ok(false);
        }
        self.db.execute(
            "INSERT INTO tasks VALUES(?1,?2,?3,'pending',?4,?5)",
            params![
                operation,
                format!("engine_{action}"),
                instance,
                now(),
                request
            ],
        )?;
        Ok(true)
    }
    pub fn finish_engine_task_with_error(&self, operation:&str, error:Option<&Failure>)->Result<()> {
        let transaction=self.db.unchecked_transaction()?;
        transaction.execute(
            "UPDATE tasks SET state=?2 WHERE id=?1",
            params![operation, if error.is_none() { "completed" } else { "failed" }],
        )?;
        transaction.execute("DELETE FROM task_failures WHERE task_id=?1",[operation])?;
        if let Some(error)=error {
            let allowed=["CANCELLED","INSTANCE_BUSY","ALREADY_RUNNING","PROJECT_MISSING","INSTALL_TIMEOUT","INSTALL_INTEGRITY","START_FAILED","PROCESS_EXIT","PI_WORKSPACE","REVISION_CONFLICT","WORKSHOP_PREPARE","ENGINE_TASK","OFFLINE_ARTIFACT_MISSING"];
            let code=if allowed.contains(&error.code.as_str()){error.code.as_str()}else{"ENGINE_FAILED"};
            transaction.execute("INSERT INTO task_failures(task_id,code) VALUES(?1,?2)",params![operation,code])?;
        }
        transaction.commit()?;
        Ok(())
    }
    pub fn discovery_key(
        &self,
        connection_id: &str,
        base_url: &str,
        secrets: &dyn Secrets,
    ) -> Result<String> {
        let (connection, reference) = self.connection(connection_id)?;
        let original = crate::model_catalog::validate_base_url(&connection.base_url)?;
        let requested = crate::model_catalog::validate_base_url(base_url)?;
        if original.origin() != requested.origin() {
            return Err(Failure::new(
                "KEY_ORIGIN_CHANGED",
                "API 地址已更换服务商，请重新填写对应 Key 后获取模型",
            ));
        }
        secrets.get(&reference)
    }
    pub fn open(root: PathBuf) -> Result<Self> {
        fs::create_dir_all(&root)?;
        let db = Connection::open(root.join("state.sqlite"))?;
        db.busy_timeout(std::time::Duration::from_secs(5))?;
        db.execute_batch("PRAGMA foreign_keys=ON; PRAGMA journal_mode=WAL;")?;
        let version: u32 = db.query_row("PRAGMA user_version", [], |r| r.get(0))?;
        if version > 1 {
            return Err(Failure::new(
                "NEWER_SCHEMA",
                "数据由较新版本创建，请升级应用后再打开",
            ));
        }
        if version == 0 {
            db.execute_batch("BEGIN IMMEDIATE;
                CREATE TABLE connections(id TEXT PRIMARY KEY, document TEXT NOT NULL, secret_ref TEXT NOT NULL);
                CREATE TABLE instances(id TEXT PRIMARY KEY, document TEXT NOT NULL, connection_id TEXT REFERENCES connections(id));
                CREATE TABLE tasks(id TEXT PRIMARY KEY, kind TEXT NOT NULL, entity_id TEXT NOT NULL, state TEXT NOT NULL, created_at INTEGER NOT NULL, request TEXT NOT NULL);
                CREATE TABLE journal(task_id TEXT PRIMARY KEY REFERENCES tasks(id), instance_id TEXT NOT NULL);
                CREATE TABLE credential_cleanup(secret_ref TEXT PRIMARY KEY);
                PRAGMA user_version=1; COMMIT;")?;
        }
        db.execute_batch("CREATE TABLE IF NOT EXISTS task_failures(task_id TEXT PRIMARY KEY REFERENCES tasks(id) ON DELETE CASCADE, code TEXT NOT NULL);")?;
        db.execute(
            "UPDATE tasks SET state='interrupted' WHERE state='pending' AND kind LIKE 'engine_%'",
            [],
        )?;
        {
            let mut query = db.prepare("SELECT id FROM instances")?;
            let ids = query.query_map([], |row| row.get::<_, String>(0))?;
            for id in ids {
                crate::managed::recover(&root, &id?)?;
            }
        }
        Ok(Self { db, root })
    }
    pub fn instance(&self, value: &str) -> Result<Instance> {
        uuid(value)?;
        let text: Option<String> = self
            .db
            .query_row("SELECT document FROM instances WHERE id=?1", [value], |r| {
                r.get(0)
            })
            .optional()?;
        serde_json::from_str(
            &text.ok_or_else(|| Failure::new("NOT_FOUND", "实例不存在，请刷新列表"))?,
        )
        .map_err(Into::into)
    }
    pub fn connection(&self, value: &str) -> Result<(ModelConnection, String)> {
        uuid(value)?;
        let row: Option<(String, String)> = self
            .db
            .query_row(
                "SELECT document, secret_ref FROM connections WHERE id=?1",
                [value],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        let (text, reference) =
            row.ok_or_else(|| Failure::new("CONNECTION_MISSING", "模型连接不存在，请重新选择"))?;
        Ok((serde_json::from_str(&text)?, reference))
    }
    fn duplicate(&self, operation: &str, request: &str) -> Result<bool> {
        uuid(operation)?;
        let existing: Option<String> = self
            .db
            .query_row("SELECT request FROM tasks WHERE id=?1", [operation], |r| {
                r.get(0)
            })
            .optional()?;
        if let Some(previous) = existing {
            if previous != request {
                return Err(Failure::new(
                    "OPERATION_CONFLICT",
                    "操作标识已被其他请求使用，请刷新后重试",
                ));
            }
            return Ok(true);
        }
        Ok(false)
    }
    pub fn recover(&mut self, secrets: &dyn Secrets) -> Result<()> {
        let pending: Vec<(String, String)> = self
            .db
            .prepare("SELECT task_id, instance_id FROM journal")?
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<std::result::Result<_, _>>()?;
        for (task, instance_id) in pending {
            let instance = self.instance(&instance_id)?;
            let (kind,request):(String,String)=self.db.query_row("SELECT kind,request FROM tasks WHERE id=?1",[&task],|row|Ok((row.get(0)?,row.get(1)?)))?;
            if kind=="switch_version" {
                let request:serde_json::Value=serde_json::from_str(&request)?;
                let recipe:crate::catalog::Recipe=serde_json::from_value(request["recipe"].clone())?;
                if let Some(point)=request["restorePoint"].as_str(){crate::managed::restore(&self.root,&instance,point)?;}
                else if crate::managed::recipe(&self.root,&instance)?!=recipe{crate::managed::apply(&self.root,&instance,&recipe,"版本切换前恢复点")?;}
            }
            self.materialize(&instance)?;
            let tx = self.db.transaction()?;
            tx.execute("UPDATE tasks SET state='completed' WHERE id=?1", [&task])?;
            tx.execute("DELETE FROM journal WHERE task_id=?1", [&task])?;
            tx.commit()?;
        }
        let refs: Vec<String> = self
            .db
            .prepare("SELECT secret_ref FROM credential_cleanup")?
            .query_map([], |r| r.get(0))?
            .collect::<std::result::Result<_, _>>()?;
        for reference in refs {
            let used: bool = self.db.query_row(
                "SELECT EXISTS(SELECT 1 FROM connections WHERE secret_ref=?1)",
                [&reference],
                |r| r.get(0),
            )?;
            if !used {
                secrets.delete(&reference)?;
            }
            self.db.execute(
                "DELETE FROM credential_cleanup WHERE secret_ref=?1",
                [&reference],
            )?;
        }
        Ok(())
    }
    fn materialize(&self, instance: &Instance) -> Result<()> {
        uuid(&instance.id)?;
        let folder = self.root.join("instances").join(&instance.id);
        fs::create_dir_all(&folder)?;
        for name in ["config", "agent-data", "logs"] {
            fs::create_dir_all(folder.join(name))?;
        }
        let profile: serde_json::Value = serde_json::from_str(
            if instance.engine == "Pi" {
                include_str!("../../../docs/profiles/pi/profile.json")
            } else {
                include_str!("../../../docs/profiles/dsh/profile.json")
            }
            .trim_start_matches('\u{feff}'),
        )?;
        let profile = if instance.profile_id.starts_with("resolved-") {
            let resolved=crate::engine_profile::resolve(&instance.engine,&instance.profile_id)?;
            serde_json::json!({"id":resolved.id,"engine":resolved.engine,"runtime":{"node":resolved.node,"npm":resolved.npm},"entry":resolved.entry,"compatibility":"unverified"})
        } else {profile};
        let lock = serde_json::json!({"schemaVersion":1,"profileId":instance.profile_id,"installed":crate::engine::installed(&self.root, &instance.engine, &instance.profile_id),"profile":profile});
        for (name, data) in [
            ("manifest.json", serde_json::to_vec_pretty(instance)?),
            ("lock.json", serde_json::to_vec_pretty(&lock)?),
        ] {
            let temporary = folder.join(format!("{name}.pending"));
            use std::io::Write;
            let mut file = fs::File::create(&temporary)?;
            file.write_all(&data)?;
            file.sync_all()?;
            drop(file);
            fs::rename(temporary, folder.join(name))?;
        }
        Ok(())
    }
    pub fn snapshot(&self) -> Result<Snapshot> {
        let docs: Vec<String> = self
            .db
            .prepare("SELECT document FROM instances ORDER BY rowid")?
            .query_map([], |r| r.get(0))?
            .collect::<std::result::Result<_, _>>()?;
        let mut instances = Vec::new();
        for text in docs {
            let instance: Instance = serde_json::from_str(&text)?;
            let project_exists = Path::new(&instance.project_path).is_dir();
            instances.push(InstanceView {
                instance,
                status: "not_installed",
                project_exists,
            });
        }
        let docs: Vec<String> = self
            .db
            .prepare("SELECT document FROM connections ORDER BY rowid")?
            .query_map([], |r| r.get(0))?
            .collect::<std::result::Result<_, _>>()?;
        let connections = docs
            .iter()
            .map(|s| serde_json::from_str(s).map_err(Failure::from))
            .collect::<Result<Vec<ModelConnection>>>()?;
        let tasks = self.db.prepare("SELECT t.id,t.kind,t.entity_id,t.state,t.created_at,f.code FROM tasks t LEFT JOIN task_failures f ON f.task_id=t.id ORDER BY t.rowid DESC LIMIT 100")?.query_map([], |r| Ok(Task {id:r.get(0)?,kind:r.get(1)?,entity_id:r.get(2)?,state:r.get(3)?,created_at:r.get(4)?,failure_code:r.get(5)?}))?.collect::<std::result::Result<_,_>>()?;
        Ok(Snapshot {
            runtime: vec![],
            mode: "desktop",
            instances,
            connections,
            tasks,
            data_root: self.root.to_string_lossy().into(),
        })
    }
    pub fn save_instance(
        &mut self,
        input: InstanceInput,
        secrets: &dyn Secrets,
    ) -> Result<Snapshot> {
        self.recover(secrets)?;
        let request = serde_json::to_string(&input)?;
        if self.duplicate(&input.operation_id, &request)? {
            return self.snapshot();
        }
        label(&input.name, 60)?;
        if !["Pi", "DSH"].contains(&input.engine.as_str()) {
            return Err(Failure::new("INVALID_ENGINE", "请选择 Pi 或 DSH"));
        }
        let project = PathBuf::from(input.project_path.trim());
        if !project.is_absolute() || !project.is_dir() {
            return Err(Failure::new(
                "PROJECT_MISSING",
                "项目目录不存在或不是绝对路径，请重新选择",
            ));
        }
        let old = input.id.as_deref().map(|v| self.instance(v)).transpose()?;
        if let Some(ref previous) = old {
            if Some(previous.revision) != input.expected_revision {
                return Err(Failure::new(
                    "REVISION_CONFLICT",
                    "实例已更新，请刷新后重新修改",
                ));
            }
            if previous.engine != input.engine {
                return Err(Failure::new(
                    "ENGINE_CHANGE",
                    "现有实例不能切换引擎，请新建实例",
                ));
            }
        }
        if let Some(ref connection_id) = input.connection_id {
            let (connection, _) = self.connection(connection_id)?;
            compatible(&input.engine, &connection.protocol)?;
        }
        let profile_id=old.as_ref().map(|v|v.profile_id.clone()).or(input.profile_id.clone()).unwrap_or_else(||if input.engine=="Pi"{crate::engine_profile::PI.id.into()}else{crate::engine_profile::DSH.id.into()});
        crate::engine_profile::resolve(&input.engine,&profile_id)?;
        if old.is_some() && input.profile_id.as_ref().is_some_and(|p|p!=&profile_id){return Err(Failure::new("VERSION_CHANGE","请使用组合变更流程切换现有实例版本"));}
        let instance = Instance {
            schema_version: 1,
            id: old.as_ref().map(|v| v.id.clone()).unwrap_or_else(id),
            name: input.name.trim().into(),
            engine: input.engine.clone(),
            project_path: project.to_string_lossy().into(),
            connection_id: input.connection_id,
            profile_id,
            revision: old.as_ref().map_or(1, |v| v.revision + 1),
            created_at: old.as_ref().map_or_else(now, |v| v.created_at),
            updated_at: now(),
        };
        let tx = self.db.transaction()?;
        tx.execute("INSERT INTO instances VALUES(?1,?2,?3) ON CONFLICT(id) DO UPDATE SET document=excluded.document,connection_id=excluded.connection_id", params![instance.id,serde_json::to_string(&instance)?,instance.connection_id])?;
        tx.execute(
            "INSERT INTO tasks VALUES(?1,?2,?3,'pending',?4,?5)",
            params![
                input.operation_id,
                if old.is_some() {
                    "update_instance"
                } else {
                    "create_instance"
                },
                instance.id,
                now(),
                request
            ],
        )?;
        tx.execute(
            "INSERT INTO journal VALUES(?1,?2)",
            params![input.operation_id, instance.id],
        )?;
        tx.commit()?;
        self.recover(secrets)?;
        self.snapshot()
    }
    pub fn commit_version(&mut self,id:&str,expected_revision:u32,recipe:&crate::catalog::Recipe,restore_point:Option<&str>,secrets:&dyn Secrets)->Result<()> {
        self.recover(secrets)?;
        let mut instance=self.instance(id)?;
        if instance.revision!=expected_revision{return Err(Failure::new("REVISION_CONFLICT","实例配置已变化，请重新预览版本"));}
        recipe.validate()?;crate::local_skills::validate_selection(&self.root,recipe)?;
        if recipe.engine!=instance.engine{return Err(Failure::new("ENGINE_CHANGE","不能就地切换引擎，请新建实例"));}
        if !crate::engine::installed(&self.root,&recipe.engine,&recipe.profile_id){return Err(Failure::new("VERSION_NOT_INSTALLED","目标版本尚未安装，原版本保留"));}
        if let Some(connection)=&instance.connection_id {if !recipe.model_protocols.contains(&self.connection(connection)?.0.protocol){return Err(Failure::new("CONNECTION_INCOMPATIBLE","模型连接不符合目标版本组合要求"));}}
        if let Some(point)=restore_point {
            let saved=crate::managed::validate_restore(&self.root,&instance,point)?;
            if &saved.recipe!=recipe{return Err(Failure::new("SNAPSHOT_CHANGED","恢复点与目标组合不一致"));}
        }
        crate::managed::snapshot(&self.root,&instance,"切换版本前备份")?;
        instance.profile_id=recipe.profile_id.clone();instance.revision+=1;instance.updated_at=now();
        let operation=Uuid::new_v4().to_string();let request=serde_json::json!({"recipe":recipe,"restorePoint":restore_point}).to_string();
        let tx=self.db.transaction()?;
        tx.execute("UPDATE instances SET document=?1 WHERE id=?2",params![serde_json::to_string(&instance)?,id])?;
        tx.execute("INSERT INTO tasks VALUES(?1,'switch_version',?2,'pending',?3,?4)",params![operation,id,now(),request])?;
        tx.execute("INSERT INTO journal VALUES(?1,?2)",params![operation,id])?;tx.commit()?;
        self.recover(secrets)
    }
    pub fn save_connection(
        &mut self,
        input: ConnectionInput,
        secrets: &dyn Secrets,
    ) -> Result<Snapshot> {
        self.recover(secrets)?;
        // No key (or key hash) goes into task metadata, manifests, or SQLite.
        let request = serde_json::json!([
            "connection",
            input.id,
            input.expected_revision,
            input.name,
            input.provider,
            input.protocol,
            input.base_url,
            input.default_model,
            input.models
        ])
        .to_string();
        if self.duplicate(&input.operation_id, &request)? {
            return self.snapshot();
        }
        label(&input.name, 60)?;
        label(&input.provider, 60)?;
        label(&input.default_model, 512)?;
        if !["deepseek", "openai-chat", "openai-responses", "anthropic"]
            .contains(&input.protocol.as_str())
        {
            return Err(Failure::new("INVALID_PROTOCOL", "接口协议不受支持"));
        }
        let url = crate::model_catalog::validate_base_url(&input.base_url)?;
        let old = input
            .id
            .as_deref()
            .map(|v| self.connection(v))
            .transpose()?;
        if let Some((ref connection, _)) = old {
            if Some(connection.revision) != input.expected_revision {
                return Err(Failure::new(
                    "REVISION_CONFLICT",
                    "连接已更新，请刷新后重试",
                ));
            }
            if connection.protocol != input.protocol {
                let mut statement = self
                    .db
                    .prepare("SELECT document FROM instances WHERE connection_id=?1")?;
                let docs = statement.query_map([&connection.id], |r| r.get::<_, String>(0))?;
                for text in docs {
                    let instance: Instance = serde_json::from_str(&text?)?;
                    compatible(&instance.engine, &input.protocol)?;
                }
            }
            let previous = crate::model_catalog::validate_base_url(&connection.base_url)?;
            if previous.origin() != url.origin()
                && input.api_key.as_deref().is_none_or(str::is_empty)
            {
                return Err(Failure::new(
                    "KEY_ORIGIN_CHANGED",
                    "API 地址已更换服务商，请填写对应 Key，避免沿用原服务商密钥",
                ));
            }
        }
        let mut models = input.models;
        if models.is_empty() {
            models.push(ModelSpec {
                id: input.default_model.trim().into(),
                name: input.default_model.trim().into(),
                context_window: None,
                max_tokens: None,
            });
        }
        validate_models(&mut models, input.default_model.trim())?;
        let key = input.api_key.as_deref().filter(|k| !k.is_empty());
        if old.is_none() && key.is_none() {
            return Err(Failure::new("KEY_REQUIRED", "请填写 API Key"));
        }
        if key.is_some_and(|k| k.len() > 2400 || k.trim() != k || k.chars().any(char::is_control)) {
            return Err(Failure::new(
                "INVALID_KEY",
                "API Key 过长或包含空白/控制字符，请检查",
            ));
        }
        let reference = if key.is_some() {
            id()
        } else {
            old.as_ref().unwrap().1.clone()
        };
        if let Some(key) = key {
            // Durable orphan intent before touching the credential store; replay can remove it after a crash.
            self.db
                .execute("INSERT INTO credential_cleanup VALUES(?1)", [&reference])?;
            secrets.put(&reference, key)?;
        }
        let connection = ModelConnection {
            id: old.as_ref().map(|v| v.0.id.clone()).unwrap_or_else(id),
            name: input.name.trim().into(),
            provider: input.provider.trim().into(),
            protocol: input.protocol,
            base_url: url.as_str().trim_end_matches('/').into(),
            default_model: input.default_model.trim().into(),
            models,
            has_key: true,
            revision: old.as_ref().map_or(1, |v| v.0.revision + 1),
        };
        let tx = self.db.transaction()?;
        tx.execute("INSERT INTO connections VALUES(?1,?2,?3) ON CONFLICT(id) DO UPDATE SET document=excluded.document,secret_ref=excluded.secret_ref",params![connection.id,serde_json::to_string(&connection)?,reference])?;
        tx.execute(
            "DELETE FROM credential_cleanup WHERE secret_ref=?1",
            [&reference],
        )?;
        if let Some((_, previous)) = old {
            if previous != reference {
                tx.execute(
                    "INSERT OR IGNORE INTO credential_cleanup VALUES(?1)",
                    [previous],
                )?;
            }
        }
        tx.execute(
            "INSERT INTO tasks VALUES(?1,'save_connection',?2,'completed',?3,?4)",
            params![input.operation_id, connection.id, now(), request],
        )?;
        tx.commit()?;
        self.recover(secrets)?;
        self.snapshot()
    }
    /// Remove the active record atomically, retaining instance-owned files and
    /// the original manifest for recovery. Never traverse the project directory.
    pub fn delete_instance(&mut self, instance_id: &str, expected_revision: u32, operation_id: &str) -> Result<Snapshot> {
        let request = serde_json::json!(["delete_instance", instance_id, expected_revision]).to_string();
        if self.duplicate(operation_id, &request)? { return self.snapshot(); }
        let instance = self.instance(instance_id)?;
        if instance.revision != expected_revision {
            return Err(Failure::new("REVISION_CONFLICT", "实例已更新，请重新打开详情后再删除"));
        }
        let pending: bool = self.db.query_row("SELECT EXISTS(SELECT 1 FROM tasks WHERE entity_id=?1 AND state='pending')", [instance_id], |row| row.get(0))?;
        if pending { return Err(Failure::new("INSTANCE_BUSY", "实例仍有未完成操作，请完成或取消后再删除")); }
        let tx = self.db.transaction()?;
        tx.execute_batch("CREATE TABLE IF NOT EXISTS removed_instances(id TEXT PRIMARY KEY, document TEXT NOT NULL, removed_at INTEGER NOT NULL);")?;
        tx.execute("INSERT INTO removed_instances VALUES(?1,?2,?3)", params![instance_id, serde_json::to_string(&instance)?, now()])?;
        tx.execute("DELETE FROM instances WHERE id=?1", [instance_id])?;
        tx.execute("INSERT INTO tasks VALUES(?1,'delete_instance',?2,'completed',?3,?4)", params![operation_id, instance_id, now(), request])?;
        tx.commit()?;
        self.snapshot()
    }
    pub fn removed_instances(&self) -> Result<Vec<RemovedInstance>> {
        let exists: bool=self.db.query_row("SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='removed_instances')",[],|row|row.get(0))?;
        if !exists{return Ok(vec![]);}
        let rows=self.db.prepare("SELECT document,removed_at FROM removed_instances ORDER BY removed_at DESC,id")?.query_map([],|row|Ok((row.get::<_,String>(0)?,row.get::<_,u64>(1)?)))?.collect::<std::result::Result<Vec<_>,_>>()?;
        rows.into_iter().map(|(document,removed_at)|{
            let instance:Instance=serde_json::from_str(&document)?;uuid(&instance.id)?;
            let connection_available=match &instance.connection_id {Some(id)=>self.db.query_row("SELECT EXISTS(SELECT 1 FROM connections WHERE id=?1)",[id],|row|row.get(0))?,None=>false};
            let data_exists=self.root.join("instances").join(&instance.id).is_dir();
            Ok(RemovedInstance{instance,removed_at,connection_available,data_exists})
        }).collect()
    }
    pub fn restore_removed_instance(&mut self, instance_id:&str, operation_id:&str) -> Result<Snapshot> {
        uuid(instance_id)?;
        let request=serde_json::json!(["restore_removed_instance",instance_id]).to_string();
        if self.duplicate(operation_id,&request)?{return self.snapshot();}
        let archived=self.removed_instances()?.into_iter().find(|entry|entry.instance.id==instance_id).ok_or_else(||Failure::new("NOT_FOUND","已移除实例不存在，请刷新恢复中心"))?;
        if !archived.data_exists{return Err(Failure::new("INSTANCE_DATA_MISSING","保留的实例目录已不存在，未创建空实例；请先恢复原数据目录"));}
        let mut instance=archived.instance;
        // Validate the retained managed tree but never copy or rewrite user data.
        crate::managed::recipe(&self.root,&instance)?;
        if !archived.connection_available{instance.connection_id=None;}
        instance.revision=instance.revision.checked_add(1).ok_or_else(||Failure::new("REVISION_CONFLICT","实例修订无法继续递增"))?;
        instance.updated_at=now();
        let tx=self.db.transaction()?;
        tx.execute("INSERT INTO instances VALUES(?1,?2,?3)",params![instance.id,serde_json::to_string(&instance)?,instance.connection_id])?;
        tx.execute("DELETE FROM removed_instances WHERE id=?1",[instance_id])?;
        tx.execute("INSERT INTO tasks VALUES(?1,'restore_removed_instance',?2,'completed',?3,?4)",params![operation_id,instance_id,now(),request])?;
        tx.commit()?;
        self.snapshot()
    }
    pub fn delete_connection(
        &mut self,
        connection_id: &str,
        operation_id: &str,
        secrets: &dyn Secrets,
    ) -> Result<Snapshot> {
        self.recover(secrets)?;
        let request = serde_json::json!(["delete_connection", connection_id]).to_string();
        if self.duplicate(operation_id, &request)? {
            return self.snapshot();
        }
        let (_, reference) = self.connection(connection_id)?;
        let used: bool = self.db.query_row(
            "SELECT EXISTS(SELECT 1 FROM instances WHERE connection_id=?1)",
            [connection_id],
            |r| r.get(0),
        )?;
        if used {
            return Err(Failure::new(
                "CONNECTION_IN_USE",
                "仍有实例使用此连接，请先为它们更换连接",
            ));
        }
        let tx = self.db.transaction()?;
        tx.execute("DELETE FROM connections WHERE id=?1", [connection_id])?;
        tx.execute("INSERT INTO credential_cleanup VALUES(?1)", [reference])?;
        tx.execute(
            "INSERT INTO tasks VALUES(?1,'delete_connection',?2,'completed',?3,?4)",
            params![operation_id, connection_id, now(), request],
        )?;
        tx.commit()?;
        self.recover(secrets)?;
        self.snapshot()
    }
}
pub fn compatible(engine: &str, protocol: &str) -> Result<()> {
    if engine == "DSH" && !["deepseek", "openai-chat"].contains(&protocol) {
        return Err(Failure::new(
            "INCOMPATIBLE_CONNECTION",
            "当前 DSH 固定组合已验证 DeepSeek 与 OpenAI Chat Completions，其他协议尚未完成适配",
        ));
    }
    Ok(())
}
fn validate_models(models: &mut [ModelSpec], default_model: &str) -> Result<()> {
    if models.len() > 1000 {
        return Err(Failure::new("MODEL_LIMIT", "最多保存 1000 个模型"));
    }
    let mut seen = std::collections::HashSet::new();
    for model in models.iter_mut() {
        model.id = model.id.trim().into();
        model.name = model.name.trim().into();
        label(&model.id, 512)?;
        if model.name.is_empty() {
            model.name = model.id.clone();
        }
        label(&model.name, 512)?;
        if !seen.insert(model.id.clone()) {
            return Err(Failure::new(
                "MODEL_DUPLICATE",
                "模型 ID 重复，请合并或删除重复项",
            ));
        }
        if model.context_window == Some(0)
            || model.max_tokens == Some(0)
            || matches!((model.context_window,model.max_tokens),(Some(c),Some(m)) if m>c)
        {
            return Err(Failure::new(
                "MODEL_CAPACITY",
                "模型长度需为正整数，最大输出不能超过上下文窗口",
            ));
        }
    }
    if !seen.contains(default_model) {
        return Err(Failure::new(
            "MODEL_DEFAULT",
            "默认模型不在目录中，请重新选择",
        ));
    }
    Ok(())
}

// P2 does not import/export packages yet. Reject rather than silently accepting legacy/demo manifests.
#[tauri::command]
pub fn import_manifest(_document: String) -> Result<()> {
    Err(Failure::new(
        "IMPORT_NOT_AVAILABLE",
        "整合包导入将在 P5 提供，当前未写入任何数据",
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        cell::{Cell, RefCell},
        collections::HashMap,
    };
    #[derive(Default)]
    struct Vault {
        values: RefCell<HashMap<String, String>>,
        fail: Cell<bool>,
    }
    impl Secrets for Vault {
        fn get(&self, key: &str) -> Result<String> {
            self.values
                .borrow()
                .get(key)
                .cloned()
                .ok_or_else(|| Failure::new("MISSING_KEY", "测试密钥不存在"))
        }
        fn put(&self, key: &str, value: &str) -> Result<()> {
            if self.fail.get() {
                return Err(Failure::new("TEST_VAULT_FAILURE", "模拟凭据失败"));
            }
            self.values.borrow_mut().insert(key.into(), value.into());
            Ok(())
        }
        fn delete(&self, key: &str) -> Result<()> {
            self.values.borrow_mut().remove(key);
            Ok(())
        }
    }
    struct Fixture(PathBuf);
    impl Fixture {
        fn new() -> Self {
            let p = std::env::temp_dir().join(format!("perch-p2-{}", id()));
            fs::create_dir_all(p.join("项目 空格")).unwrap();
            Self(p)
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    #[test]
    fn remove_instance_preserves_files_connection_and_rejects_conflicts() {
        let f = Fixture::new();
        let root = f.0.join("workspace");
        let vault = Vault::default();
        let mut store = Store::open(root.clone()).unwrap();
        let connection = store.save_connection(connection_input(), &vault).unwrap().connections.remove(0);
        let first = store.save_instance(instance_input(&f.0, Some(connection.id.clone())), &vault).unwrap().instances.remove(0).instance;
        store.save_instance(instance_input(&f.0, Some(connection.id.clone())), &vault).unwrap();
        let data = root.join("instances").join(&first.id).join("session-test.json");
        fs::write(&data, b"retained session").unwrap();
        let project = f.0.join("项目 空格/keep.txt"); fs::write(&project, b"project").unwrap();
        assert_eq!(store.delete_instance(&first.id, first.revision+1, &id()).err().unwrap().code, "REVISION_CONFLICT");
        let pending = id(); store.engine_task(&pending, &first.id, "install").unwrap();
        assert_eq!(store.delete_instance(&first.id, first.revision, &id()).err().unwrap().code, "INSTANCE_BUSY");
        store.finish_engine_task_with_error(&pending, None).unwrap();
        let operation = id();
        assert_eq!(store.delete_instance(&first.id, first.revision, &operation).unwrap().instances.len(), 1);
        assert_eq!(store.delete_instance(&first.id, first.revision, &operation).unwrap().instances.len(), 1);
        assert_eq!(fs::read(&data).unwrap(), b"retained session");
        assert_eq!(fs::read(&project).unwrap(), b"project");
        assert!(store.connection(&connection.id).is_ok());
        assert!(!vault.values.borrow().is_empty());
        drop(store);
        let reopened = Store::open(root).unwrap();
        assert_eq!(reopened.snapshot().unwrap().instances.len(), 1);
        let saved: String = reopened.db.query_row("SELECT document FROM removed_instances WHERE id=?1", [&first.id], |row| row.get(0)).unwrap();
        assert_eq!(serde_json::from_str::<Instance>(&saved).unwrap().id, first.id);
    }
    #[test]
    fn restore_removed_instance_retains_data_and_handles_missing_connection(){
        let f=Fixture::new();let root=f.0.join("workspace");let vault=Vault::default();let mut store=Store::open(root.clone()).unwrap();
        assert!(store.removed_instances().unwrap().is_empty());
        let connection=store.save_connection(connection_input(),&vault).unwrap().connections.remove(0);
        let original=store.save_instance(instance_input(&f.0,Some(connection.id.clone())),&vault).unwrap().instances.remove(0).instance;
        let file=root.join("instances").join(&original.id).join("agent-data/session.json");fs::write(&file,"keep").unwrap();
        store.delete_instance(&original.id,original.revision,&id()).unwrap();
        assert!(store.removed_instances().unwrap()[0].connection_available);
        let operation=id();let restored=store.restore_removed_instance(&original.id,&operation).unwrap().instances.remove(0).instance;
        assert_eq!(restored.connection_id,Some(connection.id.clone()));assert_eq!(restored.revision,original.revision+1);
        assert_eq!(store.restore_removed_instance(&original.id,&operation).unwrap().instances.len(),1);
        assert!(store.removed_instances().unwrap().is_empty());assert_eq!(fs::read_to_string(&file).unwrap(),"keep");
        store.delete_instance(&restored.id,restored.revision,&id()).unwrap();store.delete_connection(&connection.id,&id(),&vault).unwrap();
        assert!(!store.removed_instances().unwrap()[0].connection_available);
        let restored=store.restore_removed_instance(&original.id,&id()).unwrap().instances.remove(0).instance;assert!(restored.connection_id.is_none());
        store.delete_instance(&restored.id,restored.revision,&id()).unwrap();
        let folder=root.join("instances").join(&original.id);let retained=root.join("retained-test");fs::rename(&folder,&retained).unwrap();
        assert_eq!(store.restore_removed_instance(&original.id,&id()).err().unwrap().code,"INSTANCE_DATA_MISSING");
        assert_eq!(store.removed_instances().unwrap().len(),1);assert_eq!(fs::read_to_string(retained.join("agent-data/session.json")).unwrap(),"keep");
    }
    fn connection_input() -> ConnectionInput {
        ConnectionInput {
            operation_id: id(),
            id: None,
            expected_revision: None,
            name: "测试连接".into(),
            provider: "DeepSeek".into(),
            protocol: "deepseek".into(),
            base_url: "https://api.deepseek.com".into(),
            default_model: "test-model".into(),
            models: vec![],
            api_key: Some("test-only-not-a-real-key".into()),
        }
    }
    fn instance_input(root: &Path, connection_id: Option<String>) -> InstanceInput {
        InstanceInput {
            profile_id: None,
            operation_id: id(),
            id: None,
            expected_revision: None,
            name: "日常开发".into(),
            engine: "Pi".into(),
            project_path: root.join("项目 空格").to_string_lossy().into(),
            connection_id,
        }
    }
    #[test]
    fn version_commit_recovery_and_missing_artifact(){
        let f=Fixture::new();let vault=Vault::default();let root=f.0.join("data");let mut store=Store::open(root.clone()).unwrap();
        let original=store.save_instance(instance_input(&f.0,None),&vault).unwrap().instances.remove(0).instance;
        let current=crate::catalog::packs().remove(1);crate::managed::apply(&root,&original,&current,"initial").unwrap();
        let mut profile:crate::engine_profile::ResolvedProfile=(&crate::engine_profile::PI).into();profile.id=format!("resolved-{}",Uuid::new_v4());crate::engine_profile::register(&root,profile.clone()).unwrap();
        let mut next=current.clone();next.profile_id=profile.id.clone();
        assert!(store.commit_version(&original.id,original.revision,&next,None,&vault).is_err());
        assert_eq!(store.instance(&original.id).unwrap().profile_id,original.profile_id);
        // Minimal artifact fixture tests persistence only, not an actual engine installation.
        let artifact=root.join("artifacts").join(&profile.id);fs::create_dir_all(artifact.join(&profile.entry).parent().unwrap()).unwrap();fs::write(artifact.join(&profile.entry),"fixture").unwrap();fs::write(artifact.join("package-lock.json"),&profile.lock).unwrap();fs::write(artifact.join("perch-install.json"),profile.marker()).unwrap();
        let corrupt = crate::managed::snapshot(&root, &original, "missing data fixture").unwrap();
        let point_dir = root.join("instances").join(&original.id).join("restore-points").join(&corrupt.id);
        let mut corrupt = corrupt;
        corrupt.recipe = next.clone();
        corrupt.data_version = 1;
        fs::write(point_dir.join("point.json"), serde_json::to_vec(&corrupt).unwrap()).unwrap();
        let data = point_dir.join("agent-data");
        if data.exists() { fs::rename(&data, point_dir.join("unavailable-data")).unwrap(); }
        assert!(store.commit_version(&original.id, original.revision, &next, Some(&corrupt.id), &vault).is_err());
        assert_eq!(store.instance(&original.id).unwrap().profile_id, original.profile_id);
        assert_eq!(store.db.query_row("SELECT count(*) FROM journal",[],|row|row.get::<_,u32>(0)).unwrap(),0);
        let blocked=root.join("instances").join(&original.id).join("manifest.json.pending");fs::create_dir(&blocked).unwrap();
        assert!(store.commit_version(&original.id,original.revision,&next,None,&vault).is_err());
        assert_eq!(store.db.query_row("SELECT count(*) FROM journal",[],|row|row.get::<_,u32>(0)).unwrap(),1);
        fs::remove_dir(blocked).unwrap();drop(store);
        let mut reopened=Store::open(root.clone()).unwrap();reopened.recover(&vault).unwrap();
        let instance=reopened.instance(&original.id).unwrap();assert_eq!(instance.profile_id,next.profile_id);assert_eq!(crate::managed::recipe(&root,&instance).unwrap(),next);
        assert!(crate::managed::points(&root,&instance).unwrap().iter().any(|point|point.recipe==current));
        assert_eq!(reopened.db.query_row("SELECT count(*) FROM journal",[],|row|row.get::<_,u32>(0)).unwrap(),0);
    }
    #[test]
    fn persistence_idempotency_revision_and_secret_boundaries() {
        let f = Fixture::new();
        let vault = Vault::default();
        let root = f.0.join("data");
        let mut store = Store::open(root.clone()).unwrap();
        let connection = store
            .save_connection(connection_input(), &vault)
            .unwrap()
            .connections
            .remove(0);
        let input = instance_input(&f.0, Some(connection.id.clone()));
        let encoded = serde_json::to_string(&input).unwrap();
        let first = store.save_instance(input, &vault).unwrap();
        let instance = &first.instances[0].instance;
        assert_eq!(first.instances[0].status, "not_installed");
        assert_eq!(first.tasks.len(), 2);
        assert_eq!(
            store
                .save_instance(serde_json::from_str(&encoded).unwrap(), &vault)
                .unwrap()
                .instances
                .len(),
            1
        );
        let manifest = fs::read_to_string(
            root.join("instances")
                .join(&instance.id)
                .join("manifest.json"),
        )
        .unwrap();
        let reread: Instance = serde_json::from_str(&manifest).unwrap();
        assert_eq!(reread.connection_id, Some(connection.id.clone()));
        let lock: serde_json::Value = serde_json::from_slice(
            &fs::read(root.join("instances").join(&instance.id).join("lock.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(lock["installed"], false);
        assert_eq!(lock["profile"]["version"], "0.9.3");
        let task_requests: Vec<String> = store
            .db
            .prepare("SELECT request FROM tasks")
            .unwrap()
            .query_map([], |r| r.get(0))
            .unwrap()
            .collect::<std::result::Result<_, _>>()
            .unwrap();
        assert!(!format!(
            "{manifest}{}{}",
            serde_json::to_string(&first).unwrap(),
            task_requests.join("")
        )
        .contains("test-only-not-a-real-key"));
        assert_eq!(
            store
                .delete_connection(&connection.id, &id(), &vault)
                .err()
                .unwrap()
                .code,
            "CONNECTION_IN_USE"
        );
        let mut update = instance_input(&f.0, Some(connection.id));
        update.id = Some(instance.id.clone());
        update.expected_revision = Some(0);
        assert_eq!(
            store.save_instance(update, &vault).err().unwrap().code,
            "REVISION_CONFLICT"
        );
        drop(store);
        let mut reopened = Store::open(root).unwrap();
        reopened.recover(&vault).unwrap();
        assert_eq!(
            reopened.snapshot().unwrap().instances[0].instance.id,
            instance.id
        );
        let mut update = instance_input(&f.0, instance.connection_id.clone());
        update.id = Some(instance.id.clone());
        update.expected_revision = Some(1);
        update.name = "已修改名称".into();
        let updated = reopened.save_instance(update, &vault).unwrap();
        assert_eq!(updated.instances[0].instance.revision, 2);
        assert_eq!(updated.instances[0].instance.name, "已修改名称");
        let second = instance_input(&f.0, instance.connection_id.clone());
        let shared = reopened.save_instance(second, &vault).unwrap();
        assert_eq!(shared.instances.len(), 2);
        assert_ne!(
            shared.instances[0].instance.id,
            shared.instances[1].instance.id
        );
        assert_eq!(
            shared.instances[0].instance.connection_id,
            shared.instances[1].instance.connection_id
        );
        assert_eq!(vault.values.borrow().len(), 1);
        fs::remove_dir(f.0.join("项目 空格")).unwrap();
        assert!(!reopened.snapshot().unwrap().instances[0].project_exists);
    }
    #[test]
    fn model_catalog_persistence_legacy_and_saved_key_origin() {
        let f = Fixture::new();
        let vault = Vault::default();
        let root = f.0.join("data");
        let mut store = Store::open(root.clone()).unwrap();
        let old = store
            .save_connection(connection_input(), &vault)
            .unwrap()
            .connections
            .remove(0);
        let mut legacy = serde_json::to_value(&old).unwrap();
        legacy.as_object_mut().unwrap().remove("models");
        assert!(serde_json::from_value::<ModelConnection>(legacy)
            .unwrap()
            .models
            .is_empty());
        let mut update = connection_input();
        update.id = Some(old.id.clone());
        update.expected_revision = Some(old.revision);
        update.api_key = None;
        update.models = vec![
            ModelSpec {
                id: "test-model".into(),
                name: "默认模型".into(),
                context_window: Some(128000),
                max_tokens: Some(8192),
            },
            ModelSpec {
                id: "another-model".into(),
                name: "另一个模型".into(),
                context_window: None,
                max_tokens: None,
            },
        ];
        store.save_connection(update, &vault).unwrap();
        drop(store);
        let store = Store::open(root).unwrap();
        let saved = store.snapshot().unwrap().connections.remove(0);
        assert_eq!(saved.models.len(), 2);
        assert_eq!(saved.models[0].max_tokens, Some(8192));
        assert_eq!(
            store
                .discovery_key(&old.id, "https://api.deepseek.com/v1", &vault)
                .unwrap(),
            "test-only-not-a-real-key"
        );
        assert_eq!(
            store
                .discovery_key(&old.id, "https://other.example/v1", &vault)
                .unwrap_err()
                .code,
            "KEY_ORIGIN_CHANGED"
        );
        assert_eq!(vault.values.borrow().len(), 1);
        let mut models = saved.models.clone();
        assert!(validate_models(&mut models, "missing-model").is_err());
        models.push(models[0].clone());
        assert!(validate_models(&mut models, "test-model").is_err());
    }
    #[test]
    fn failed_materialization_replays_after_reopen() {
        let f = Fixture::new();
        let vault = Vault::default();
        let root = f.0.join("data");
        let mut store = Store::open(root.clone()).unwrap();
        fs::write(root.join("instances"), b"file blocks directory").unwrap();
        let input = instance_input(&f.0, None);
        let encoded = serde_json::to_string(&input).unwrap();
        assert!(store.save_instance(input, &vault).is_err());
        assert_eq!(store.snapshot().unwrap().tasks[0].state, "pending");
        drop(store);
        fs::remove_file(root.join("instances")).unwrap();
        let mut store = Store::open(root).unwrap();
        store.recover(&vault).unwrap();
        let result = store
            .save_instance(serde_json::from_str(&encoded).unwrap(), &vault)
            .unwrap();
        assert_eq!(result.instances.len(), 1);
        assert_eq!(result.tasks.len(), 1);
        assert_eq!(result.tasks[0].state, "completed");
    }
    #[test]
    fn task_failure_codes_survive_reopen_without_private_error_text() {
        let root=std::env::temp_dir().join(format!("perch-task-failure-{}",Uuid::new_v4()));
        let store=Store::open(root.clone()).unwrap();
        let operation=Uuid::new_v4().to_string();let instance=Uuid::new_v4().to_string();
        store.engine_task(&operation,&instance,"start").unwrap();
        // Existing version-1 databases have no auxiliary failure table.
        store.db.execute_batch("DROP TABLE task_failures;").unwrap();drop(store);
        let store=Store::open(root.clone()).unwrap();
        assert_eq!(store.snapshot().unwrap().tasks.len(),1);
        store.finish_engine_task_with_error(&operation,Some(&Failure::new("INSTALL_TIMEOUT","fixture-private-key C:/private"))).unwrap();
        drop(store);
        let store=Store::open(root.clone()).unwrap();let snapshot=store.snapshot().unwrap();
        assert_eq!(snapshot.tasks[0].failure_code.as_deref(),Some("INSTALL_TIMEOUT"));
        assert_eq!(snapshot.tasks[0].state,"failed");
        assert!(!serde_json::to_string(&snapshot).unwrap().contains("fixture-private"));
        store.finish_engine_task_with_error(&operation,Some(&Failure::new("fixture-private-code","private message"))).unwrap();
        assert_eq!(store.snapshot().unwrap().tasks[0].failure_code.as_deref(),Some("ENGINE_FAILED"));
        store.finish_engine_task_with_error(&operation,None).unwrap();
        assert!(store.snapshot().unwrap().tasks[0].failure_code.is_none());
        drop(store);fs::remove_dir_all(root).unwrap();
    }
    #[test]
    #[ignore="one authorized actual version switch and rollback, isolated cached installation"]
    fn real_version_switch_and_rollback(){
        let root=PathBuf::from(std::env::var_os("PERCH_VERSION_TEST_ROOT").expect("explicit isolated root"));
        assert_eq!(root.parent().unwrap().canonicalize().unwrap(),std::env::temp_dir().canonicalize().unwrap());
        assert!(root.file_name().unwrap().to_string_lossy().starts_with("perch-p8-version-"));
        crate::engine_profile::load(&root).unwrap();
        let file=fs::read_dir(root.join("resolved-profiles")).unwrap().next().unwrap().unwrap().path();
        let profile:crate::engine_profile::ResolvedProfile=serde_json::from_slice(&fs::read(file).unwrap()).unwrap();
        assert_eq!(serde_json::from_str::<serde_json::Value>(&profile.package).unwrap()["dependencies"]["@agegr/pi-web"],"0.9.2");
        let project=root.join(format!("version-project-{}",id()));fs::create_dir_all(project.join("项目 空格")).unwrap();
        fs::write(project.join("项目 空格/keep.txt"),"unchanged").unwrap();
        let vault=Vault::default();let mut store=Store::open(root.clone()).unwrap();
        let mut input=connection_input();input.protocol="openai-chat".into();input.base_url="http://127.0.0.1:9/v1".into();
        let connection=store.save_connection(input,&vault).unwrap().connections.remove(0);
        let original=store.save_instance(instance_input(&project,Some(connection.id.clone())),&vault).unwrap().instances.into_iter().find(|item|item.instance.connection_id.as_ref()==Some(&connection.id)).unwrap().instance;
        let recipe=crate::catalog::packs().remove(1);crate::managed::apply(&root,&original,&recipe,"initial").unwrap();
        let engines=crate::engine::Engines::default();
        let outcome=std::panic::catch_unwind(std::panic::AssertUnwindSafe(||{
            engines.begin(&original,"install").unwrap();engines.execute(&root,&original,None,"install").unwrap();
            let point=crate::managed::snapshot(&root,&original,"before version trial").unwrap();
            let mut next=recipe.clone();next.profile_id=profile.id.clone();
            store.commit_version(&original.id,original.revision,&next,None,&vault).unwrap();
            for rollback in [false,true]{
                if rollback {let current=store.instance(&original.id).unwrap();store.commit_version(&original.id,current.revision,&recipe,Some(&point.id),&vault).unwrap();}
                let current=store.instance(&original.id).unwrap();assert_eq!(current.connection_id,original.connection_id);
                engines.begin(&current,"start").unwrap();engines.execute(&root,&current,Some((connection.clone(),"fake-version-test".into())),"start").unwrap();
                let address=engines.open(&current.id).unwrap();assert!(reqwest::blocking::get(address).unwrap().status().is_success());
                println!("VERSION_STARTED {} rollback={rollback}",current.profile_id);engines.stop(&current.id).unwrap();
            }
            assert_eq!(crate::managed::recipe(&root,&store.instance(&original.id).unwrap()).unwrap(),recipe);
            assert_eq!(fs::read_to_string(project.join("项目 空格/keep.txt")).unwrap(),"unchanged");
        }));engines.shutdown();if let Err(error)=outcome{std::panic::resume_unwind(error);}
    }
    #[test]
    fn engine_operations_are_idempotent_and_interrupted_on_reopen() {
        let f = Fixture::new();
        let root = f.0.join("data");
        let store = Store::open(root.clone()).unwrap();
        let operation = id();
        let instance = id();
        assert!(store.engine_task(&operation, &instance, "start").unwrap());
        assert!(!store.engine_task(&operation, &instance, "start").unwrap());
        assert!(store.engine_task(&operation, &instance, "stop").is_err());
        drop(store);
        let store = Store::open(root).unwrap();
        assert_eq!(store.snapshot().unwrap().tasks[0].state, "interrupted");
    }
    #[test]
    fn credential_failure_preserves_old_and_rotation_cleans_up() {
        let f = Fixture::new();
        let vault = Vault::default();
        let mut store = Store::open(f.0.join("data")).unwrap();
        let old = store
            .save_connection(connection_input(), &vault)
            .unwrap()
            .connections
            .remove(0);
        let mut update = connection_input();
        update.id = Some(old.id.clone());
        update.expected_revision = Some(1);
        update.api_key = Some("replacement-test-key".into());
        vault.fail.set(true);
        assert!(store.save_connection(update, &vault).is_err());
        assert_eq!(store.snapshot().unwrap().connections[0].revision, 1);
        assert_eq!(vault.values.borrow().len(), 1);
        vault.fail.set(false);
        store.recover(&vault).unwrap();
        let mut update = connection_input();
        update.id = Some(old.id.clone());
        update.expected_revision = Some(1);
        update.api_key = Some("replacement-test-key".into());
        store.save_connection(update, &vault).unwrap();
        assert_eq!(vault.values.borrow().len(), 1);
        assert!(vault
            .values
            .borrow()
            .values()
            .any(|v| v == "replacement-test-key"));
        store.delete_connection(&old.id, &id(), &vault).unwrap();
        assert!(vault.values.borrow().is_empty());
    }
    #[test]
    fn rejects_invalid_paths_protocol_urls_and_imports() {
        let f = Fixture::new();
        let vault = Vault::default();
        let mut store = Store::open(f.0.join("data")).unwrap();
        let mut input = instance_input(&f.0, None);
        input.project_path = "../escape".into();
        assert_eq!(
            store.save_instance(input, &vault).err().unwrap().code,
            "PROJECT_MISSING"
        );
        let mut input = instance_input(&f.0, None);
        input.id = Some("../escape".into());
        assert_eq!(
            store.save_instance(input, &vault).err().unwrap().code,
            "INVALID_ID"
        );
        for address in [
            "https://user:secret@example.com",
            "https://example.com?key=secret",
            "http://example.com",
        ] {
            let mut input = connection_input();
            input.base_url = address.into();
            assert_eq!(
                store.save_connection(input, &vault).err().unwrap().code,
                "INVALID_URL"
            );
        }
        assert!(compatible("DSH", "anthropic").is_err());
        assert!(compatible("DSH", "deepseek").is_ok());
        assert!(
            serde_json::from_str::<InstanceInput>(r#"{"operationId":"bad","running":true}"#)
                .is_err()
        );
        assert!(import_manifest("{malformed}".into()).is_err());
        assert!(store.snapshot().unwrap().instances.is_empty());
        assert!(vault.values.borrow().is_empty());
    }
}
