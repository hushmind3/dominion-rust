#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
mod engine;

use engine::EngineConnection;
use std::sync::Mutex;
use tauri::{
    menu::{Menu, MenuItem, PredefinedMenuItem, Submenu},
    Manager, WebviewUrl, WebviewWindow, WebviewWindowBuilder,
};

struct DesktopState {
    engine: Mutex<EngineConnection>,
    origin: String,
    context_menu: Menu<tauri::Wry>,
    devtools: bool,
}

#[tauri::command]
fn native_context_menu(
    window: WebviewWindow,
    state: tauri::State<'_, DesktopState>,
) -> Result<(), String> {
    if window.label() != "main"
        || window
            .url()
            .map_err(|e| e.to_string())?
            .origin()
            .ascii_serialization()
            != state.origin
    {
        return Err("Native menus are available only in DOMINION's own window.".into());
    }
    window
        .popup_menu(&state.context_menu)
        .map_err(|e| e.to_string())
}
fn context_menu(app: &tauri::AppHandle, devtools: bool) -> tauri::Result<Menu<tauri::Wry>> {
    let menu = Menu::with_items(
        app,
        &[
            &PredefinedMenuItem::cut(app, Some("잘라내기"))?,
            &PredefinedMenuItem::copy(app, Some("복사"))?,
            &PredefinedMenuItem::paste(app, Some("붙여넣기"))?,
            &PredefinedMenuItem::select_all(app, Some("전체 선택"))?,
            &PredefinedMenuItem::separator(app)?,
            &MenuItem::with_id(app, "back", "뒤로", true, None::<&str>)?,
            &MenuItem::with_id(app, "forward", "앞으로", true, None::<&str>)?,
            &MenuItem::with_id(app, "reload", "새로고침", true, Some("CmdOrCtrl+R"))?,
        ],
    )?;
    if devtools {
        menu.append(&MenuItem::with_id(
            app,
            "inspect",
            "개발자 도구",
            true,
            None::<&str>,
        )?)?;
    }
    Ok(menu)
}
fn application_menu(app: &tauri::AppHandle, devtools: bool) -> tauri::Result<Menu<tauri::Wry>> {
    let edit = Submenu::with_items(
        app,
        "편집",
        true,
        &[
            &PredefinedMenuItem::cut(app, Some("잘라내기"))?,
            &PredefinedMenuItem::copy(app, Some("복사"))?,
            &PredefinedMenuItem::paste(app, Some("붙여넣기"))?,
            &PredefinedMenuItem::select_all(app, Some("전체 선택"))?,
        ],
    )?;
    let view = Submenu::with_items(
        app,
        "보기",
        true,
        &[
            &MenuItem::with_id(app, "back", "뒤로", true, None::<&str>)?,
            &MenuItem::with_id(app, "forward", "앞으로", true, None::<&str>)?,
            &MenuItem::with_id(app, "reload", "새로고침", true, Some("CmdOrCtrl+R"))?,
        ],
    )?;
    if devtools {
        view.append(&MenuItem::with_id(
            app,
            "inspect",
            "개발자 도구",
            true,
            Some("CmdOrCtrl+Shift+I"),
        )?)?;
    }
    let application = Submenu::with_items(
        app,
        "DOMINION",
        true,
        &[
            &PredefinedMenuItem::about(app, Some("DOMINION 정보"), None)?,
            &PredefinedMenuItem::separator(app)?,
            &PredefinedMenuItem::quit(app, Some("DOMINION 종료"))?,
        ],
    )?;
    Menu::with_items(app, &[&application, &edit, &view])
}
fn menu_event(app: &tauri::AppHandle, event: tauri::menu::MenuEvent) {
    let Some(window) = app.get_webview_window("main") else {
        return;
    };
    match event.id().as_ref() {
        "back" => {
            let _ = window.eval("history.back()");
        }
        "forward" => {
            let _ = window.eval("history.forward()");
        }
        "reload" => {
            let _ = window.reload();
        }
        "inspect" if app.state::<DesktopState>().devtools => window.open_devtools(),
        _ => {}
    }
}
fn main() {
    let app=tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app,_,_| {
            if let Some(window)=app.get_webview_window("main") {let _=window.show();let _=window.unminimize();let _=window.set_focus();}
        }))
        .invoke_handler(tauri::generate_handler![native_context_menu])
        .setup(|app| {
            let resources=app.path().resource_dir()?;
            #[cfg(debug_assertions)]
            let resources=if resources.join("web/index.html").is_file() {resources} else {std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("resources")};
            let data=std::env::var_os("DOMINION_DESKTOP_DATA").map(std::path::PathBuf::from).unwrap_or(app.path().app_data_dir()?);
            let connection=EngineConnection::connect(&resources,&data).map_err(std::io::Error::other)?;
            let origin=connection.origin.clone();
            let devtools=std::env::var("DOMINION_DESKTOP_DEVTOOLS").ok().as_deref()==Some("1");
            let context_menu=context_menu(app.handle(),devtools)?;
            let menu=application_menu(app.handle(),devtools)?;
            app.set_menu(menu)?;
            let capability=serde_json::json!({"identifier":"owned-dominion-origin","windows":["main"],"remote":{"urls":[format!("{origin}/*")]},"permissions":["allow-native-context-menu"]});
            app.add_capability(serde_json::to_string(&capability)?.as_str())?;
            let script=include_str!("desktop-init.js").replace("__DOMINION_ORIGIN__",&serde_json::to_string(&origin)?);
            app.manage(DesktopState{engine:Mutex::new(connection),origin:origin.clone(),context_menu,devtools});
            let allowed_origin=origin.clone();
            let window=WebviewWindowBuilder::new(app,"main",WebviewUrl::External(origin.parse()?))
                .title("DOMINION").inner_size(1320.,860.).min_inner_size(840.,600.).center().prevent_overflow().devtools(devtools)
                .initialization_script(&script)
                .on_navigation(move|url|url.origin().ascii_serialization()==allowed_origin)
                .on_new_window(|_,_|tauri::webview::NewWindowResponse::Deny)
                .build()?;
            #[cfg(target_os="macos")]
            {
                let hidden_window=window.clone();
                window.on_window_event(move|event|if let tauri::WindowEvent::CloseRequested{api,..}=event {api.prevent_close();let _=hidden_window.hide();});
            }
            app.on_menu_event(menu_event);
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("DOMINION native desktop could not start");
    app.run(|app, event| match event {
        tauri::RunEvent::Exit => {
            if let Some(state) = app.try_state::<DesktopState>() {
                state.engine.lock().unwrap().stop();
            }
        }
        #[cfg(target_os = "macos")]
        tauri::RunEvent::Reopen { .. } => {
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.show();
                let _ = window.set_focus();
            }
        }
        _ => {}
    });
}
