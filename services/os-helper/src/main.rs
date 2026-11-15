//! Main binary entry point for `friday-os-helper`.

#[cfg(not(target_os = "linux"))]
fn main() {
    eprintln!("Linux only");
}

#[cfg(target_os = "linux")]
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    friday_os_helper::run_daemon().await
}
