//! Link wowdps's app icon into the binary on Windows: Explorer shows it,
//! and the daemon's tray icon loads it (`src/tray.rs`).

#[path = "../build-support/icon.rs"]
mod icon;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    icon::link();
}
