//! Loss: a music player and downloader for Android, written in Rust.

mod diag;
#[cfg(target_os = "android")]
mod glue;
mod media;
mod nav;
mod notify;
mod platform;
mod services;
mod ui;

fn main() {
    diag::init();
    log::info!("Loss starting");
    #[cfg(feature = "desktop")]
    {
        use dioxus::desktop::{Config, LogicalSize, WindowBuilder};
        // Desktop build is for development: open a phone-sized window.
        dioxus::LaunchBuilder::desktop()
            .with_cfg(
                Config::new().with_window(
                    WindowBuilder::new()
                        .with_title("Loss")
                        .with_inner_size(LogicalSize::new(412.0, 880.0)),
                ),
            )
            .launch(ui::App);
    }
    #[cfg(not(feature = "desktop"))]
    dioxus::launch(ui::App);
}
