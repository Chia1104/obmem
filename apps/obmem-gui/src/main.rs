//! `cargo run -p obmem-gui`: the window on this machine's native backend. `nsh run` and
//! `nsh package` build their own runner around [`obmem_gui::App`].

#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

#[cfg(target_os = "macos")]
use nsh_platform_macos as platform;
#[cfg(target_os = "windows")]
use nsh_platform_windows as platform;

#[cfg(any(target_os = "macos", target_os = "windows"))]
fn main() -> impl std::process::Termination {
    obmem_gui::prepare_process();
    env_logger::init();
    platform::run_app(
        platform::AppSource::Rust(platform::Root::of::<obmem_gui::App>()),
        platform::AppOptions {
            title: "obmem".to_owned(),
            #[cfg(target_os = "windows")]
            app_id: "io.github.chia1104.obmem".to_owned(),
            size: (960, 680),
            ..Default::default()
        },
    )
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn main() {
    eprintln!("obmem-gui runs on macOS and Windows.");
}
