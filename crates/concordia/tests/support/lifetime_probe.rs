//! Test-only process-lifetime probe.

use std::io::Write;
use std::time::Duration;

#[tokio::main]
async fn main() {
    let fake_provider = std::env::args()
        .nth(1)
        .expect("lifetime probe requires the fake provider path");
    let child = concordia::process_ui::tokio_command(fake_provider)
        .env("FAKE_PROVIDER_MODE", "hang")
        .spawn()
        .expect("spawn fake provider");
    concordia::process_lifetime::bind_to_concordia(&child);
    println!("{}", child.id().expect("fake provider has a process id"));
    std::io::stdout().flush().expect("flush child PID");
    std::thread::sleep(Duration::from_secs(120));
}
