//! Verse-by-verse Quran audio.
//!
//! * One MP3 per verse, named `SSSAAA.mp3` (everyayah.com layout), saved in the
//!   app's private folder.
//! * Downloading and playing use Android's own Java classes through JNI
//!   (`java.net.URL` and `android.media.MediaPlayer`), so no extra native
//!   libraries are needed. On other platforms audio is simply unavailable.
//!
//! Audio files: everyayah.com. Credit the reciter and everyayah.com in the app.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};

pub struct Reciter {
    pub id: &'static str,
    pub name: &'static str,
    /// Folder name on everyayah.com, e.g. `Alafasy_128kbps`.
    pub folder: &'static str,
}

/// Reciters the app can use. The first one is the default.
/// To add one, add a line with a new unique `id` and its everyayah.com `folder`.
/// (`id` names the folder on the phone and is saved in the settings, so never change it.)
pub const RECITERS: &[Reciter] = &[
	
	    // Your own recording, kept on GitHub: one release per surah (`creator-002` holds
    // the files 002001.mp3, 002002.mp3 ...). `{s}` becomes the 3-digit surah number.
    Reciter {
        id: "creator",
        name: "Creator",
        folder: "https://github.com/Lugayizi-pro/QuranLuganda/releases/download/creator-{s}",
    },
    Reciter {
        id: "shuraym",
        name: "Saud Ash-Shuraym",
        folder: "Saood_ash-Shuraym_128kbps",
    },
    Reciter {
        id: "almaliki",
        name: "Aiman Almaliki",
        folder: "Ayman_Sowaid_64kbps",
    },
    Reciter {
        id: "feroz_alam",
        name: "Maulana Feroz Alam",
        folder: "translations/urdu_farhat_hashmi", // Note: replace with your exact everyayah folder path for Feroz Alam
    },
    Reciter {
        id: "alafasy",
        name: "Mishary Rashid AlAfasy",
        folder: "Alafasy_128kbps",
    },
    Reciter {
        id: "ashiq",
        name: "Qari Muhammad Ashiq",
        folder: "Muhammad_AbdulKareem_128kbps", // Note: replace with your exact everyayah folder path for Qari Ashiq
    },

];


/// Audio only works on Android (it uses Android's MediaPlayer).
pub const AVAILABLE: bool = cfg!(target_os = "android");

/// The inserted Bismillah entry (ayah 0) uses the recording of 1:1.
fn verse_key(surah: u16, ayah: u16) -> (u16, u16) {
    if ayah == 0 { (1, 1) } else { (surah, ayah) }
}

fn file_name(surah: u16, ayah: u16) -> String {
    let (s, a) = verse_key(surah, ayah);
    format!("{:03}{:03}.mp3", s, a)
}

pub fn verse_path(reciter: usize, surah: u16, ayah: u16) -> Option<PathBuf> {
    let r = RECITERS.get(reciter)?;
    Some(
        crate::app::save_dir()?
            .join("audio")
            .join(r.id)
            .join(file_name(surah, ayah)),
    )
}

fn verse_url(reciter: usize, surah: u16, ayah: u16) -> Option<String> {
    let r = RECITERS.get(reciter)?;
    if r.folder.starts_with("https://") {
        // Own recording: `folder` is a full web address. The Bismillah entries use
        // the recording of 1:1, so the surah number comes from the mapped verse.
        let (s, _) = verse_key(surah, ayah);
        let base = r.folder.replace("{s}", &format!("{:03}", s));
        return Some(format!("{}/{}", base, file_name(surah, ayah)));
    }
    Some(format!(
        "https://everyayah.com/data/{}/{}",
        r.folder,
        file_name(surah, ayah)
    ))
}

fn file_ok(path: &Path) -> bool {
    path.metadata().map(|m| m.len() > 0).unwrap_or(false)
}

pub fn verse_exists(reciter: usize, surah: u16, ayah: u16) -> bool {
    verse_path(reciter, surah, ayah)
        .map(|p| file_ok(&p))
        .unwrap_or(false)
}

