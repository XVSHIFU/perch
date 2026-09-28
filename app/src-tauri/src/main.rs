#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
mod catalog_seed;
mod source_browser;
mod github_skills;
mod pack_archive;
mod resource_catalog;
mod pi_catalog;
mod resource_readme;
mod topic_catalog;
mod pack_drafts;
mod local_skills;
mod local_pack;
mod artifact_cache;
mod catalog;
mod credentials;
mod data_commands;
mod engine;
mod engine_profile;
mod managed;
mod model_catalog;
mod owned_process;
mod store;
mod version_catalog;
use std::{
    collections::HashMap,
    io::{Read, Write},
    net::TcpListener,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    thread,
    time::Duration,
};
use tauri::{
    menu::{Menu, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    Emitter, Manager,
};
use tauri_plugin_opener::OpenerExt;

struct Service {
    url: String,
    stop: Arc<AtomicBool>,
    worker: Option<thread::JoinHandle<()>>,
}
impl Service {
    fn shutdown(mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(h) = self.worker.take() {
            let _ = h.join();
        }
    }
}
#[derive(Default)]
struct Services(Mutex<HashMap<String, Service>>);
fn stop_all_services(state: &Services) -> Result<(), String> {
    let services: Vec<Service> = state
        .0
        .lock()
        .map_err(|_| "服务状态不可用")?
        .drain()
        .map(|(_, s)| s)
        .collect();
    for s in services {
        s.shutdown();
    }
    Ok(())
}
fn reveal(app: &tauri::AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.show();
        let _ = w.unminimize();
        let _ = w.set_focus();
    }
}
#[tauri::command]
fn window_action(
    app: tauri::AppHandle,
    action: String,
    state: tauri::State<'_, Services>,
) -> Result<(), String> {
    let w = app.get_webview_window("main").ok_or("窗口不存在")?;
    match action.as_str() {
        "minimize" => w.minimize().map_err(|e| e.to_string()),
        "maximize" => if w.is_maximized().map_err(|e| e.to_string())? {
            w.unmaximize()
        } else {
            w.maximize()
        }
        .map_err(|e| e.to_string()),
        "hide" => w.hide().map_err(|e| e.to_string()),
        "quit" => {
            app.state::<engine::Engines>().shutdown();
            stop_all_services(&state)?;
            app.exit(0);
            Ok(())
        }
        _ => Err("不支持的窗口操作".into()),
    }
}
#[tauri::command]
fn start_demo(id: String, state: tauri::State<'_, Services>) -> Result<String, String> {
    start_demo_service(id, &state)
}
fn start_demo_service(id: String, state: &Services) -> Result<String, String> {
    if id.is_empty() || id.len() > 80 || !id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
    {
        return Err("实例标识不正确".into());
    }
    let mut services = state.0.lock().map_err(|_| "服务状态不可用")?;
    if let Some(s) = services.get(&id) {
        return Ok(s.url.clone());
    }
    if services.len() >= 12 {
        return Err("样机最多同时启动 12 个演示服务".into());
    }
    let listener = TcpListener::bind("127.0.0.1:0").map_err(|e| e.to_string())?;
    listener.set_nonblocking(true).map_err(|e| e.to_string())?;
    let token = uuid::Uuid::new_v4().to_string();
    let route = format!("/{token}/");
    let url = format!(
        "http://127.0.0.1:{}{route}",
        listener.local_addr().map_err(|e| e.to_string())?.port()
    );
    let stop = Arc::new(AtomicBool::new(false));
    let flag = stop.clone();
    let worker = thread::spawn(move || {
        while !flag.load(Ordering::Acquire) {
            match listener.accept() {
                Ok((mut stream, _)) => {
                    let _ = stream.set_read_timeout(Some(Duration::from_millis(300)));
                    let _ = stream.set_write_timeout(Some(Duration::from_millis(300)));
                    let mut buf = [0u8; 4096];
                    if let Ok(n) = stream.read(&mut buf) {
                        let req = String::from_utf8_lossy(&buf[..n]);
                        let valid = req
                            .lines()
                            .next()
                            .map(|line| line.starts_with(&format!("GET {route} HTTP/")))
                            .unwrap_or(false);
                        let (status, body) = if valid {
                            ("200 OK", include_str!("demo.html"))
                        } else {
                            ("404 Not Found", "Not found")
                        };
                        let response = format!("HTTP/1.1 {status}\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nCache-Control: no-store\r\nX-Content-Type-Options: nosniff\r\nContent-Security-Policy: default-src 'none'; img-src data:; style-src 'unsafe-inline'; frame-ancestors 'none'\r\nConnection: close\r\n\r\n{body}", body.len());
                        let _ = stream.write_all(response.as_bytes());
                    }
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    thread::sleep(Duration::from_millis(35))
                }
                Err(_) => break,
            }
        }
    });
    services.insert(
        id,
        Service {
            url: url.clone(),
            stop,
            worker: Some(worker),
        },
    );
    Ok(url)
}
#[tauri::command]
fn open_demo(
    app: tauri::AppHandle,
    id: String,
    state: tauri::State<'_, Services>,
) -> Result<(), String> {
    let url = state
        .0
        .lock()
        .map_err(|_| "服务状态不可用")?
        .get(&id)
        .ok_or("请先启动演示服务")?
        .url
        .clone();
    app.opener()
        .open_url(url, None::<&str>)
        .map_err(|e| e.to_string())
}
#[tauri::command]
fn stop_demo(id: String, state: tauri::State<'_, Services>) -> Result<(), String> {
    stop_demo_service(id, &state)
}
fn stop_demo_service(id: String, state: &Services) -> Result<(), String> {
    let service = state.0.lock().map_err(|_| "服务状态不可用")?.remove(&id);
    if let Some(s) = service {
        s.shutdown();
    }
    Ok(())
}
#[tauri::command]
fn stop_all(state: tauri::State<'_, Services>) -> Result<(), String> {
    stop_all_services(&state)
}
#[tauri::command]
fn open_reference(app: tauri::AppHandle, url: String) -> Result<(), String> {
    if ![
        "https://github.com/agegr/pi-web",
        "https://github.com/openma-ai/dsh-agents-plugins",
    ]
    .contains(&url.as_str())
    {
        return Err("来源链接未列入目录".into());
    }
    app.opener()
        .open_url(url, None::<&str>)
        .map_err(|e| e.to_string())
}
fn resource_source_url(address:&str)->Result<url::Url,String>{
    let url=url::Url::parse(address).map_err(|_|"资源来源地址无效")?;
    if url.scheme()!="https"||url.host_str().is_none()||!url.username().is_empty()||url.password().is_some(){return Err("资源来源只允许不含登录凭据的 HTTPS 网页".into());}
    Ok(url)
}
#[tauri::command]
fn open_resource_source(app:tauri::AppHandle,address:String)->Result<(),String>{
    let url=resource_source_url(&address)?;
    app.opener().open_url(url.as_str(),None::<&str>).map_err(|_|"无法打开默认浏览器，请复制来源地址打开".into())
}
fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            reveal(app)
        }))
        .plugin(tauri_plugin_opener::init())
        .manage(Services::default())
        .manage(source_browser::BrowserState::default())
        .manage(engine::Engines::default())
        .invoke_handler(tauri::generate_handler![
            window_action,
            start_demo,
            open_demo,
            stop_demo,
            stop_all,
            open_reference,
            data_commands::data_snapshot,
            data_commands::export_diagnostics,
            data_commands::package_catalog,
            local_skills::local_skills,
            github_skills::github_skill_source,
            github_skills::github_skill_sources,
            github_skills::set_github_skill_source_enabled,
            github_skills::import_github_skill,
            local_skills::import_local_skill,
            local_skills::import_skill_zip,
            local_skills::preview_skill_zip_url,
            local_skills::import_downloaded_skill,
            pack_drafts::pack_drafts,
            pack_drafts::save_pack_draft,
            data_commands::save_instance_pack,
            data_commands::preview_local_pack,
            data_commands::save_local_pack,
            pack_drafts::export_pack,
            pack_drafts::import_pack,
            resource_catalog::resource_catalog,
            resource_catalog::resource_source_preferences,
            resource_catalog::set_resource_source_enabled,
            resource_catalog::workshop_catalog,
            resource_catalog::preview_workshop_asset,
            resource_catalog::download_workshop_asset,
            resource_catalog::cancel_workshop_download,
            resource_catalog::workshop_asset_reference,
            resource_catalog::prepare_workshop_resources,
            data_commands::install_workshop_asset,
            topic_catalog::dsh_topic_catalog,
            pi_catalog::pi_official_catalog,
            topic_catalog::inspect_topic_package,
            resource_catalog::inspect_resource,
            resource_catalog::resolve_resource_pack,
            version_catalog::version_catalog,
            version_catalog::resolve_version_plan,
            version_catalog::lock_version_plan,
            version_catalog::cancel_version_plan,
            engine_profile::resolved_profiles,
            data_commands::artifact_cache,
            data_commands::clone_instance,
            data_commands::clone_version_trial,
            data_commands::instance_package,
            data_commands::change_package,
            data_commands::pi_package_drift,
            open_resource_source,
            resource_readme::resource_readme,
            source_browser::source_browser_open,
            source_browser::source_browser_control,
            data_commands::adopt_pi_package_selection,
            data_commands::restore_pi_package_selection,
            data_commands::install_package,
            data_commands::restore_package,
            data_commands::save_instance,
            data_commands::create_pack_instance,
            data_commands::save_connection,
            data_commands::delete_connection,
            data_commands::delete_instance,
            data_commands::removed_instances,
            data_commands::restore_removed_instance,
            data_commands::pick_project,
            data_commands::fetch_models,
            data_commands::engine_action,
            data_commands::cancel_engine,
            engine::engine_open,
            store::import_manifest
        ])
        .setup(|app| {
            let mut data = app
                .path()
                .app_data_dir()
                .map_err(|_| store::Failure::new("DATA_PATH", "无法定位应用数据目录"))
                .map(|root| root.join("workspace"));
            let workspace_lock = match data.as_ref() {
                Ok(root) => match data_commands::lock_workspace(root) {
                    Ok(file) => Some(file),
                    Err(e) => {
                        data = Err(e);
                        None
                    }
                },
                Err(_) => None,
            };
            if let Ok(root) = &data {
                engine_profile::load(root).map_err(|e| std::io::Error::other(e.message))?;
            }
            app.manage(data_commands::DataState {
                _workspace_lock: workspace_lock,
                root: data,
                store: Mutex::new(None),
            });
            let show = MenuItem::with_id(app, "show", "打开栖点 Studio", true, None::<&str>)?;
            let running =
                MenuItem::with_id(app, "running", "查看运行中的工作台", true, None::<&str>)?;
            let stop = MenuItem::with_id(app, "stop", "停止运行中的工作台", true, None::<&str>)?;
            let quit = MenuItem::with_id(app, "quit", "停止服务并退出", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&show, &running, &stop, &quit])?;
            TrayIconBuilder::new()
                .icon(tauri::include_image!("icons/32x32.png"))
                .tooltip("栖点 Studio · 桌面样机")
                .menu(&menu)
                .show_menu_on_left_click(false)
                .on_menu_event(|app, event| match event.id.as_ref() {
                    "show" => reveal(app),
                    "running" => {
                        reveal(app);
                        let _ = app.emit("desktop-running-requested", ());
                    }
                    "stop" => {
                        let handle = app.clone();
                        std::thread::spawn(move || {
                            handle.state::<engine::Engines>().stop_running();
                            let _ = stop_all_services(&handle.state::<Services>());
                            let _ = handle.emit("desktop-all-stopped", ());
                        });
                    }
                    "quit" => {
                        app.state::<engine::Engines>().shutdown();
                        let _ = stop_all_services(&app.state::<Services>());
                        app.exit(0);
                    }
                    _ => (),
                })
                .on_tray_icon_event(|tray, event| {
                    if let TrayIconEvent::Click {
                        button: MouseButton::Left,
                        button_state: MouseButtonState::Up,
                        ..
                    } = event
                    {
                        reveal(tray.app_handle());
                    }
                })
                .build(app)?;
            Ok(())
        })
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = window.emit("desktop-close-requested", ());
            }
        })
        .build(tauri::generate_context!())
        .expect("Unable to start Perch Studio")
        .run(|app, event| {
            if let tauri::RunEvent::Exit = event {
                app.state::<engine::Engines>().shutdown();
                let _ = stop_all_services(&app.state::<Services>());
            }
        });
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpStream;

    fn endpoint(url: &str) -> (&str, String) {
        let (address, path) = url
            .strip_prefix("http://")
            .unwrap()
            .split_once('/')
            .unwrap();
        (address, format!("/{path}"))
    }

    fn request(url: &str, valid_route: bool) -> String {
        let (address, path) = endpoint(url);
        let mut stream = TcpStream::connect(address).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        let route = if valid_route { path.as_str() } else { "/" };
        write!(
            stream,
            "GET {route} HTTP/1.1\r\nHost: {address}\r\nConnection: close\r\n\r\n"
        )
        .unwrap();
        let mut response = String::new();
        stream.read_to_string(&mut response).unwrap();
        response
    }

    #[test]
    fn demo_lifecycle_isolated_and_releases_ports() {
        let services = Services::default();
        assert!(start_demo_service("../invalid".into(), &services).is_err());
        assert!(services.0.lock().unwrap().is_empty());
        let first = start_demo_service("first".into(), &services).unwrap();
        let second = start_demo_service("second".into(), &services).unwrap();
        assert_eq!(
            first,
            start_demo_service("first".into(), &services).unwrap()
        );
        assert_ne!(endpoint(&first).0, endpoint(&second).0);
        let response = request(&first, true);
        assert!(response.starts_with("HTTP/1.1 200 OK"));
        assert!(response.contains("不是 DSH / Pi 本体"));
        assert!(response.contains("img-src data:"));
        assert!(request(&first, false).starts_with("HTTP/1.1 404 Not Found"));
        stop_demo_service("first".into(), &services).unwrap();
        assert!(TcpStream::connect(endpoint(&first).0).is_err());
        assert!(request(&second, true).starts_with("HTTP/1.1 200 OK"));
        stop_demo_service("first".into(), &services).unwrap();
        stop_all_services(&services).unwrap();
        assert!(TcpStream::connect(endpoint(&second).0).is_err());
        assert!(services.0.lock().unwrap().is_empty());
    }
}
