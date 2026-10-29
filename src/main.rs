#[tokio::main]
async fn main() {
    if std::env::args().nth(1).as_deref() == Some("--supervise") {
        if let Err(e) = procshelf::processes::supervisor::serve().await {
            eprintln!("supervisor: {e}");
            std::process::exit(1);
        }
    } else {
        eprintln!("usage: procshelf --supervise");
        std::process::exit(1);
    }
}