/// True when every verse in `verses` (surah, ayah) has its audio on the phone.
pub fn all_exist(reciter: usize, verses: &[(u16, u16)]) -> bool {
    !verses.is_empty() && verses.iter().all(|&(s, a)| verse_exists(reciter, s, a))
}

/// Deletes the audio of the given verses.
pub fn delete(reciter: usize, verses: &[(u16, u16)]) {
    for &(s, a) in verses {
        if let Some(p) = verse_path(reciter, s, a) {
            let _ = std::fs::remove_file(p);
        }
    }
}

// ---------------------------------------------------------------- downloading

/// A running (or finished) download of one surah's audio.
pub struct Download {
    pub surah: usize,
    pub total: u32,
    done: Arc<AtomicU32>,
    cancel: Arc<AtomicBool>,
    finished: Arc<AtomicBool>,
    error: Arc<Mutex<Option<String>>>,
}

impl Download {
    pub fn done(&self) -> u32 {
        self.done.load(Ordering::Relaxed)
    }
    pub fn is_finished(&self) -> bool {
        self.finished.load(Ordering::Relaxed)
    }
    pub fn is_cancelled(&self) -> bool {
        self.cancel.load(Ordering::Relaxed)
    }
    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
    pub fn error(&self) -> Option<String> {
        self.error.lock().ok().and_then(|e| e.clone())
    }
}

/// Starts downloading the audio of `verses` (surah, ayah) in the background.
/// Files already on the phone are skipped.
pub fn start_download(reciter: usize, surah: usize, verses: &[(u16, u16)]) -> Option<Download> {
    let mut files: Vec<(String, PathBuf)> = Vec::new();
    for &(s, a) in verses {
        let url = verse_url(reciter, s, a)?;
        let path = verse_path(reciter, s, a)?;
        if !files.iter().any(|(_, p)| *p == path) {
            files.push((url, path));
        }
    }
    let download = Download {
        surah,
        total: files.len() as u32,
        done: Arc::new(AtomicU32::new(0)),
        cancel: Arc::new(AtomicBool::new(false)),
        finished: Arc::new(AtomicBool::new(false)),
        error: Arc::new(Mutex::new(None)),
    };
    let done = download.done.clone();
    let cancel = download.cancel.clone();
    let finished = download.finished.clone();
    let error = download.error.clone();
    std::thread::spawn(move || {
        for (url, path) in files {
            if cancel.load(Ordering::Relaxed) {
                break;
            }
            if file_ok(&path) {
                done.fetch_add(1, Ordering::Relaxed);
                continue;
            }
            if let Some(dir) = path.parent() {
                let _ = std::fs::create_dir_all(dir);
            }
            match platform::download(&url, &path, &cancel) {
                Ok(true) => {
                    done.fetch_add(1, Ordering::Relaxed);
                }
                Ok(false) => break, // cancelled
                Err(e) => {
                    if let Ok(mut slot) = error.lock() {
                        *slot = Some(e);
                    }
                    break;
                }
            }
        }
        finished.store(true, Ordering::Relaxed);
    });
    Some(download)
}

// ------------------------------------------------------------------- platform

#[cfg(target_os = "android")]
pub fn init_android(vm_ptr: *mut std::ffi::c_void) {
    platform::init(vm_ptr);
}

/// Gives the Android helpers (share sheet, clipboard) access to the activity.
#[cfg(target_os = "android")]
pub fn set_app(app: winit::platform::android::activity::AndroidApp) {
    platform::set_app(app);
}

pub use platform::{clipboard_text, download, share_text, Player};

#[cfg(target_os = "android")]
mod platform {
    use super::*;
    use jni::objects::{GlobalRef, JObject, JString, JValue};
    use jni::{JNIEnv, JavaVM};
    use std::sync::OnceLock;
    use std::time::Instant;
    use winit::platform::android::activity::AndroidApp;

    static VM: OnceLock<JavaVM> = OnceLock::new();
    static APP: OnceLock<AndroidApp> = OnceLock::new();

    pub fn set_app(app: AndroidApp) {
        let _ = APP.set(app);
    }

