use procshelf::{
    configuration::{
        Config,
        watch::{self, ConfigWatch},
    },
    processes::manager::Manager,
};
use std::{fs::OpenOptions, io::Read, os::unix::fs::OpenOptionsExt, path::PathBuf};

#[tokio::main]
async fn main() {
    let result = if std::env::args().nth(1).as_deref() == Some("--supervise") {
        procshelf::processes::supervisor::serve()
            .await
            .map_err(|e| e.to_string())
    } else {
        run().await
    };
    if let Err(error) = result {
        eprintln!("procshelf: {error}");
        std::process::exit(1);
    }
}
async fn run() -> Result<(), String> {
    let mut args = std::env::args().skip(1);
    let path = match args.next().as_deref() {
        None => PathBuf::from("config.toml"),
        Some("--config") => PathBuf::from(args.next().ok_or("--config requires a path")?),
        Some("--help" | "-h") => {
            println!(
                "Usage: procshelf [--config PATH]\nDefault configuration: ./config.toml\nRun as a foreground service. SIGINT/SIGTERM stops all managed processes."
            );
            return Ok(());
        }
        _ => return Err("usage: procshelf [--config PATH]".into()),
    };
    if args.next().is_some() {
        return Err("unexpected argument".into());
    }
    let path = std::fs::canonicalize(path)
        .map_err(|e| format!("cannot resolve configuration path: {e}"))?;
    let source = watch::read(&path).await?;
    let config = Config::parse(&source, path.parent().unwrap())?;
    let file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .mode(0o600)
        .custom_flags(nix::libc::O_NOFOLLOW | nix::libc::O_CLOEXEC)
        .open(path.with_extension("procshelf.lock"))
        .map_err(|e| e.to_string())?;
    let _lock = nix::fcntl::Flock::lock(file, nix::fcntl::FlockArg::LockExclusiveNonblock)
        .map_err(|(_, e)| format!("configuration already managed or cannot lock: {e}"))?;
    let listener = tokio::net::TcpListener::bind(config.listen)
        .await
        .map_err(|e| format!("cannot bind {}: {e}", config.listen))?;
    let listen = config.listen;
    let mut bytes = [0u8; 32];
    std::fs::File::open("/dev/urandom")
        .and_then(|mut f| f.read_exact(&mut bytes))
        .map_err(|e| format!("cannot create session token: {e}"))?;
    let token = bytes.iter().map(|b| format!("{b:02x}")).collect::<String>();
    let manager = Manager::with_lease(
        config,
        std::env::current_exe().map_err(|e| e.to_string())?,
        Some(_lock.try_clone().map_err(|e| e.to_string())?),
    );
    let _watch = match ConfigWatch::start(path, manager.clone(), source) {
        Ok(watch) => watch,
        Err(error) => {
            manager.shutdown().await;
            return Err(error);
        }
    };
    let app = procshelf::web::router(manager.clone(), listen, token);
    eprintln!("Procshelf listening on http://{listen}");
    let mut term = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        .map_err(|e| e.to_string())?;
    let shutdown_manager = manager.clone();
    let result = axum::serve(listener, app)
        .with_graceful_shutdown(async move {
            tokio::select! {_=tokio::signal::ctrl_c()=>{},_=term.recv()=>{}}
            shutdown_manager.shutdown().await;
        })
        .await;
    manager.shutdown().await;
    result.map_err(|e| e.to_string())
}
