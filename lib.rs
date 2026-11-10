mod app;
mod arabic;
mod audio;
mod data;
mod theme;

pub use app::QuranApp;

#[cfg(target_os = "android")]
#[unsafe(no_mangle)]
fn android_main(app: winit::platform::android::activity::AndroidApp) {
    android_logger::init_once(
        android_logger::Config::default().with_max_level(log::LevelFilter::Info),
    );

    // Folder the app can always write to; saved settings, bookmarks and the
    // last-read verse live here.
    if let Some(dir) = app.internal_data_path() {
        crate::app::set_save_dir(dir);
    }

    // Lets the audio code use Android's MediaPlayer and networking.
    audio::init_android(app.vm_as_ptr());
    audio::set_app(app.clone());

    let options = eframe::NativeOptions {
        android_app: Some(app),
        ..Default::default()
    };

    let result = eframe::run_native(
        "RustQuran",
        options,
        Box::new(|cc| Ok(Box::new(QuranApp::new(cc)))),
    );
    if let Err(e) = result {
        log::error!("The app stopped: {e}");
    }

    // Android destroys and re-creates the activity for split screen, rotation and
    // similar changes, and then calls `android_main` again in the same process.
    // winit cannot start a second event loop in one process, so that second start
    // used to crash. Ending the process here lets Android start the app fresh
    // (your saved place and settings are restored). It also makes Back really exit.
    // `_exit` ends the process at once without running exit handlers. `exit` could
    // hang here, because Android's main thread is waiting for this thread to finish.
    unsafe { libc::_exit(0) };
}