    /// Opens Android's share sheet with `text` as plain text.
    pub fn share_text(text: &str) -> Result<(), String> {
        let app = APP.get().ok_or_else(|| "sharing is not ready".to_string())?;
        let vm = VM.get().ok_or_else(|| "sharing is not ready".to_string())?;
        let mut env = vm.attach_current_thread().map_err(|e| e.to_string())?;
        match send_text(&mut env, app, text) {
            Ok(()) => Ok(()),
            Err(e) => Err(describe_error(&mut env, e.to_string())),
        }
    }

    /// Text currently on the Android clipboard (None when it is empty).
    pub fn clipboard_text() -> Result<Option<String>, String> {
        let app = APP.get().ok_or_else(|| "paste is not ready".to_string())?;
        let vm = VM.get().ok_or_else(|| "paste is not ready".to_string())?;
        let mut env = vm.attach_current_thread().map_err(|e| e.to_string())?;
        match read_clipboard(&mut env, app) {
            Ok(t) => Ok(t),
            Err(e) => Err(describe_error(&mut env, e.to_string())),
        }
    }

    fn read_clipboard(env: &mut JNIEnv, app: &AndroidApp) -> jni::errors::Result<Option<String>> {
        let activity = unsafe { JObject::from_raw(app.activity_as_ptr() as jni::sys::jobject) };
        let name = env.new_string("clipboard")?;
        let manager = env
            .call_method(
                &activity,
                "getSystemService",
                "(Ljava/lang/String;)Ljava/lang/Object;",
                &[JValue::Object(&name)],
            )?
            .l()?;
        if manager.is_null() {
            return Ok(None);
        }
        let clip = env
            .call_method(&manager, "getPrimaryClip", "()Landroid/content/ClipData;", &[])?
            .l()?;
        if clip.is_null() {
            return Ok(None);
        }
        if env.call_method(&clip, "getItemCount", "()I", &[])?.i()? < 1 {
            return Ok(None);
        }
        let item = env
            .call_method(
                &clip,
                "getItemAt",
                "(I)Landroid/content/ClipData$Item;",
                &[JValue::Int(0)],
            )?
            .l()?;
        let text = env
            .call_method(
                &item,
                "coerceToText",
                "(Landroid/content/Context;)Ljava/lang/CharSequence;",
                &[JValue::Object(&activity)],
            )?
            .l()?;
        if text.is_null() {
            return Ok(None);
        }
        let jstr = env
            .call_method(&text, "toString", "()Ljava/lang/String;", &[])?
            .l()?;
        let out: String = env.get_string(&JString::from(jstr))?.into();
        Ok(Some(out))
    }

    fn send_text(env: &mut JNIEnv, app: &AndroidApp, text: &str) -> jni::errors::Result<()> {
        // The activity pointer is owned by the app; it is only borrowed here.
        let activity = unsafe { JObject::from_raw(app.activity_as_ptr() as jni::sys::jobject) };
        let action = env.new_string("android.intent.action.SEND")?;
        let intent = env.new_object(
            "android/content/Intent",
            "(Ljava/lang/String;)V",
            &[JValue::Object(&action)],
        )?;
        let mime = env.new_string("text/plain")?;
        env.call_method(
            &intent,
            "setType",
            "(Ljava/lang/String;)Landroid/content/Intent;",
            &[JValue::Object(&mime)],
        )?;
        let key = env.new_string("android.intent.extra.TEXT")?;
        let body = env.new_string(text)?;
        env.call_method(
            &intent,
            "putExtra",
            "(Ljava/lang/String;Ljava/lang/String;)Landroid/content/Intent;",
            &[JValue::Object(&key), JValue::Object(&body)],
        )?;
        let title = env.new_string("Share verse")?;
        let chooser = env
            .call_static_method(
                "android/content/Intent",
                "createChooser",
                "(Landroid/content/Intent;Ljava/lang/CharSequence;)Landroid/content/Intent;",
                &[JValue::Object(&intent), JValue::Object(&title)],
            )?
            .l()?;
        env.call_method(
            &activity,
            "startActivity",
            "(Landroid/content/Intent;)V",
            &[JValue::Object(&chooser)],
        )?;
        Ok(())
    }

