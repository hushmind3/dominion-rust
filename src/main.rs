#[tokio::main]
async fn main() {
    if let Err(error) = dominion_rust::bootstrap::run(std::env::args().skip(1).collect()).await {
        eprintln!("{error}");
        std::process::exit(1);
    }
}
