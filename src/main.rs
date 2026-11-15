//! Thin HTTP adapter. The unchanged main UI uses --home / --open and port 4317.
use dominion::{Dominion,Settings};
use std::{error::Error,path::PathBuf,process::Command};
use tracing_subscriber::EnvFilter;

struct Launch { home:Option<PathBuf>, open:bool, help:bool }
fn args()->Result<Launch,Box<dyn Error>>{
    let mut out=Launch{home:None,open:false,help:false};
    let mut parts=std::env::args_os().skip(1);
    while let Some(arg)=parts.next(){
        match arg.to_str(){
            Some("--home")=>{
                let value=parts.next().ok_or("--home requires a directory")?;
                out.home=Some(PathBuf::from(value));
            },
            Some("--open")=>out.open=true,
            Some("-h"|"--help")=>out.help=true,
            _=>return Err(format!("unsupported server flag: {}",arg.to_string_lossy()).into())
        }
    }
    Ok(out)
}
fn open_browser(url:String){
    #[cfg(target_os="macos")]
    let mut command={let mut c=Command::new("open");c.arg(&url);c};
    #[cfg(target_os="windows")]
    let mut command={let mut c=Command::new("cmd");c.args(["/C","start","",url.as_str()]);c};
    #[cfg(not(any(target_os="macos",target_os="windows")))]
    let mut command={let mut c=Command::new("xdg-open");c.arg(&url);c};
    if let Err(error)=command.spawn(){tracing::warn!(%error,"could not open browser automatically");}
}
#[tokio::main]
async fn main()->Result<(),Box<dyn Error>>{
    let launch=args()?;
    if launch.help{
        println!("Dominion Rust server\n  --home PATH    Serve from this project home\n  --open         Open the UI in a browser after starting\n  --help         Show this help");
        return Ok(());
    }
    if let Some(home)=launch.home{
        std::env::set_current_dir(home)?;
    }
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env()
            .unwrap_or_else(|_|EnvFilter::new("dominion=info")))
        .init();
    let settings=Settings::from_env()?;
    let bind=settings.bind;
    let sdk=Dominion::open(settings).await?;
    let listener=tokio::net::TcpListener::bind(bind).await?;
    tracing::info!("DOMINION Rust SDK · main-frontend compatible HTTP adapter on http://{bind}");
    if launch.open{
        // CLI option from the original main package scripts, not a web request.
        std::thread::spawn(move||open_browser(format!("http://{bind}")));
    }
    axum::serve(listener,sdk.http_router()).await?;
    Ok(())
}