    pub fn init(ptr: *mut std::ffi::c_void) {
        if ptr.is_null() {
            return;
        }
        if let Ok(vm) = unsafe { JavaVM::from_raw(ptr as *mut jni::sys::JavaVM) } {
            let _ = VM.set(vm);
        }
    }

    /// Turns a pending Java exception into text (and clears it).
    fn describe_error(env: &mut JNIEnv, fallback: String) -> String {
        let mut text = fallback;
        if env.exception_check().unwrap_or(false) {
            if let Ok(ex) = env.exception_occurred() {
                let _ = env.exception_clear();
                if let Ok(v) = env.call_method(&ex, "toString", "()Ljava/lang/String;", &[]) {
                    if let Ok(o) = v.l() {
                        let js = JString::from(o);
                        if let Ok(s) = env.get_string(&js) {
                            text = String::from(s);
                        }
                    }
                }
            }
        }
        text
    }

    fn clear_exception(env: &mut JNIEnv) {
        if env.exception_check().unwrap_or(false) {
            let _ = env.exception_clear();
        }
    }

    /// Returns Ok(true) when saved, Ok(false) when cancelled.
    pub fn download(url: &str, dest: &Path, cancel: &AtomicBool) -> Result<bool, String> {
        let vm = VM.get().ok_or_else(|| "audio is not ready".to_string())?;
        let mut env = vm.attach_current_thread().map_err(|e| e.to_string())?;
        let bytes = match fetch(&mut env, url, cancel) {
            Ok(Some(b)) => b,
            Ok(None) => return Ok(false),
            Err(e) => return Err(describe_error(&mut env, e.to_string())),
        };
        if bytes.is_empty() {
            return Err("empty file received".to_string());
        }
        let tmp = dest.with_extension("part");
        std::fs::write(&tmp, &bytes).map_err(|e| e.to_string())?;
        std::fs::rename(&tmp, dest).map_err(|e| e.to_string())?;
        Ok(true)
    }

    fn fetch(
        env: &mut JNIEnv,
        url: &str,
        cancel: &AtomicBool,
    ) -> jni::errors::Result<Option<Vec<u8>>> {
        let jstr = env.new_string(url)?;
        let jurl = env.new_object("java/net/URL", "(Ljava/lang/String;)V", &[JValue::Object(&jstr)])?;
        let conn = env
            .call_method(&jurl, "openConnection", "()Ljava/net/URLConnection;", &[])?
            .l()?;
        env.call_method(&conn, "setConnectTimeout", "(I)V", &[JValue::Int(15000)])?;
        env.call_method(&conn, "setReadTimeout", "(I)V", &[JValue::Int(20000)])?;
        let input = env
            .call_method(&conn, "getInputStream", "()Ljava/io/InputStream;", &[])?
            .l()?;
        let buf = env.new_byte_array(16384)?;
        let mut data: Vec<u8> = Vec::new();
        let mut cancelled = false;
        loop {
            if cancel.load(Ordering::Relaxed) {
                cancelled = true;
                break;
            }
            let n = env
                .call_method(&input, "read", "([B)I", &[JValue::Object(&buf)])?
                .i()?;
            if n < 0 {
                break;
            }
            if n == 0 {
                continue;
            }
            let mut tmp = vec![0i8; n as usize];
            env.get_byte_array_region(&buf, 0, &mut tmp)?;
            data.extend(tmp.iter().map(|b| *b as u8));
        }
        let _ = env.call_method(&input, "close", "()V", &[]);
        clear_exception(env);
        Ok(if cancelled { None } else { Some(data) })
    }

    /// Plays one MP3 file at a time with Android's MediaPlayer.
    pub struct Player {
        obj: Option<GlobalRef>,
        started: Option<Instant>,
        paused: bool,
    }

    impl Player {
        pub fn new() -> Self {
            Player {
                obj: None,
                started: None,
                paused: false,
            }
        }

