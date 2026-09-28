use crate::engine_profile::{self, ResolvedProfile};
use crate::{
    owned_process::OwnedProcess,
    store::{Failure, Instance, ModelConnection, Result},
};
use serde::Serialize;
use std::{
    collections::{HashMap, VecDeque},
    fs,
    io::{Read, Write},
    net::TcpListener,
    path::{Path, PathBuf},
    process::Command,
    sync::{Arc, Mutex},
    thread,
    time::{Duration, Instant},
};
use tauri::Manager;
use tauri_plugin_opener::OpenerExt;
const HOST: &str = include_str!("engine-host.mjs");
#[derive(Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeView {
    pub id: String,
    pub status: String,
    pub phase: String,
    pub error: Option<Failure>,
    pub logs: VecDeque<String>,
    pub connection_revision: Option<u32>,
    pub instance_revision: Option<u32>,
}
#[derive(Default)]
struct Output {
    lines: VecDeque<String>,
    url: Option<String>,
    workspace_ready: bool,
}
struct Slot {
    cancelled: bool,
    cancellable: bool,
    view: RuntimeView,
    process: Option<OwnedProcess>,
    output: Arc<Mutex<Output>>,
}
#[derive(Default)]
pub struct Engines {
    slots: Mutex<HashMap<String, Slot>>,
    installer: Mutex<()>,
    closing: std::sync::atomic::AtomicBool,
}
fn fail(code: &str, msg: &str) -> Failure {
    Failure::new(code, msg)
}
fn install_dir(root: &Path, profile: &ResolvedProfile) -> PathBuf {
    root.join("artifacts").join(&profile.id)
}
struct StagingCleanup(PathBuf);
impl Drop for StagingCleanup {
    fn drop(&mut self) {
        // Only remove the newly-created UUID staging directory, never a cache
        // target or a path outside its verified artifacts parent.
        let Some(parent) = self.0.parent() else {
            return;
        };
        let (Ok(parent), Ok(target)) = (parent.canonicalize(), self.0.canonicalize()) else {
            return;
        };
        if target.parent() == Some(parent.as_path())
            && target
                .file_name()
                .is_some_and(|n| n.to_string_lossy().starts_with(".staging-"))
        {
            let _ = fs::remove_dir_all(target);
        }
    }
}
fn promote(staging: &Path, target: &Path) -> Result<()> {
    let previous = target.with_file_name(format!(".previous-{}", uuid::Uuid::new_v4()));
    if target.exists() {
        fs::rename(target, &previous)?;
    }
    if let Err(error) = fs::rename(staging, target) {
        if previous.exists() {
            fs::rename(&previous, target).map_err(|_| {
                fail(
                    "INSTALL_ROLLBACK",
                    "安装启用失败；旧工件已保留在 artifacts/.previous 目录，请勿删除",
                )
            })?;
        }
        return Err(error.into());
    }
    Ok(())
}
pub fn installed(root: &Path, engine: &str, id: &str) -> bool {
    let Ok(profile) = engine_profile::resolve(engine, id) else {
        return false;
    };
    let p = install_dir(root, &profile);
    p.join(&profile.entry).is_file()
        && fs::read_to_string(p.join("package-lock.json")).is_ok_and(|s| s == profile.lock)
        && fs::read_to_string(p.join("perch-install.json")).is_ok_and(|s| s == profile.marker())
}
pub(crate) fn clean_command(executable: &Path) -> Command {
    let mut c = Command::new(executable);
    c.env_clear();
    for key in [
        "SystemRoot",
        "WINDIR",
        "TEMP",
        "TMP",
        "USERPROFILE",
        "APPDATA",
        "LOCALAPPDATA",
        "ProgramFiles",
        "ProgramFiles(x86)",
        "ComSpec",
        "PATHEXT",
        "PATH",
    ] {
        if let Some(v) = std::env::var_os(key) {
            c.env(key, v);
        }
    }
    c.env("NO_COLOR", "1").env("FORCE_COLOR", "0");
    c
}
pub(crate) fn node() -> Result<(PathBuf, PathBuf)> {
    let path = std::env::var_os("PATH").unwrap_or_default();
    let executable = std::env::split_paths(&path)
        .map(|p| p.join("node.exe"))
        .find(|p| p.is_file())
        .ok_or_else(|| {
            fail(
                "NODE_MISSING",
                "请安装 Node.js 24.18.0（含 npm 12.0.2）后重试",
            )
        })?;
    let mut command = clean_command(&executable);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    let output = command
        .args([
            "-p",
            "[process.version,process.platform,process.arch].join(' ')",
        ])
        .output()?;
    if !output.status.success()
        || String::from_utf8_lossy(&output.stdout).trim() != "v24.18.0 win32 x64"
    {
        return Err(fail(
            "NODE_VERSION",
            "当前需要 Windows x64 版 Node.js 24.18.0，请安装该版本后重试",
        ));
    }
    let bundled_npm = executable
        .parent()
        .unwrap()
        .join("node_modules/npm/bin/npm-cli.js");
    let npm = std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .map(|p| p.join("npm/node_modules/npm/bin/npm-cli.js"))
        .filter(|p| p.is_file())
        .unwrap_or(bundled_npm);
    let package: serde_json::Value = serde_json::from_slice(
        &fs::read(npm.parent().unwrap().parent().unwrap().join("package.json"))
            .map_err(|_| fail("NPM_MISSING", "Node.js 安装缺少 npm 12.0.2"))?,
    )?;
    if package["version"] != "12.0.2" {
        return Err(fail("NPM_VERSION", "当前固定安装流程需要 npm 12.0.2"));
    }
    Ok((executable, npm))
}
fn sanitize(line: &str, key: &str) -> String {
    let lower = line.to_lowercase();
    if lower.contains("token")
        || lower.contains("authorization")
        || lower.contains("api_key")
        || lower.contains("apikey")
        || lower.contains("cookie")
        || lower.contains("credential")
        || lower.contains("dsh web: http")
    {
        return "[已隐藏含认证信息的日志]".into();
    }
    let value = if key.is_empty() {
        line.to_string()
    } else {
        line.replace(key, "[密钥已隐藏]")
    };
    value
        .chars()
        .filter(|c| !c.is_control() || *c == '\t')
        .take(2000)
        .collect()
}
fn capture(
    reader: impl Read + Send + 'static,
    output: Arc<Mutex<Output>>,
    key: String,
    port: Option<u16>,
    log: PathBuf,
) {
    thread::spawn(move || {
        let mut reader = reader;
        let mut bytes = [0u8; 2048];
        let mut line = Vec::new();
        let mut overflow = false;
        let mut window = Instant::now();
        let mut count = 0u32;
        loop {
            let n = match reader.read(&mut bytes) {
                Ok(0) | Err(_) => break,
                Ok(n) => n,
            };
            for b in &bytes[..n] {
                if *b != b'\n' {
                    if line.len() < 65536 {
                        line.push(*b);
                    } else {
                        overflow = true;
                    }
                    continue;
                }
                let raw = String::from_utf8_lossy(&line);
                let mut out = output.lock().unwrap_or_else(|e| e.into_inner());
                if !overflow {
                    if raw.trim() == "PERCH_WORKSPACE_READY" {
                        out.workspace_ready = true;
                    }
                    if let Some(port) = port {
                        if let Some(pos) = raw.find("dsh web: http://127.0.0.1:") {
                            let candidate = raw[pos + 9..].split_whitespace().next().unwrap_or("");
                            if let Ok(url) = url::Url::parse(candidate) {
                                if url.host_str() == Some("127.0.0.1")
                                    && url.port() == Some(port)
                                    && url.path() == "/"
                                    && !url.query().unwrap_or("").is_empty()
                                {
                                    out.url = Some(candidate.into());
                                }
                            }
                        }
                    }
                    if window.elapsed() >= Duration::from_secs(1) {
                        window = Instant::now();
                        count = 0;
                    }
                    count += 1;
                    let text = if count > 101 {
                        String::new()
                    } else if count == 101 {
                        "[日志输出过快，已限流]".into()
                    } else {
                        sanitize(&raw, &key)
                    };
                    if !text.is_empty() {
                        if out.lines.len() == 160 {
                            out.lines.pop_front();
                        }
                        out.lines.push_back(text.clone());
                        // Disk logs are bounded as well as the IPC tail; only sanitized text is written.
                        if fs::metadata(&log).is_ok_and(|m| m.len() > 1024 * 1024) {
                            let _ = fs::rename(&log, log.with_extension("previous.log"));
                        }
                        if let Ok(mut file) =
                            fs::OpenOptions::new().create(true).append(true).open(&log)
                        {
                            let _ = writeln!(file, "{text}");
                        }
                    }
                }
                line.clear();
                overflow = false;
            }
        }
    });
}
// The launch boundary keeps process inputs explicit rather than hiding ownership in a shared context.
#[allow(clippy::too_many_arguments)]
fn launch(
    node: &Path,
    host: &Path,
    cwd: &Path,
    payload: &serde_json::Value,
    output: Arc<Mutex<Output>>,
    key: &str,
    port: Option<u16>,
    log: &Path,
) -> Result<OwnedProcess> {
    let mut command = clean_command(node);
    command.arg(host).current_dir(cwd);
    let mut process = OwnedProcess::spawn(&mut command)?;
    capture(
        process.child.stdout.take().unwrap(),
        output.clone(),
        key.into(),
        port,
        log.into(),
    );
    capture(
        process.child.stderr.take().unwrap(),
        output,
        key.into(),
        None,
        log.into(),
    );
    let input = process.child.stdin.as_mut().unwrap();
    serde_json::to_writer(&mut *input, payload)?;
    input.write_all(b"\n")?;
    input.flush()?;
    Ok(process)
}
impl Engines {
    fn attach(&self, id: &str, process: OwnedProcess) -> Result<()> {
        let mut slots = self.slots.lock().unwrap_or_else(|e| e.into_inner());
        if self.closing.load(std::sync::atomic::Ordering::SeqCst) {
            return Err(fail("CANCELLED", "应用正在退出"));
        }
        let slot = slots
            .get_mut(id)
            .ok_or_else(|| fail("CANCELLED", "实例操作已取消"))?;
        if slot.cancelled {
            return Err(fail("CANCELLED", "操作已取消"));
        }
        slot.process = Some(process);
        Ok(())
    }
    pub fn cancel(&self, id: &str) -> Result<()> {
        let mut slots = self.slots.lock().unwrap();
        let slot = slots
            .get_mut(id)
            .ok_or_else(|| fail("NOT_RUNNING", "没有正在进行的任务"))?;
        if !slot.cancellable || !["installing", "starting"].contains(&slot.view.status.as_str()) {
            return Err(fail("NOT_CANCELLABLE", "此任务已结束，无法取消"));
        }
        slot.cancelled = true;
        slot.process.take();
        slot.view.phase = "正在取消并清理暂存内容".into();
        Ok(())
    }
    pub fn begin_commit(&self, id: &str) -> Result<()> {
        let mut slots = self.slots.lock().unwrap();
        let slot = slots
            .get_mut(id)
            .ok_or_else(|| fail("CANCELLED", "任务已取消"))?;
        if slot.cancelled || self.closing.load(std::sync::atomic::Ordering::SeqCst) {
            return Err(fail("CANCELLED", "操作已取消，原组合保留"));
        }
        slot.cancellable = false;
        slot.view.phase = "正在启用组合并保存恢复点".into();
        Ok(())
    }
    pub fn finish_install(&self, id: &str) {
        self.phase(id, "stopped", "固定组合已安装");
    }
    pub(crate) fn resource_progress(&self,id:&str,index:usize,total:usize)->Result<()> {
        self.check_cancelled(id)?;
        let mut slots=self.slots.lock().unwrap_or_else(|error|error.into_inner());
        let slot=slots.get_mut(id).ok_or_else(||fail("CANCELLED","安装任务已结束"))?;
        if slot.cancelled{return Err(fail("CANCELLED","操作已取消，原组合保留"));}
        slot.view.phase=format!("准备工坊资源 {index}/{total}（下载或校验缓存）");
        Ok(())
    }
    pub fn check_cancelled(&self, id: &str) -> Result<()> {
        if self.closing.load(std::sync::atomic::Ordering::SeqCst)
            || self
                .slots
                .lock()
                .unwrap()
                .get(id)
                .is_some_and(|s| s.cancelled)
        {
            return Err(fail("CANCELLED", "操作已取消，原组合保留"));
        }
        Ok(())
    }
    pub fn begin_stop(&self, id: &str) -> Result<()> {
        let mut slots = self.slots.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(s) = slots.get_mut(id) {
            if ["installing", "starting", "stopping"].contains(&s.view.status.as_str()) {
                return Err(fail("INSTANCE_BUSY", "请等待当前操作完成后停止"));
            }
            s.view.status = "stopping".into();
        }
        Ok(())
    }
    pub fn views(&self, root: &Path, instances: &[crate::store::InstanceView]) -> Vec<RuntimeView> {
        let mut slots = self.slots.lock().unwrap_or_else(|e| e.into_inner());
        instances
            .iter()
            .map(|i| {
                if let Some(s) = slots.get_mut(&i.instance.id) {
                    if s.view.status == "running"
                        && s.process
                            .as_mut()
                            .is_some_and(|p| p.child.try_wait().ok().flatten().is_some())
                    {
                        s.process.take();
                        s.view.status = "failed".into();
                        s.view.phase = "引擎已退出".into();
                        s.view.error =
                            Some(fail("PROCESS_EXIT", "引擎意外退出，可查看日志后重新启动"));
                    }
                    let mut v = s.view.clone();
                    v.logs = s
                        .output
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .lines
                        .clone();
                    v
                } else {
                    RuntimeView {
                        id: i.instance.id.clone(),
                        status: if installed(root, &i.instance.engine, &i.instance.profile_id) {
                            "stopped"
                        } else {
                            "not_installed"
                        }
                        .into(),
                        phase: String::new(),
                        ..Default::default()
                    }
                }
            })
            .collect()
    }
    fn phase(&self, id: &str, status: &str, phase: &str) {
        if let Some(s) = self
            .slots
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get_mut(id)
        {
            s.view.status = status.into();
            s.view.phase = phase.into();
        }
    }
    pub fn begin(&self, instance: &Instance, action: &str) -> Result<()> {
        if self.closing.load(std::sync::atomic::Ordering::SeqCst) {
            return Err(fail("APP_CLOSING", "应用正在退出"));
        }
        engine_profile::resolve(&instance.engine, &instance.profile_id)?;
        let mut slots = self.slots.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(s) = slots.get_mut(&instance.id) {
            if ["installing", "starting", "stopping"].contains(&s.view.status.as_str()) {
                return Err(fail("INSTANCE_BUSY", "实例正在执行操作，请稍候"));
            }
            if s.view.status == "running" {
                return Err(fail("ALREADY_RUNNING", "实例已运行，请先停止再修改或重启"));
            }
        }
        slots.insert(
            instance.id.clone(),
            Slot {
                cancelled: false,
                cancellable: true,
                view: RuntimeView {
                    id: instance.id.clone(),
                    status: if action == "install" {
                        "installing"
                    } else {
                        "starting"
                    }
                    .into(),
                    phase: "检查运行时与固定组合".into(),
                    ..Default::default()
                },
                process: None,
                output: Arc::new(Mutex::new(Output::default())),
            },
        );
        Ok(())
    }
    pub fn busy(&self, id: &str) -> bool {
        self.slots
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(id)
            .is_some_and(|s| {
                ["running", "starting", "installing", "stopping"].contains(&s.view.status.as_str())
            })
    }
    pub fn error(&self, id: &str, e: Failure) {
        let mut slots = self.slots.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(s) = slots.get_mut(id) {
            s.process.take();
            s.view.status = "failed".into();
            s.view.phase = e.message.clone();
            s.view.error = Some(e);
        }
    }
    pub fn execute(
        &self,
        root: &Path,
        i: &Instance,
        connection: Option<(ModelConnection, String)>,
        action: &str,
    ) -> Result<()> {
        if action == "start" && !Path::new(&i.project_path).is_dir() {
            return Err(fail("PROJECT_MISSING", "项目目录不存在，请重新选择"));
        }
        self.check_cancelled(&i.id)?;
        let profile = engine_profile::resolve(&i.engine, &i.profile_id)?;
        let (node, npm) = node()?;
        let host = root
            .join("instances")
            .join(&i.id)
            .join("config/engine-host.mjs");
        fs::create_dir_all(host.parent().unwrap())?;
        fs::write(&host, HOST)?;
        fs::write(host.with_file_name("public-preset.mjs"),include_str!("public-preset.mjs"))?;
        fs::write(
            host.with_file_name("pi-config.mjs"),
            include_str!("pi-config.mjs"),
        )?;
        let bridge = host.with_file_name("dsh-workspace.mjs");
        fs::write(&bridge, include_str!("dsh-workspace.mjs"))?;
        let patch = host.with_file_name("perch.patch.json");
        let module = url::Url::from_file_path(&bridge)
            .map_err(|_| fail("CONFIG_PATH", "无法解析实例配置路径"))?;
        fs::write(
            &patch,
            serde_json::to_vec(
                &serde_json::json!([{"insert":[{"id":"perch-workspace","name":module.as_str()}]}]),
            )?,
        )?;
        let output = self
            .slots
            .lock()
            .unwrap()
            .get(&i.id)
            .unwrap()
            .output
            .clone();
        let log = root.join("instances").join(&i.id).join("logs/engine.log");
        fs::create_dir_all(log.parent().unwrap())?;
        {
            let _guard = self
                .installer
                .lock()
                .map_err(|_| fail("INSTALL_LOCK", "安装服务不可用"))?;
            if !installed(root, &i.engine, &i.profile_id) {
                self.phase(
                    &i.id,
                    "installing",
                    "下载并校验锁定依赖（首次安装可能需要数分钟）",
                );
                let staging = root
                    .join("artifacts")
                    .join(format!(".staging-{}", uuid::Uuid::new_v4()));
                fs::create_dir_all(&staging)?;
                let _cleanup = StagingCleanup(staging.clone());
                fs::write(staging.join("package.json"), &profile.package)?;
                fs::write(staging.join("package-lock.json"), &profile.lock)?;
                let process = launch(
                    &node,
                    &host,
                    &staging,
                    &serde_json::json!({"mode":"install","npm":npm}),
                    output.clone(),
                    "",
                    None,
                    &log,
                )?;
                self.attach(&i.id, process)?;
                let started = Instant::now();
                let result = loop {
                    if self.closing.load(std::sync::atomic::Ordering::SeqCst) {
                        break Err(fail("CANCELLED", "安装已随应用退出取消"));
                    }
                    let status = {
                        let mut slots = self.slots.lock().unwrap();
                        slots
                            .get_mut(&i.id)
                            .and_then(|s| s.process.as_mut())
                            .ok_or_else(|| fail("CANCELLED", "安装已取消"))?
                            .child
                            .try_wait()?
                    };
                    if let Some(status) = status {
                        break if status.success() {
                            Ok(())
                        } else {
                            Err(fail(
                                "INSTALL_FAILED",
                                "依赖安装失败，请检查网络与安装日志后重试；旧安装保留",
                            ))
                        };
                    }
                    if started.elapsed() > Duration::from_secs(600) {
                        break Err(fail("INSTALL_TIMEOUT", "安装超时，请检查网络后重试"));
                    }
                    thread::sleep(Duration::from_millis(200));
                };
                self.slots
                    .lock()
                    .unwrap()
                    .get_mut(&i.id)
                    .unwrap()
                    .process
                    .take();
                result?;
                if fs::read_to_string(staging.join("package-lock.json"))? != profile.lock
                    || !staging.join(&profile.entry).is_file()
                {
                    return Err(fail("INSTALL_INTEGRITY", "安装校验失败，未启用暂存目录"));
                }
                fs::write(staging.join("perch-install.json"), profile.marker())?;
                self.check_cancelled(&i.id)?;
                let target = install_dir(root, &profile);
                crate::resource_catalog::validate_dsh_artifacts(&staging,&profile).map_err(|message|fail("DSH_VERSION",&message))?;
                promote(&staging, &target)?;
            }
        }
        crate::resource_catalog::validate_dsh_artifacts(&install_dir(root,&profile),&profile).map_err(|message|fail("DSH_VERSION",&message))?;
        let lock = root.join("instances").join(&i.id).join("lock.json");
        if lock.exists() && action!="prepare" {
            let mut document: serde_json::Value = serde_json::from_slice(&fs::read(&lock)?)?;
            document["installed"] = true.into();
            document["runtime"] = serde_json::json!({"node":profile.node,"npm":profile.npm,"source":"verified-local"});
            let pending = lock.with_extension("json.pending");
            fs::write(&pending, serde_json::to_vec_pretty(&document)?)?;
            fs::rename(pending, lock)?;
        }
        self.check_cancelled(&i.id)?;
        if ["install", "prepare"].contains(&action) {
            if action == "install" {
                self.finish_install(&i.id);
            }
            return Ok(());
        }
        let (c, key) = connection.ok_or_else(|| {
            fail(
                "CONNECTION_REQUIRED",
                "请先在模型连接中配置 Key 并绑定此实例",
            )
        })?;
        if !Path::new(&i.project_path).is_dir() {
            return Err(fail("PROJECT_MISSING", "项目目录不存在，请重新选择"));
        }
        let install = install_dir(root, &profile);
        crate::managed::recover(root, &i.id)?;
        let extensions = crate::managed::extension_files(root, i, &install)?;
        let skills = crate::managed::skill_roots(root, i)?;
        let selected=crate::managed::recipe(root,i)?;
        let packages:Vec<serde_json::Value>=selected.extensions.iter().filter(|item|item.enabled).filter_map(|item|{
            let name=item.id.strip_prefix("npm:").or_else(||item.id.strip_prefix("dsh-npm:"))?;let path=install.join("node_modules").join(name);
            if item.disabled_resources.is_empty()&&item.resource_rules.is_empty(){Some(serde_json::json!(path))}else{let mut selection=serde_json::json!({"source":path});for kind in &item.disabled_resources{selection[kind]=serde_json::json!([]);}for (kind,rules) in &item.resource_rules{selection[kind]=serde_json::json!(rules);}Some(selection)}
        }).collect();
        if i.engine == "DSH" {
            let mut entries =
                vec![serde_json::json!({"id":"perch-workspace","name":module.as_str()})];
            for (index, file) in extensions.iter().enumerate() {
                let url = url::Url::from_file_path(file)
                    .map_err(|_| fail("EXTENSION_PATH", "扩展路径无效"))?;
                entries.push(serde_json::json!({"id":format!("perch-extension-{index}"),"name":url.as_str()}));
            }
            fs::write(
                &patch,
                serde_json::to_vec(&serde_json::json!([{"insert":entries}]))?,
            )?;
        }
        let home = root.join("instances").join(&i.id).join("agent-data");
        for attempt in 0..3 {
            self.phase(&i.id, "starting", "启动引擎并检查工作台健康状态");
            let listener = TcpListener::bind("127.0.0.1:0")?;
            let port = listener.local_addr()?.port();
            drop(listener);
            {
                let mut out = output.lock().unwrap();
                out.url = if i.engine == "Pi" {
                    Some(format!("http://127.0.0.1:{port}/"))
                } else {
                    None
                };
                out.workspace_ready = i.engine == "Pi";
            }
            let process = launch(
                &node,
                &host,
                Path::new(&i.project_path),
                &serde_json::json!({"mode":"start","engine":i.engine,"install":install,"entry":install.join(&profile.entry),"home":home,"port":port,"connection":c,"key":key,"patch":patch,"extensions":extensions,"skills":skills,"packages":packages,"thinkingLevel":selected.thinking_level}),
                output.clone(),
                &key,
                Some(port),
                &log,
            )?;
            self.attach(&i.id, process)?;
            let begun = Instant::now();
            let mut healthy = false;
            while begun.elapsed() < Duration::from_secs(60) {
                self.check_cancelled(&i.id)?;
                if self.closing.load(std::sync::atomic::Ordering::SeqCst) {
                    return Err(fail("CANCELLED", "启动已取消"));
                }
                {
                    let mut slots = self.slots.lock().unwrap();
                    let s = slots.get_mut(&i.id).unwrap();
                    if s.process
                        .as_mut()
                        .ok_or_else(|| fail("CANCELLED", "启动已取消"))?
                        .child
                        .try_wait()?
                        .is_some()
                    {
                        break;
                    }
                }
                let url = output.lock().unwrap().url.clone();
                if let Some(url) = url {
                    if output.lock().unwrap().workspace_ready && health(&url) {
                        healthy = true;
                        break;
                    }
                }
                thread::sleep(Duration::from_millis(300));
            }
            if healthy {
                if i.engine == "Pi" {
                    let base = format!("http://127.0.0.1:{port}/");
                    let url = pi_workspace(&base, &i.project_path)?;
                    output.lock().unwrap().url = Some(url);
                }
                let mut slots = self.slots.lock().unwrap();
                let s = slots.get_mut(&i.id).unwrap();
                s.view.status = "running".into();
                s.view.phase = "工作台已就绪".into();
                s.view.connection_revision = Some(c.revision);
                s.view.instance_revision = Some(i.revision);
                return Ok(());
            }
            self.slots
                .lock()
                .unwrap()
                .get_mut(&i.id)
                .unwrap()
                .process
                .take();
            let conflict = output
                .lock()
                .unwrap()
                .lines
                .iter()
                .any(|l| l.contains("EADDRINUSE"));
            if !conflict || attempt == 2 {
                return Err(fail(
                    if conflict {
                        "PORT_CONFLICT"
                    } else {
                        "HEALTH_FAILED"
                    },
                    "工作台未通过健康检查，进程已清理；请查看日志后重试",
                ));
            }
        }
        Err(fail("START_FAILED", "启动失败"))
    }
    pub fn stop(&self, id: &str) -> Result<()> {
        let mut process = {
            let mut slots = self.slots.lock().unwrap();
            let Some(s) = slots.get_mut(id) else {
                return Ok(());
            };
            s.view.status = "stopping".into();
            s.view.phase = "正在停止工作台".into();
            s.process.take()
        };
        if let Some(p) = process.as_mut() {
            if let Some(input) = p.child.stdin.as_mut() {
                let _ = input.write_all(b"stop\n");
                let _ = input.flush();
            }
            let deadline = Instant::now();
            while deadline.elapsed() < Duration::from_secs(6) {
                if p.child.try_wait()?.is_some() {
                    break;
                }
                thread::sleep(Duration::from_millis(100));
            }
            p.terminate();
        }
        self.phase(id, "stopped", "已停止工作台及所属子进程");
        Ok(())
    }
    pub fn shutdown(&self) {
        self.closing
            .store(true, std::sync::atomic::Ordering::SeqCst);
        let mut slots = self.slots.lock().unwrap_or_else(|e| e.into_inner());
        for s in slots.values_mut() {
            s.process.take();
            s.view.status = "stopped".into();
        }
    }
    pub fn stop_running(&self) {
        let ids: Vec<String> = self
            .slots
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .iter()
            .filter(|(_, s)| s.view.status == "running")
            .map(|(id, _)| id.clone())
            .collect();
        for id in ids {
            if self.begin_stop(&id).is_ok() {
                let _ = self.stop(&id);
            }
        }
    }
    pub fn open(&self, id: &str) -> Result<String> {
        let slots = self.slots.lock().unwrap();
        let s = slots
            .get(id)
            .filter(|s| s.view.status == "running")
            .ok_or_else(|| fail("NOT_RUNNING", "工作台尚未就绪，请先启动"))?;
        let url = s
            .output
            .lock()
            .unwrap()
            .url
            .clone()
            .ok_or_else(|| fail("NOT_RUNNING", "工作台地址尚未就绪"))?;
        if !health(&url) {
            return Err(fail(
                "HEALTH_FAILED",
                "工作台目前无法连接，请检查日志或重新启动",
            ));
        }
        Ok(url)
    }
}
fn pi_workspace(base: &str, project: &str) -> Result<String> {
    let client = reqwest::blocking::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(30))
        .build()
        .map_err(|_| fail("PI_WORKSPACE", "无法准备 Pi 工作区"))?;
    let sessions = client
        .get(format!("{base}api/sessions"))
        .send()
        .ok()
        .filter(|r| r.status().is_success())
        .and_then(|r| r.json::<serde_json::Value>().ok());
    let existing = sessions
        .as_ref()
        .and_then(|v| v["sessions"].as_array())
        .and_then(|list| list.iter().find(|s| s["cwd"].as_str() == Some(project)))
        .and_then(|s| s["id"].as_str())
        .map(str::to_owned);
    let mut url = url::Url::parse(base).map_err(|_| fail("PI_WORKSPACE", "Pi 地址无效"))?;
    if let Some(id) = existing {
        url.query_pairs_mut().append_pair("session", &id);
    } else {
        // Empty ensure_session results are not in Pi's persisted sidebar list.
        // Its supported cwd entry prepares the composer without a phantom session.
        let valid = client
            .post(format!("{base}api/cwd/validate"))
            .json(&serde_json::json!({"cwd":project}))
            .send()
            .ok()
            .filter(|r| r.status().is_success())
            .and_then(|r| r.json::<serde_json::Value>().ok())
            .is_some_and(|v| v["cwd"].as_str().is_some_and(|s| !s.is_empty()));
        if !valid {
            return Err(fail(
                "PI_WORKSPACE",
                "Pi 无法打开项目目录，请检查目录是否存在及访问权限",
            ));
        }
        url.query_pairs_mut().append_pair("cwd", project);
    }
    Ok(url.into())
}
fn health(url: &str) -> bool {
    let Ok(client) = reqwest::blocking::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .no_proxy()
        .timeout(Duration::from_secs(2))
        .build()
    else {
        return false;
    };
    let Ok(response) = client.get(url).send() else {
        return false;
    };
    if url::Url::parse(url).is_ok_and(|u| {
        u.query().is_none()
            || u.query()
                .is_some_and(|q| q.starts_with("session=") || q.starts_with("cwd="))
    }) {
        if !response.status().is_success() {
            return false;
        }
        let Ok(mut endpoint) = url::Url::parse(url) else {
            return false;
        };
        endpoint.set_path("/api/sessions");
        endpoint.set_query(None);
        return client
            .get(endpoint)
            .send()
            .ok()
            .filter(|r| r.status().is_success())
            .and_then(|r| r.json::<serde_json::Value>().ok())
            .is_some_and(|v| v["sessions"].is_array());
    }
    if !response.status().is_redirection() {
        return false;
    }
    let Some(cookie) = response
        .headers()
        .get(reqwest::header::SET_COOKIE)
        .and_then(|h| h.to_str().ok())
        .map(|s| s.split(';').next().unwrap_or("").to_string())
    else {
        return false;
    };
    let Ok(mut root) = url::Url::parse(url) else {
        return false;
    };
    root.set_query(None);
    client
        .get(root)
        .header(reqwest::header::COOKIE, cookie)
        .send()
        .ok()
        .filter(|r| r.status().is_success())
        .and_then(|r| r.text().ok())
        .is_some_and(|s| s.contains("__DSH_BOOT__"))
}
#[tauri::command]
pub async fn engine_open(app: tauri::AppHandle, id: String) -> Result<()> {
    tauri::async_runtime::spawn_blocking(move || {
        let url = app.state::<Engines>().open(&id)?;
        app.opener()
            .open_url(url, None::<&str>)
            .map_err(|_| fail("BROWSER_OPEN", "默认浏览器未能打开，请重试"))
    })
    .await
    .map_err(|_| fail("TASK_FAILED", "打开失败"))?
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[ignore = "starts an explicitly supplied isolated non-default Pi installation"]
    fn nondefault_pi_starts_with_project_entry(){
        let root=PathBuf::from(std::env::var_os("PERCH_VERSION_TEST_ROOT").expect("explicit temporary artifact root"));
        assert_eq!(root.parent().unwrap().canonicalize().unwrap(),std::env::temp_dir().canonicalize().unwrap());
        assert!(root.file_name().unwrap().to_string_lossy().starts_with("perch-p8-version-"));
        crate::engine_profile::load(&root).unwrap();
        let file=fs::read_dir(root.join("resolved-profiles")).unwrap().next().unwrap().unwrap().path();
        let profile:crate::engine_profile::ResolvedProfile=serde_json::from_slice(&fs::read(file).unwrap()).unwrap();
        let package:serde_json::Value=serde_json::from_str(&profile.package).unwrap();
        assert_eq!(package["dependencies"]["@agegr/pi-web"],"0.9.2");
        assert!(installed(&root,"Pi",&profile.id));
        let project=root.join(format!("项目 空格-{}",uuid::Uuid::new_v4()));fs::create_dir(&project).unwrap();
        fs::create_dir(project.join(".git")).unwrap();
        let connection=ModelConnection{id:uuid::Uuid::new_v4().to_string(),name:"fixture".into(),provider:"test".into(),protocol:"openai-chat".into(),base_url:"http://127.0.0.1:9/v1".into(),default_model:"fixture".into(),models:vec![],has_key:true,revision:1};
        let instance=Instance{schema_version:1,id:uuid::Uuid::new_v4().to_string(),name:"Nondefault Pi".into(),engine:"Pi".into(),profile_id:profile.id.clone(),project_path:project.to_string_lossy().into(),connection_id:Some(connection.id.clone()),revision:1,created_at:0,updated_at:0};
        let mut recipe=crate::catalog::packs().remove(1);recipe.profile_id=profile.id.clone();
        crate::managed::apply(&root,&instance,&recipe,"nondefault startup verification").unwrap();
        let engines=Engines::default();
        let result=(||->Result<String>{
            engines.begin(&instance,"start")?;
            engines.execute(&root,&instance,Some((connection,"fake-version-test".into())),"start")?;
            let address=engines.open(&instance.id)?;
            if !health(&address){return Err(fail("TEST_HEALTH","Nondefault Pi health failed"));}
            let parsed=url::Url::parse(&address).map_err(|_|fail("TEST_URL","Invalid workspace URL"))?;
            if !parsed.query_pairs().any(|(key,value)|key=="cwd"&&value==instance.project_path){return Err(fail("TEST_PROJECT","New project URL did not preserve the complete Unicode/space path"));}
            Ok(address)
        })();
        engines.shutdown();
        let address=result.unwrap();assert!(!health(&address));
        assert!(project.join(".git").is_dir());
        println!("Nondefault Pi Web 0.9.2 started with exact cwd entry; stopped cleanly. Instance {}. No model requests.",instance.id);
    }
    #[test]
    fn failed_promotion_preserves_existing_artifacts() {
        let root = std::env::temp_dir().join(format!("perch-promote-{}", uuid::Uuid::new_v4()));
        let old = root.join("current");
        fs::create_dir_all(&old).unwrap();
        fs::write(old.join("evidence"), "old revision").unwrap();
        assert!(promote(&root.join("missing"), &old).is_err());
        assert_eq!(
            fs::read_to_string(old.join("evidence")).unwrap(),
            "old revision"
        );
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    #[ignore = "rebuilds a shared Pi package and Skill in isolated directories"]
    fn shared_pack_rebuilds_and_starts(){
        let fixture=PathBuf::from(std::env::var_os("PERCH_SHARE_PROFILE").expect("explicit profile fixture"));
        let skill=PathBuf::from(std::env::var_os("PERCH_SHARE_SKILL").expect("explicit imported Skill fixture"));
        let root=std::env::temp_dir().join(format!("perch-share-rebuild-{}",uuid::Uuid::new_v4()));
        let source=root.join("source");let destination=root.join("destination");fs::create_dir_all(&source).unwrap();fs::create_dir_all(&destination).unwrap();
        let mut profile:crate::engine_profile::ResolvedProfile=serde_json::from_slice(&fs::read(&fixture).unwrap()).unwrap();
        profile.id=format!("resolved-{}",uuid::Uuid::new_v4());let original_profile=profile.id.clone();crate::engine_profile::register(&source,profile).unwrap();
        let resource=source.join(".pi/skills/brand-guidelines");crate::local_skills::copy_atomic(&skill,&resource).unwrap();
        let digest=crate::local_skills::content_digest(&resource).unwrap();
        let mut recipe=crate::catalog::packs().remove(1);recipe.profile_id=original_profile.clone();recipe.description="Share rebuild verification".into();
        recipe.extensions.push(crate::catalog::ExtensionRef{resource_rules:Default::default(),disabled_resources:Vec::new(),id:"npm:pi-web-access".into(),version:"0.31.0".into(),enabled:true});
        let original=Instance{schema_version:1,id:uuid::Uuid::new_v4().to_string(),name:"Source local Skill".into(),engine:"Pi".into(),profile_id:original_profile.clone(),project_path:source.to_string_lossy().into(),connection_id:None,revision:1,created_at:0,updated_at:0};
        crate::managed::apply(&source,&original,&recipe,"source recipe").unwrap();
        let preview=crate::local_pack::preview(&source,&original).unwrap();assert_eq!(preview.skills.len(),1);
        let recipe=crate::local_pack::save(&source,&original,&preview.token).unwrap().recipe;
        let skill_id=recipe.extensions.iter().find(|item|crate::local_skills::valid_id(&item.id)).unwrap().id.clone();
        let manifest=crate::pack_drafts::export_bytes(&recipe).unwrap();let zip=crate::pack_archive::encode(&source,&recipe,&manifest).unwrap();
        let imported=crate::pack_archive::decode(&destination,&zip).unwrap();assert_ne!(imported.profile_id,original_profile);assert_eq!(imported.description,recipe.description);assert!(!imported.extensions.iter().any(|entry|entry.id==skill_id));
        let connection=ModelConnection{id:uuid::Uuid::new_v4().to_string(),name:"existing test connection".into(),provider:"test".into(),protocol:"openai-chat".into(),base_url:"http://127.0.0.1:9/v1".into(),default_model:"fixture".into(),models:vec![],has_key:true,revision:1};
        let instance=Instance{schema_version:1,id:uuid::Uuid::new_v4().to_string(),name:"Shared Pi rebuild".into(),engine:"Pi".into(),profile_id:imported.profile_id.clone(),project_path:destination.to_string_lossy().into(),connection_id:Some(connection.id.clone()),revision:1,created_at:0,updated_at:0};
        crate::managed::apply(&destination,&instance,&imported,"imported pack").unwrap();
        let engines=Engines::default();
        let result=(||->Result<()>{engines.begin(&instance,"install")?;engines.execute(&destination,&instance,None,"install")?;engines.begin(&instance,"start")?;engines.execute(&destination,&instance,Some((connection,"fake-share-test".into())),"start")?;assert!(health(&engines.open(&instance.id)?));Ok(())})();
        engines.shutdown();result.unwrap();
        assert_eq!(crate::local_skills::content_digest(&resource).unwrap(),digest);
        assert_eq!(crate::managed::recipe(&destination,&instance).unwrap(),imported);
        println!("Shared Pi package rebuilt and started: {}",root.display());
    }
    #[test]
    #[ignore = "starts the previously installed temporary DSH community bundle"]
    fn external_dsh_bundle_starts_in_isolated_instance(){
        let root=PathBuf::from(std::env::var_os("PERCH_DSH_BUNDLE_TEST_ROOT").expect("explicit test artifact root"));
        assert_eq!(root.parent().unwrap().canonicalize().unwrap(),std::env::temp_dir().canonicalize().unwrap());
        assert!(root.file_name().unwrap().to_string_lossy().starts_with("perch-p8-dsh-bundle-"));
        crate::engine_profile::load(&root).unwrap();
        let file=fs::read_dir(root.join("resolved-profiles")).unwrap().next().unwrap().unwrap().path();
        let profile:crate::engine_profile::ResolvedProfile=serde_json::from_slice(&fs::read(file).unwrap()).unwrap();
        let c=ModelConnection{id:uuid::Uuid::new_v4().to_string(),name:"test".into(),provider:"test".into(),protocol:"deepseek".into(),base_url:"http://127.0.0.1:9/v1".into(),default_model:"test".into(),models:vec![],has_key:true,revision:1};
        let i=Instance{schema_version:1,id:uuid::Uuid::new_v4().to_string(),name:"DSH bundle load".into(),engine:"DSH".into(),profile_id:profile.id.clone(),project_path:root.to_string_lossy().into(),connection_id:Some(c.id.clone()),revision:1,created_at:0,updated_at:0};
        let mut recipe=crate::catalog::packs().remove(0);recipe.profile_id=profile.id;recipe.extensions.push(crate::catalog::ExtensionRef{resource_rules:Default::default(),disabled_resources:Vec::new(),id:"dsh-npm:@morlay/session-branch".into(),version:"0.0.8".into(),enabled:true});
        crate::managed::apply(&root,&i,&recipe,"bundle verification").unwrap();
        let engines=Engines::default();
        let result=(||->Result<()>{engines.begin(&i,"start")?;engines.execute(&root,&i,Some((c,"fake-bundle-test".into())),"start")?;assert!(health(&engines.open(&i.id)?));Ok(())})();
        engines.shutdown();
        println!("DSH test instance {}",i.id);
        result.unwrap();
        let log=fs::read_to_string(root.join("instances").join(&i.id).join("logs/engine.log")).unwrap();
        assert!(log.contains("PERCH_BUNDLE_MODULE_LOADED: @morlay/session-branch"),"community module was not loaded: {log}");
    }
    #[test]
    #[ignore = "downloads locked DSH packages; explicit Windows integration run"]
    fn real_dsh_install_start_isolation_stop_restart() {
        let root = std::env::var_os("PERCH_P3_TEST_ROOT")
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                std::env::temp_dir().join(format!("perch-p3-{}", uuid::Uuid::new_v4()))
            });
        assert!(
            root.parent().unwrap().canonicalize().unwrap()
                == std::env::temp_dir().canonicalize().unwrap()
                && root
                    .file_name()
                    .unwrap()
                    .to_string_lossy()
                    .starts_with("perch-p3-")
        );
        fs::create_dir_all(root.join("项目 空格")).unwrap();
        let c = ModelConnection {
            id: uuid::Uuid::new_v4().to_string(),
            name: "测试连接".into(),
            provider: "test".into(),
            protocol: "deepseek".into(),
            base_url: "http://127.0.0.1:9/v1".into(),
            default_model: "test-model".into(),
            models: vec![],
            has_key: true,
            revision: 1,
        };
        let make = || Instance {
            schema_version: 1,
            id: uuid::Uuid::new_v4().to_string(),
            name: "测试实例".into(),
            engine: "DSH".into(),
            project_path: root.join("项目 空格").to_string_lossy().into(),
            connection_id: Some(c.id.clone()),
            profile_id: "dsh-0.1.5-rc.3".into(),
            revision: 1,
            created_at: 0,
            updated_at: 0,
        };
        let a = make();
        let b = make();
        let engines = Engines::default();
        let test = (|| -> Result<()> {
            engines.begin(&a, "start")?;
            engines.execute(
                &root,
                &a,
                Some((c.clone(), "fake-p3-only-secret".into())),
                "start",
            )?;
            let a_url = engines.open(&a.id)?;
            assert!(health(&a_url));
            assert!(installed(&root, &a.engine, &a.profile_id));
            engines.begin(&b, "start")?;
            engines.execute(
                &root,
                &b,
                Some((c.clone(), "fake-p3-only-secret".into())),
                "start",
            )?;
            let b_url = engines.open(&b.id)?;
            assert_ne!(a_url, b_url);
            assert!(engines.begin(&a, "start").is_err());
            for i in [&a, &b] {
                let home = root.join("instances").join(&i.id).join("agent-data");
                let settings = fs::read_to_string(home.join("settings.yaml"))?;
                assert!(settings.contains("PERCH_MODEL_KEY"));
                assert!(!settings.contains("fake-p3-only-secret"));
                let credentials = fs::read_to_string(home.join(".credentials.yaml"))?;
                assert!(!credentials.contains("fake-p3-only-secret"));
            }
            engines.stop(&a.id)?;
            assert!(!health(&a_url));
            assert!(health(&b_url));
            engines.begin(&a, "start")?;
            let mut rotated = c.clone();
            rotated.revision = 2;
            rotated.protocol = "openai-chat".into();
            engines.execute(
                &root,
                &a,
                Some((rotated, "replacement-fake-secret".into())),
                "start",
            )?;
            assert!(health(&engines.open(&a.id)?));
            let a_new = engines.open(&a.id)?;
            engines.shutdown();
            assert!(!health(&a_new));
            assert!(!health(&b_url));
            Ok(())
        })();
        if let Err(ref e) = test {
            eprintln!("{}: {}", e.code, e.message);
            for s in engines.slots.lock().unwrap().values() {
                for line in &s.output.lock().unwrap().lines {
                    eprintln!("{line}");
                }
            }
        }
        engines.shutdown();
        assert!(
            test.is_ok(),
            "integration failed; artifacts retained at {}",
            root.display()
        );
        // Keep test evidence out of user data; cleanup only this UUID temporary directory.
        let _ = fs::remove_dir_all(&root);
    }
    #[test]
    fn cancelling_before_launch_preserves_artifacts_and_discards_staging() {
        let root = std::env::temp_dir().join(format!("perch-cancel-{}", uuid::Uuid::new_v4()));
        let artifact = root.join("artifacts").join(engine_profile::PI.id);
        fs::create_dir_all(&artifact).unwrap();
        fs::write(artifact.join("keep"), "old artifact").unwrap();
        let instance = Instance {
            schema_version: 1,
            id: uuid::Uuid::new_v4().to_string(),
            name: "cancel".into(),
            engine: "Pi".into(),
            profile_id: engine_profile::PI.id.into(),
            project_path: root.to_string_lossy().into(),
            connection_id: None,
            revision: 1,
            created_at: 0,
            updated_at: 0,
        };
        let engines = Engines::default();
        engines.begin(&instance, "install").unwrap();
        engines.cancel(&instance.id).unwrap();
        assert_eq!(
            engines
                .execute(&root, &instance, None, "install")
                .unwrap_err()
                .code,
            "CANCELLED"
        );
        let staging = root
            .join("artifacts")
            .join(format!(".staging-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&staging).unwrap();
        fs::write(staging.join("partial"), "partial").unwrap();
        {
            let _guard = StagingCleanup(staging.clone());
        }
        assert!(!staging.exists());
        assert_eq!(
            fs::read_to_string(artifact.join("keep")).unwrap(),
            "old artifact"
        );
        engines.shutdown();
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    #[ignore = "starts and cancels a real npm installation"]
    fn cancelling_active_install_preserves_previous_and_removes_staging() {
        let root = std::env::temp_dir().join(format!("perch-cancel-live-{}", uuid::Uuid::new_v4()));
        let old = root.join("artifacts").join(engine_profile::PI.id);
        fs::create_dir_all(&old).unwrap();
        fs::write(old.join("keep"), "previous installation").unwrap();
        let instance = Instance {
            schema_version: 1,
            id: uuid::Uuid::new_v4().to_string(),
            name: "cancel live".into(),
            engine: "Pi".into(),
            profile_id: engine_profile::PI.id.into(),
            project_path: root.to_string_lossy().into(),
            connection_id: None,
            revision: 1,
            created_at: 0,
            updated_at: 0,
        };
        let engines = Arc::new(Engines::default());
        engines.begin(&instance, "install").unwrap();
        let worker = engines.clone();
        let directory = root.clone();
        let target = instance.clone();
        let task = thread::spawn(move || worker.execute(&directory, &target, None, "install"));
        let deadline = Instant::now();
        let mut attached = false;
        while deadline.elapsed() < Duration::from_secs(30) {
            if engines
                .slots
                .lock()
                .unwrap()
                .get(&instance.id)
                .is_some_and(|s| s.process.is_some())
            {
                attached = true;
                break;
            }
            if task.is_finished() {
                break;
            }
            thread::sleep(Duration::from_millis(50));
        }
        if attached {
            engines.cancel(&instance.id).unwrap();
        } else {
            engines.shutdown();
        }
        let result = task.join().unwrap();
        engines.shutdown();
        assert!(attached, "installer never attached");
        assert_eq!(result.unwrap_err().code, "CANCELLED");
        assert_eq!(
            fs::read_to_string(old.join("keep")).unwrap(),
            "previous installation"
        );
        assert!(!fs::read_dir(root.join("artifacts")).unwrap().any(|e| e
            .unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with(".staging-")));
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    #[ignore = "real locked Pi installation and Windows processes"]
    fn four_engine_parallel_runtime() {
        let root = std::env::temp_dir().join(format!("perch-p4-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(root.join("项目 空格")).unwrap();
        let c = ModelConnection {
            id: uuid::Uuid::new_v4().to_string(),
            name: "Test".into(),
            provider: "test".into(),
            protocol: "openai-chat".into(),
            base_url: "http://127.0.0.1:9/v1".into(),
            default_model: "test-model".into(),
            models: vec![],
            has_key: true,
            revision: 1,
        };
        let make = || {
            let project = root
                .join("项目 空格")
                .join(uuid::Uuid::new_v4().to_string());
            fs::create_dir_all(&project).unwrap();
            Instance {
                schema_version: 1,
                id: uuid::Uuid::new_v4().to_string(),
                name: "Pi test".into(),
                engine: "Pi".into(),
                project_path: project.to_string_lossy().into(),
                connection_id: Some(c.id.clone()),
                profile_id: engine_profile::PI.id.into(),
                revision: 1,
                created_at: 0,
                updated_at: 0,
            }
        };
        let a = make();
        let b = make();
        let mut d = make();
        let mut e = make();
        for instance in [&mut d, &mut e] {
            instance.engine = "DSH".into();
            instance.profile_id = engine_profile::DSH.id.into();
        }
        let engines = Engines::default();
        let result = (|| -> Result<()> {
            for instance in [&a, &b, &d, &e] {
                let selected = crate::catalog::packs()
                    .into_iter()
                    .find(|p| p.engine == instance.engine)
                    .unwrap();
                crate::managed::apply(&root, instance, &selected, "integration install")?;
                engines.begin(instance, "start")?;
                engines.execute(
                    &root,
                    instance,
                    Some((c.clone(), "fake-p4-secret".into())),
                    "start",
                )?;
                let settings = fs::read_to_string(root.join("instances").join(&instance.id).join(
                    if instance.engine == "Pi" {
                        "agent-data/models.json"
                    } else {
                        "agent-data/settings.yaml"
                    },
                ))?;
                assert!(!settings.contains("fake-p4-secret"));
                assert!(settings.contains("PERCH_MODEL_KEY"));
            }
            for instance in [&a, &b, &d, &e] {
                assert!(
                    engines
                        .slots
                        .lock()
                        .unwrap()
                        .get(&instance.id)
                        .unwrap()
                        .output
                        .lock()
                        .unwrap()
                        .lines
                        .iter()
                        .any(|line| line.contains("PERCH_EXTENSION_READY")),
                    "extension not loaded: {}",
                    instance.engine
                );
            }
            let a_url = engines.open(&a.id)?;
            let b_url = engines.open(&b.id)?;
            assert_ne!(a_url, b_url);
            let d_url = engines.open(&d.id)?;
            let e_url = engines.open(&e.id)?;
            let ports = [&a_url, &b_url, &d_url, &e_url]
                .map(|u| url::Url::parse(u).unwrap().port().unwrap());
            assert_eq!(
                ports
                    .into_iter()
                    .collect::<std::collections::HashSet<_>>()
                    .len(),
                4
            );
            engines.begin_stop(&a.id)?;
            engines.stop(&a.id)?;
            assert!(!health(&a_url));
            assert!(health(&b_url));
            assert!(health(&d_url));
            assert!(health(&e_url));
            let mut missing = make();
            missing.project_path = root.join("missing-project").to_string_lossy().into();
            engines.begin(&missing, "start")?;
            assert!(engines
                .execute(
                    &root,
                    &missing,
                    Some((c.clone(), "fake-p4-secret".into())),
                    "start"
                )
                .is_err());
            assert!(health(&b_url));
            assert!(health(&d_url));
            assert!(health(&e_url));
            Ok(())
        })();
        if let Err(error) = &result {
            eprintln!("Pi runtime error: {:?}; retained {}", error, root.display());
            for slot in engines.slots.lock().unwrap().values() {
                eprintln!("{:?}", slot.output.lock().unwrap().lines);
            }
        }
        engines.shutdown();
        assert!(result.is_ok());
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn pi_empty_workspace_uses_cwd_entry() {
        use std::net::TcpListener;
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}/", listener.local_addr().unwrap());
        let server = thread::spawn(move || {
            for body in [r#"{"sessions":[]}"#, r#"{"cwd":"C:\\项目 空格"}"#] {
                let (mut stream, _) = listener.accept().unwrap();
                let mut request = [0u8; 4096];
                let count = stream.read(&mut request).unwrap();
                let request = String::from_utf8_lossy(&request[..count]);
                assert!(!request.contains("/api/agent/new"));
                write!(stream,"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",body.len(),body).unwrap();
            }
        });
        let link = pi_workspace(&base, r"C:\项目 空格").unwrap();
        let url = url::Url::parse(&link).unwrap();
        assert_eq!(
            url.query_pairs().collect::<Vec<_>>(),
            vec![("cwd".into(), r"C:\项目 空格".into())]
        );
        server.join().unwrap();
    }
    #[test]
    fn logs_never_expose_known_credentials() {
        assert!(!sanitize("error test-secret trailing", "test-secret").contains("test-secret"));
        assert!(!sanitize("dsh web: http://127.0.0.1:123/?token=secret", "").contains("secret"));
    }
}
