#[tokio::main]
async fn main() {
    let code = agent_ide_lib::cli::run_from_env().await;
    if code == 0
        && std::env::args()
            .any(|arg| arg == "--help" || arg == "-h" || arg == "--version" || arg == "-V")
    {
        return;
    }
    std::process::exit(code as i32);
}
