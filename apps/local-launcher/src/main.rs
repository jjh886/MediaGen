#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

mod health;
mod launch;
mod open_browser;
mod platform;
#[cfg(any(target_os = "macos", target_os = "windows"))]
mod tray;

fn main() {
    let session = match launch::run() {
        Ok(session) => session,
        Err(err) => {
            platform::show_error(&err);
            std::process::exit(1);
        }
    };

    #[cfg(any(target_os = "macos", target_os = "windows"))]
    if let Err(err) = tray::run(session) {
        platform::show_error(&err);
        std::process::exit(1);
    }

    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    if let Err(err) = open_browser::open_browser(&session.url) {
        platform::show_error(&err);
        std::process::exit(1);
    }
}
