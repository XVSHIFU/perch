//! Remote pages have their own WebView and no local application capabilities.
use serde::{Deserialize, Serialize};
use std::sync::Mutex;
use tauri::{Emitter, Manager, LogicalPosition, LogicalSize, WebviewUrl};

#[derive(Default)]
pub struct BrowserState(pub Mutex<Option<String>>);
#[derive(Clone, Deserialize)]
pub struct Bounds { x:f64, y:f64, width:f64, height:f64 }
#[derive(Clone, Serialize)]
#[serde(rename_all="camelCase")]
struct Update { token:String, kind:String, value:String }
fn emit(app:&tauri::AppHandle,token:&str,kind:&str,value:&str){
    let _=app.emit_to(tauri::EventTarget::webview("main"),"source-browser",Update{token:token.into(),kind:kind.into(),value:value.into()});
}
fn local(caller:&tauri::Webview)->Result<(),String>{
    if caller.label()!="main"{return Err("此操作仅供应用使用".into());} Ok(())
}
fn address(raw:&str)->Result<url::Url,String>{
    let url=url::Url::parse(raw).map_err(|_|"网页地址无效")?;
    if url.scheme()!="https"||url.host_str().is_none()||!url.username().is_empty()||url.password().is_some(){return Err("请使用 HTTPS 网页地址".into());} Ok(url)
}
fn rect(bounds:&Bounds)->Result<tauri::Rect,String>{
    if [bounds.x,bounds.y,bounds.width,bounds.height].iter().any(|n|!n.is_finite())||bounds.x<0.||bounds.y<0.||bounds.width<1.||bounds.height<1.{return Err("网页区域尚未就绪".into());}
    Ok(tauri::Rect{position:LogicalPosition::new(bounds.x,bounds.y).into(),size:LogicalSize::new(bounds.width,bounds.height).into()})
}
#[tauri::command]
pub async fn source_browser_open(app:tauri::AppHandle,webview:tauri::Webview,token:String,url:String,bounds:Bounds)->Result<(),String>{
    local(&webview)?;
    let target=address(&url)?;let area=rect(&bounds)?;
    // Serialize with close; a stale drawer cannot close a newly opened page.
    tauri::async_runtime::spawn_blocking(move||{
        let state=app.state::<BrowserState>();let mut current=state.0.lock().map_err(|_|"网页状态不可用")?;
        if let Some(old)=app.get_webview("source-browser"){old.close().map_err(|e|e.to_string())?;}
        let nav_app=app.clone();let nav_token=token.clone();
        let load_app=app.clone();let load_token=token.clone();
        let title_app=app.clone();let title_token=token.clone();
        let popup_app=app.clone();
        let builder=tauri::webview::WebviewBuilder::new("source-browser",WebviewUrl::External(target)).focused(false).incognito(true).zoom_hotkeys_enabled(false)
            .on_download(|_,_|false)
            .initialization_script("document.addEventListener('keydown',e=>{if(e.key==='Escape'){e.preventDefault();e.stopImmediatePropagation();location.href='perch-source://close';}},true);")
            .on_navigation(move|url|{
                if url.scheme()=="perch-source"&&url.host_str()==Some("close"){emit(&nav_app,&nav_token,"close","");return false;}
                let allowed=address(url.as_str()).is_ok();if allowed{emit(&nav_app,&nav_token,"navigate",url.as_str());} allowed
            })
            .on_new_window(move|url,_|{
                if address(url.as_str()).is_ok(){let h=popup_app.clone();let _=popup_app.run_on_main_thread(move||{if let Some(view)=h.get_webview("source-browser"){let _=view.navigate(url);}});}
                tauri::webview::NewWindowResponse::Deny
            })
            .on_document_title_changed(move|_,title|emit(&title_app,&title_token,"title",&title))
            .on_page_load(move|view,page|{
                emit(&load_app,&load_token,match page.event(){tauri::webview::PageLoadEvent::Started=>"loading",tauri::webview::PageLoadEvent::Finished=>"loaded"},page.url().as_str());
                if matches!(page.event(),tauri::webview::PageLoadEvent::Finished){
                    let h=load_app.clone();let t=load_token.clone();
                    let _=view.eval_with_callback("location.protocol === 'chrome-error:' || location.protocol === 'edge-error:'",move|value|{if value=="true"{emit(&h,&t,"error","页面暂时无法连接，请刷新或在浏览器中打开。");}});
                }
            });
        app.get_window("main").ok_or("应用窗口已关闭")?.add_child(builder,area.position,area.size).map_err(|e|e.to_string())?;
        *current=Some(token);Ok(())
    }).await.map_err(|_|"网页打开任务中断")?
}
#[tauri::command]
pub async fn source_browser_control(app:tauri::AppHandle,webview:tauri::Webview,token:String,action:String,bounds:Option<Bounds>)->Result<(),String>{
    local(&webview)?;
    tauri::async_runtime::spawn_blocking(move||{
        let state=app.state::<BrowserState>();let mut current=state.0.lock().map_err(|_|"网页状态不可用")?;
        if current.as_deref()!=Some(&token){return Ok(());}
        let Some(view)=app.get_webview("source-browser") else{return Ok(());};
        let result=match action.as_str(){
            "bounds"=>view.set_bounds(rect(&bounds.ok_or("缺少网页区域")?)?),
            "hide"=>view.hide(),"show"=>view.show(),"back"=>view.eval("history.back()"),
            "reload"=>view.reload(),"close"=>{*current=None;view.close()},
            _=>return Err("网页操作无效".into())
        };result.map_err(|e|e.to_string())
    }).await.map_err(|_|"网页操作中断")?
}
