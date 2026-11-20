#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
// Tauri 2 shell; all agent work belongs to the single Rust HTTP sidecar.
use std::{io::{Read,Write},net::{SocketAddr,TcpStream},
    time::{Duration,Instant}};
use tauri::{Manager,WebviewUrl,WebviewWindowBuilder};
use tauri_plugin_shell::{ShellExt,process::CommandEvent};
const ENDPOINT:&str="127.0.0.1:4317";
fn healthy(token:&str,nonce:&str)->bool{
    let address:SocketAddr=ENDPOINT.parse().expect("constant loopback address");
    let Ok(mut socket)=TcpStream::connect_timeout(&address,Duration::from_millis(250))
        else{return false};
    let _=socket.set_read_timeout(Some(Duration::from_millis(350)));
    let request=format!("GET /api/v1/health HTTP/1.1\r\nHost: {ENDPOINT}\r\nX-Dominion-Token: {token}\r\nConnection: close\r\n\r\n");
    if socket.write_all(request.as_bytes()).is_err(){return false;}
    let mut data=[0u8;4096];
    let Ok(count)=socket.read(&mut data)else{return false};
    let Ok(text)=std::str::from_utf8(&data[..count])else{return false};
    text.starts_with("HTTP/1.1 200")&&text.contains(&format!("\"launchNonce\":\"{nonce}\""))
}
fn await_backend(token:&str,nonce:&str)->Result<(),String>{
    let start=Instant::now();
    while start.elapsed()<Duration::from_secs(12){
        if healthy(token,nonce){return Ok(());}
        std::thread::sleep(Duration::from_millis(120));
    }
    Err("native sidecar did not pass authenticated localhost readiness check".into())
}
fn main(){
    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .setup(|app|{
            let data=app.path().app_data_dir()?;
            let workspace=data.join("workspace");
            std::fs::create_dir_all(&workspace)?;
            let home=app.path().resource_dir()?;
            if !home.join("web/dist/index.html").is_file(){
                return Err("packaged React web/dist missing".into());
            }
            let token=format!("{}{}{}",uuid::Uuid::new_v4().simple(),
                uuid::Uuid::new_v4().simple(),uuid::Uuid::new_v4().simple());
            let nonce=uuid::Uuid::new_v4().simple().to_string();
            let (mut events,_child)=app.shell().sidecar("dominion")?
                .args(["--home".to_owned(),home.to_string_lossy().to_string()])
                .env("DOMINION_BIND",ENDPOINT)
                .env("DOMINION_ACCESS_TOKEN",&token)
                .env("DOMINION_LAUNCH_NONCE",&nonce)
                .env("DOMINION_DATA_DIR",data.as_os_str())
                .env("DOMINION_WORKSPACE_DIR",workspace.as_os_str())
                .spawn()?;
            tauri::async_runtime::spawn(async move{
                while let Some(event)=events.recv().await{
                    if let CommandEvent::Terminated(status)=event{
                        eprintln!("DOMINION backend exited: {:?}",status.code);
                        break;
                    }
                }
            });
            await_backend(&token,&nonce).map_err(std::io::Error::other)?;
            let url=tauri::Url::parse("http://127.0.0.1:4317/")?;
            let secret=serde_json::to_string(&token)?;
            let bootstrap=format!("if(location.origin==='http://127.0.0.1:4317'){{Object.defineProperty(globalThis,'__DOMINION_BOOTSTRAP_TOKEN__',{{value:{secret},writable:false,configurable:false}});}}");
            WebviewWindowBuilder::new(app,"main",WebviewUrl::External(url))
                .title("DOMINION").inner_size(1280.0,840.0)
                .initialization_script(bootstrap).build()?;
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("Tauri desktop initialization failed");
}
