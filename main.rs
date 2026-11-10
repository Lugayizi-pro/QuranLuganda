#![allow(dead_code, unused_imports)]

mod app;
mod arabic;
mod audio;
mod data;
mod theme;

#[cfg(not(target_os = "android"))]
fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default().with_inner_size([420.0, 820.0]),
        ..Default::default()
    };
    eframe::run_native(
        "RustQuran",
        options,
        Box::new(|cc| Ok(Box::new(app::QuranApp::new(cc)))),
    )
}

// On Android the app starts from `android_main` in lib.rs.
#[cfg(target_os = "android")]
fn main() {}