        pub fn play(&mut self, path: &Path) -> Result<(), String> {
            self.stop();
            let vm = VM.get().ok_or_else(|| "audio is not ready".to_string())?;
            let mut env = vm.attach_current_thread().map_err(|e| e.to_string())?;
            match start_player(&mut env, path) {
                Ok(g) => {
                    self.obj = Some(g);
                    self.started = Some(Instant::now());
                    self.paused = false;
                    Ok(())
                }
                Err(e) => Err(describe_error(&mut env, e.to_string())),
            }
        }

        pub fn stop(&mut self) {
            if let Some(g) = self.obj.take() {
                if let Some(vm) = VM.get() {
                    if let Ok(mut env) = vm.attach_current_thread() {
                        let _ = env.call_method(&g, "stop", "()V", &[]);
                        clear_exception(&mut env);
                        let _ = env.call_method(&g, "release", "()V", &[]);
                        clear_exception(&mut env);
                    }
                }
            }
            self.started = None;
            self.paused = false;
        }

        pub fn pause(&mut self) {
            if let (Some(g), Some(vm)) = (self.obj.as_ref(), VM.get()) {
                if let Ok(mut env) = vm.attach_current_thread() {
                    let _ = env.call_method(g, "pause", "()V", &[]);
                    clear_exception(&mut env);
                    self.paused = true;
                }
            }
        }

        pub fn resume(&mut self) {
            if let (Some(g), Some(vm)) = (self.obj.as_ref(), VM.get()) {
                if let Ok(mut env) = vm.attach_current_thread() {
                    let _ = env.call_method(g, "start", "()V", &[]);
                    clear_exception(&mut env);
                    self.paused = false;
                    self.started = Some(Instant::now());
                }
            }
        }

        /// True once the current file has played to its end.
        pub fn is_finished(&mut self) -> bool {
            let Some(g) = self.obj.as_ref() else {
                return false;
            };
            if self.paused {
                return false;
            }
            // Give the player a moment to start before asking if it is playing.
            if self
                .started
                .map(|t| t.elapsed().as_millis() < 400)
                .unwrap_or(false)
            {
                return false;
            }
            let Some(vm) = VM.get() else {
                return true;
            };
            let Ok(mut env) = vm.attach_current_thread() else {
                return true;
            };
            match env
                .call_method(g, "isPlaying", "()Z", &[])
                .and_then(|v| v.z())
            {
                Ok(playing) => !playing,
                Err(_) => {
                    clear_exception(&mut env);
                    true
                }
            }
        }
    }

    fn start_player(env: &mut JNIEnv, path: &Path) -> jni::errors::Result<GlobalRef> {
        let mp = env.new_object("android/media/MediaPlayer", "()V", &[])?;
        let jpath = env.new_string(path.to_string_lossy().to_string())?;
        env.call_method(
            &mp,
            "setDataSource",
            "(Ljava/lang/String;)V",
            &[JValue::Object(&jpath)],
        )?;
        env.call_method(&mp, "prepare", "()V", &[])?;
        env.call_method(&mp, "start", "()V", &[])?;
        env.new_global_ref(&mp)
    }
}

#[cfg(not(target_os = "android"))]
mod platform {
    use super::*;

    pub fn download(_url: &str, _dest: &Path, _cancel: &AtomicBool) -> Result<bool, String> {
        Err("audio downloads only work on Android".to_string())
    }

    pub fn share_text(_text: &str) -> Result<(), String> {
        Err("sharing only works on Android".to_string())
    }

    pub fn clipboard_text() -> Result<Option<String>, String> {
        Err("pasting only works on Android".to_string())
    }

    pub struct Player;

    impl Player {
        pub fn new() -> Self {
            Player
        }
        pub fn play(&mut self, _path: &Path) -> Result<(), String> {
            Err("audio only works on Android".to_string())
        }
        pub fn stop(&mut self) {}
        pub fn pause(&mut self) {}
        pub fn resume(&mut self) {}
        pub fn is_finished(&mut self) -> bool {
            false
        }
    }
}
