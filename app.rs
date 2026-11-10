//! RustQuran: glassmorphic Quran reader for egui / eframe 0.31 (Android + desktop).

use core::f32;
use std::collections::HashMap;
use std::time::Duration;

use eframe::egui::{
    self, Align, Color32, CornerRadius, FontFamily, FontId, Layout, Pos2, RichText, Sense, Stroke,
};

use crate::arabic::{self, contains_ci};
use crate::audio;
use crate::data::{
    self, QuranDataset, SURAHS, SurahInfo, TRANSLATIONS, Verse, get_ruku_number, juz_of,
    juz_start,
};
use crate::theme::{self, Theme};

const STORE_KEY: &str = "rustquran_v1";

// ---- Downloadable translations ---------------------------------------------------
/// Shared between the download thread and the UI.
#[derive(Default)]
struct TransDl {
    /// Index (in `TRANSLATIONS`) of the translation being downloaded.
    active: Option<usize>,
    /// Finished download waiting to be picked up by the UI:
    /// (index, (new file, version) or error).
    done: Option<(usize, Result<(std::path::PathBuf, u32), String>)>,
    /// Translations that have a newer version online (result of the update check).
    updates: Option<Vec<usize>>,
}

/// Parses lines like `en_sahih|3` (translation id and its version number).
fn parse_versions(text: &str) -> HashMap<String, u32> {
    text.lines()
        .filter_map(|l| {
            let (id, n) = l.split_once('|')?;
            Some((id.trim().to_string(), n.trim().parse().ok()?))
        })
        .collect()
}

/// Version numbers of the translations on the phone (missing = 1).
fn read_local_versions(dir: &std::path::Path) -> HashMap<String, u32> {
    std::fs::read_to_string(dir.join("versions.txt"))
        .map(|t| parse_versions(&t))
        .unwrap_or_default()
}

fn write_local_version(dir: &std::path::Path, id: &str, version: u32) {
    let mut m = read_local_versions(dir);
    m.insert(id.to_string(), version);
    let text: String = m.iter().map(|(k, v)| format!("{k}|{v}\n")).collect();
    let _ = std::fs::write(dir.join("versions.txt"), text);
}

/// Downloads the small `translations_version.txt` of the GitHub release
/// (one `id|version` line per translation). `None` when it is not there.
fn fetch_remote_versions(dir: &std::path::Path) -> Option<HashMap<String, u32>> {
    let dest = dir.join("translations_version.txt");
    let url = format!("{}translations_version.txt", data::TRANSLATIONS_URL);
    let cancel = std::sync::atomic::AtomicBool::new(false);
    if !matches!(audio::download(&url, &dest, &cancel), Ok(true)) {
        return None;
    }
    let text = std::fs::read_to_string(&dest).ok()?;
    let _ = std::fs::remove_file(&dest);
    Some(parse_versions(&text))
}

/// Folder where downloaded translations are kept.
fn translations_dir() -> Option<std::path::PathBuf> {
    let dir = SAVE_DIR.get()?.join("translations");
    std::fs::create_dir_all(&dir).ok()?;
    Some(dir)
}

// ---- Saving to our own file --------------------------------------------------
// eframe's built-in storage is not reliable on Android, so everything the user
// does (last read verse, tapped verses, bookmarks, settings) is also written to
// a plain file in the app's private folder.
static SAVE_DIR: std::sync::OnceLock<std::path::PathBuf> = std::sync::OnceLock::new();

pub fn set_save_dir(dir: std::path::PathBuf) {
    let _ = SAVE_DIR.set(dir);
}

/// The app's private folder (Android). Audio files are stored inside it.
pub fn save_dir() -> Option<std::path::PathBuf> {
    SAVE_DIR.get().cloned()
}

fn save_file() -> Option<std::path::PathBuf> {
    if let Some(d) = SAVE_DIR.get() {
        return Some(d.join("rustquran_save.txt"));
    }
    // Desktop fallback.
    let home = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE"))?;
    Some(std::path::PathBuf::from(home).join(".rustquran_save.txt"))
}

fn read_save_file() -> Option<String> {
    std::fs::read_to_string(save_file()?).ok()
}

fn write_save_file(text: &str) {
    if let Some(path) = save_file() {
        let tmp = path.with_extension("tmp");
        if std::fs::write(&tmp, text).is_ok() {
            let _ = std::fs::rename(&tmp, &path);
        }
    }
}

const LONG_PRESS_SECS: f64 = 0.4;
const MOVE_SLOP: f32 = 12.0;
const MAX_HITS: usize = 40;
const OVERSCAN: f32 = 300.0;
const TOP_INSET: f32 = if cfg!(target_os = "android") {
    30.0
} else {
    6.0
};
const BOTTOM_PAD: f32 = 36.0;
const HIDE_BAR_WHEN_SWIPING_UP: bool = true;
const BAR_SCROLL_THRESHOLD: f32 = 30.0;

// Scroll momentum (fling) after you lift your finger.
// FLING_FRICTION: lower = rolls longer (0.3 = max, 1.0 = long, 2.0+ = short).
// FLING_BOOST: multiplies the speed of the flick (1.0 = same speed as your finger).
// FLING_MAX: top speed in points per second.
const FLING_FRICTION: f32 = 0.3;
const FLING_BOOST: f32 = 1.0;
const FLING_MAX: f32 = 20000.0;
const FLING_MIN: f32 = 20.0;

// Auto-scroll: pixels per second for each step of the speed slider.
const AUTO_PX_PER_LEVEL: f32 = 6.0;

// Translation text size range (slider in Reader settings).
const MIN_TRANS: f32 = 12.0;
const MAX_TRANS: f32 = 32.0;

enum AudioAction {
    Download(usize),
    Cancel,
    PlaySurah(usize),
    Delete(usize),
}

// Android only: swipe right from the left edge opens the sidebar, swipe left closes it.
const SWIPE_EDGE: f32 = f32::INFINITY;
const SWIPE_MIN: f32 = 20.0;

const MIN_ARABIC: f32 = 20.0;
const MAX_ARABIC: f32 = 72.0;
const REF_SIZE: f32 = 40.0;

const MAX_SPACING: f32 = 0.9;
const MIN_WIDTH_FRAC: f32 = 0.5;

const REF_GAP: f32 = 6.0;
const SEP_BLOCK: f32 = 17.0;
const SECTION_GAP: f32 = 18.0;
const MSG_H: f32 = 70.0;
const KEEP_SECTIONS: usize = 16;

struct SurahLabel {
    number: String,
    meta: String,
    arabic: String,
}

struct Hit {
    verse: u32,
    label: String,
    shaped_ar: String,
}

struct Bookmark {
    global: u32,
    label: String,
}

#[derive(Clone, Copy)]
struct FlowItem {
    verse: u32,
    start: u32,
    end: u32,
    base: f32,
    width: f32,
}

#[derive(Clone, Copy)]
struct Line {
    first: u32,
    end: u32,
    width: f32,
}

struct SurahLayout {
    shaped: Vec<String>,
    markers: Vec<String>,
    ref_labels: Vec<String>,
    basmala: String,
    items: Vec<FlowItem>,
    verse_items: Vec<(u32, u32)>,
    flow_lines: Vec<Line>,
    verse_lines: Vec<Line>,
    verse_line_range: Vec<(u32, u32)>,
    trans_h: Vec<f32>,
    verse_h: Vec<f32>,
    verse_y: Vec<f32>,
    height: f32,
}

impl SurahLayout {
    fn build(
        data: &QuranDataset,
        ctx: &egui::Context,
        s: usize,
        m: &Metrics,
        translation: bool,
        tr_idx: usize,
        cache: &mut HashMap<String, f32>,
    ) -> Self {
        let verses = data.surah_verses(s);
        let surah_no = s + 1;
        let shaped: Vec<String> = verses
            .iter()
            .map(|v| arabic::shape_words_ex(v.arabic, m.keep_marks, m.tajweed))
            .collect();
        let markers: Vec<String> = verses
            .iter()
            .map(|v| arabic::to_arabic_indic(v.num))
            .collect();
        let ref_labels: Vec<String> = verses
            .iter()
            .map(|v| format!("{}:{}", surah_no, v.num))
            .collect();

        // The Bismillah is now verse 1 of the surah, so the header no longer draws it.
        let basmala = String::new();

        let mut items: Vec<FlowItem> = Vec::new();
        let mut verse_items: Vec<(u32, u32)> = Vec::with_capacity(shaped.len());
        for (v, sh) in shaped.iter().enumerate() {
            let first = items.len() as u32;
            let mut pos = 0usize;
            for word in sh.split(' ') {
                let start = pos;
                let end = start + word.len();
                pos = end + 1;
                if word.is_empty() {
                    continue;
                }
                let base = match cache.get(word) {
                    Some(&w) => w,
                    None => {
                        // Marks are drawn separately, so they must not count in the width.
                        let bare: String = word
                            .chars()
                            .filter(|c| !arabic::is_mark(*c) && !arabic::is_tag(*c))
                            .collect();
                        let w = ctx.fonts_mut(|f| {
                            f.layout_no_wrap(bare, m.ref_font.clone(), Color32::WHITE)
                                .size()
                                .x
                        });
                        cache.insert(word.to_owned(), w);
                        w
                    }
                };
                items.push(FlowItem {
                    verse: v as u32,
                    start: start as u32,
                    end: end as u32,
                    base,
                    width: 0.0,
                });
            }
            items.push(FlowItem {
                verse: v as u32,
                start: 0,
                end: 0,
                base: 0.0,
                width: 0.0,
            });
            verse_items.push((first, items.len() as u32));
        }

        let mut lay = SurahLayout {
            shaped,
            markers,
            ref_labels,
            basmala,
            items,
            verse_items,
            flow_lines: Vec::new(),
            verse_lines: Vec::new(),
            verse_line_range: Vec::new(),
            trans_h: Vec::new(),
            verse_h: Vec::new(),
            verse_y: Vec::new(),
            height: 0.0,
        };
        lay.relayout(ctx, verses, data.surah_translations(tr_idx, s), m, translation);
        lay
    }

    fn relayout(
        &mut self,
        ctx: &egui::Context,
        verses: &[Verse],
        trans: &[&'static str],
        m: &Metrics,
        translation: bool,
    ) {
        let scale = m.size / REF_SIZE;
        for it in self.items.iter_mut() {
            it.width = if it.start == it.end {
                m.marker_d
            } else {
                it.base * scale
            };
        }

        self.flow_lines.clear();
        {
            let lines = &mut self.flow_lines;
            break_lines(&self.items, 0, m.gap, m.max_w, |a, b, w| {
                lines.push(Line {
                    first: a as u32,
                    end: b as u32,
                    width: w,
                })
            });
        }

        self.verse_lines.clear();
        self.verse_line_range.clear();
        for &(a, b) in &self.verse_items {
            let l0 = self.verse_lines.len() as u32;
            let words_end = (b as usize).saturating_sub(1).max(a as usize);
            let lines = &mut self.verse_lines;
            break_lines(
                &self.items[a as usize..words_end],
                a as usize,
                m.gap,
                m.max_w,
                |st, en, w| {
                    lines.push(Line {
                        first: st as u32,
                        end: en as u32,
                        width: w,
                    })
                },
            );
            self.verse_line_range
                .push((l0, self.verse_lines.len() as u32));
        }

        self.trans_h.clear();
        self.verse_h.clear();
        self.verse_y.clear();
        let mut body_h = self.flow_lines.len() as f32 * m.line_h;
        if translation {
            let mut y = 0.0f32;
            for (i, v) in verses.iter().enumerate() {
                self.verse_y.push(y);
                let text = trans.get(i).copied().unwrap_or("");
                let th = if text.is_empty() {
                    0.0
                } else {
                    translation_galley_height(ctx, text, m)
                };
                let (l0, l1) = self.verse_line_range[i];
                let has_ruku = get_ruku_number(v.surah, v.ayah).is_some();
                let ruku_space = if has_ruku { 28.0 } else { 0.0 };

                let h = ruku_space
                    + m.body_row_h
                    + REF_GAP
                    + (l1 - l0) as f32 * m.line_h
                    + if th > 0.0 { REF_GAP + th } else { 0.0 }
                    + SEP_BLOCK;
                self.trans_h.push(th);
                self.verse_h.push(h);
                y += h;
            }
            self.verse_y.push(y);
            body_h = y;
        }
        if verses.is_empty() {
            body_h = MSG_H;
        }
        self.height = header_height(m, !self.basmala.is_empty()) + body_h + SECTION_GAP;
    }
}

struct Metrics {
    size: f32,
    line_h: f32,
    gap: f32,
    marker_d: f32,
    /// Draw verse numbers as 1, 2, 3 instead of Arabic-Indic digits.
    numeric_verses: bool,
    max_w: f32,
    wrap_w: f32,
    keep_marks: bool,
    tajweed: bool,
    center_translation: bool,
    font: FontId,
    ref_font: FontId,
    body_font: FontId,
    body_row_h: f32,
    trans_font: FontId,
    trans_row_h: f32,
    title_font: FontId,
    title_h: f32,
    head_font: FontId,
    head_h: f32,
    meta_font: FontId,
    meta_h: f32,
    bas_font: FontId,
    bas_h: f32,
}

impl Metrics {
    fn placeholder(family: &FontFamily) -> Self {
        let arabic = |s: f32| FontId::new(s, family.clone());
        Self {
            size: 30.0,
            line_h: 60.0,
            gap: 9.0,
            marker_d: 34.5,
            numeric_verses: false,
            max_w: 300.0,
            wrap_w: 300.0,
            keep_marks: true,
            tajweed: false,
            center_translation: true,
            font: arabic(30.0),
            ref_font: arabic(REF_SIZE),
            body_font: FontId::proportional(17.0),
            body_row_h: 22.0,
            trans_font: FontId::proportional(17.0),
            trans_row_h: 22.0,
            title_font: arabic(37.5),
            title_h: 60.0,
            head_font: FontId::proportional(26.0),
            head_h: 32.0,
            meta_font: FontId::proportional(13.0),
            meta_h: 16.0,
            bas_font: arabic(30.0),
            bas_h: 40.0,
        }
    }
}

pub struct LongPressContext {
    pub global: u32,
    pub title: String,
    pub shaped_ar: String,
    pub show_tafsir: bool,
}

#[derive(Default)]
struct PressTracker {
    target: Option<usize>,
    origin: Pos2,
    start: f64,
    settled: bool,
}

impl PressTracker {
    fn update(&mut self, ui: &egui::Ui, key: usize) -> bool {
        let (now, pos) = ui.input(|i| (i.time, i.pointer.interact_pos()));
        let Some(pos) = pos else {
            return false;
        };
        if self.target != Some(key) {
            *self = PressTracker {
                target: Some(key),
                origin: pos,
                start: now,
                settled: false,
            };
        }
        if self.settled {
            return false;
        }
        if pos.distance(self.origin) > MOVE_SLOP {
            self.settled = true;
            return false;
        }
        if now - self.start >= LONG_PRESS_SECS {
            self.settled = true;
            return true;
        }
        ui.ctx().request_repaint_after(Duration::from_millis(40));
        false
    }
}

#[derive(Clone, Copy, PartialEq, Default)]
struct LayoutKey {
    width: i32,
    font_bits: u32,
    harakat: bool,
    translation: bool,
    translation_idx: usize,
    trans_size: u32,
    spacing_e3: u32,
    width_pct: u32,
}

pub struct QuranApp {
    data: QuranDataset,
    labels: Vec<SurahLabel>,
    selected_surah: usize,
    sidebar_open: bool,
    prev_sidebar_open: bool,
    sidebar_toggle_time: f64,
    compact: bool,
    surah_filter: String,

    layouts: Vec<Option<SurahLayout>>,
    tops: Vec<f32>,
    m: Metrics,
    word_w: HashMap<String, f32>,
    layout_key: LayoutKey,
    layouts_dirty: bool,
    pending_jump: Option<(usize, Option<usize>)>,
    est_words: Vec<u32>,
    est_trans: Vec<u32>,

    fling_vel: f32,
    /// The page scrolls by itself (started in the Jump dialog).
    auto_scroll: bool,
    /// Pace of the auto-scroll, 1 (slowest) to 20 (fastest).
    auto_speed: f32,
    /// Fraction of a pixel not yet scrolled.
    auto_accum: f32,
    /// Frames in a row where auto-scroll could not move the page (end of the Quran).
    auto_stall: u32,
    /// Auto-scroll pushed the page this frame.
    auto_pushed: bool,

    bar_visible: bool,
    /// Height of the bar / keyboard at the bottom of the screen right now.
    bottom_inset: f32,
    last_offset: f32,
    scroll_accum: f32,
    bar_toggle_time: f64,

    search_query: String,
    search_applied: String,
    needle: String,
    hits: Vec<Hit>,
    hit_heights: Vec<f32>,
    surah_hits: Vec<u16>,
    search_status: String,

    press: PressTracker,
    long_press_state: Option<LongPressContext>,
    bookmarks: Vec<Bookmark>,

    arabic_family: FontFamily,
    arabic_size: f32,
    size_edit: f32,
    show_harakat: bool,
    /// Verse numbers as 1, 2, 3 instead of Arabic-Indic digits.
    numeric_verses: bool,
    show_translation: bool,
    translation_idx: usize,
    /// Per translation: is its text available (bundled, or already downloaded)?
    trans_ready: Vec<bool>,
    trans_dl: std::sync::Arc<std::sync::Mutex<TransDl>>,
    /// Per translation: is its tafsir on the phone?
    tafsir_ready: Vec<bool>,
    /// Per translation: a newer tafsir is available online.
    tafsir_update: Vec<bool>,
    tafsir_dl: std::sync::Arc<std::sync::Mutex<TransDl>>,
    /// Last tafsir download problem.
    tafsir_msg: Option<String>,
    /// Downloaded translations that have a newer version online.
    trans_update: Vec<bool>,
    /// The online version check has been started this run.
    trans_checked: bool,
    /// The automatic first download has been considered this run.
    trans_auto: bool,
    /// Last download problem, shown under the translation list.
    trans_msg: Option<String>,
    trans_size: f32,
    center_translation: bool,
    line_spacing: f32,
    width_frac: f32,
    theme: Theme,
    font_found: bool,

    quick_jump: bool,
    jump_query: String,

    swipe_start: Option<Pos2>,
    swipe_last: Option<Pos2>,

    dirty: bool,
    last_persist: f64,
    restore_position: Option<u32>,
    /// Last verse the reader tapped (shown in the tapped colour): the app always reopens here.
    last_read: Option<u32>,
    press_verse: Option<usize>,
    press_origin: Option<Pos2>,
    press_moved: bool,
    stopped_fling: bool,

    // Audio
    audio_player: audio::Player,
    /// Verse (global index) being recited right now; shown in the playing colour.
    audio_playing: Option<u32>,
    /// End (exclusive, global index) of the surah being played.
    audio_end: u32,
    audio_paused: bool,
    audio_single: bool,
    /// Index into `audio::RECITERS`.
    reciter: usize,
    /// Colour the letters by tajweed rule.
    show_tajweed: bool,
    /// Which sidebar section is open (accordion): 0 Translation, 1 Audio, 2 Reader settings, 3 Juz, 4 Bookmarks.
    sidebar_section: Option<u8>,
    /// Scroll position of the surah list (scrolling it closes the open section).
    surah_list_offset: f32,

    // First-use guide
    /// Page of the welcome guide being shown (None = hidden).
    guide_page: Option<usize>,
    /// True once the reader has finished or skipped the guide.
    guide_seen: bool,
    guide_tex: Vec<Option<egui::TextureHandle>>,
    /// On-screen Arabic keyboard shown?
    ar_kbd_open: bool,
    /// Text field the Arabic keyboard types into: 1 search, 2 surah filter, 3 jump box.
    ar_target: u8,
    audio_download: Option<audio::Download>,
    audio_msg: String,
    audio_msg_until: f64,
    audio_hint_shown: bool,
    /// (surah, whole surah downloaded, time checked)
    audio_ready_cache: (usize, bool, f64),
}

impl QuranApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let (arabic_family, font_found) = setup_fonts(&cc.egui_ctx);
        let theme = Theme::Sepia;
        theme::apply(&cc.egui_ctx, theme);
        // The translation size slider starts at the theme's normal body text size.
        let default_trans_size = cc
            .egui_ctx
            .style()
            .text_styles
            .get(&egui::TextStyle::Body)
            .map(|f| f.size.round())
            .unwrap_or(17.0)
            .clamp(MIN_TRANS, MAX_TRANS);
        // Slower than egui's default (~0.08s) so slides and fades look smooth.
        cc.egui_ctx.style_mut(|s| s.animation_time = 0.22);

        let data = QuranDataset::load_embedded();
        let labels: Vec<SurahLabel> = SURAHS
            .iter()
            .map(|s| SurahLabel {
                number: s.number.to_string(),
                meta: format!(
                    "{} \u{b7} {} verses",
                    s.english,
                    if s.number == 1 || s.number == 9 { s.verses } else { s.verses + 1 }
                ),
                arabic: arabic::shape_line(s.arabic, false),
            })
            .collect();
        let est_words: Vec<u32> = (0..SURAHS.len())
            .map(|s| {
                data.surah_verses(s)
                    .iter()
                    .map(|v| v.arabic.split(' ').count() as u32)
                    .sum()
            })
            .collect();
        // Filled by `recompute_est_trans` once the saved translation is known.
        let est_trans: Vec<u32> = vec![0; SURAHS.len()];

        let mut layouts = Vec::with_capacity(SURAHS.len());
        layouts.resize_with(SURAHS.len(), || None);
        let m = Metrics::placeholder(&arabic_family);

        let mut app = Self {
            data,
            labels,
            selected_surah: 0,
            sidebar_open: false,
            prev_sidebar_open: false,
            sidebar_toggle_time: -10.0,
            compact: true,
            surah_filter: String::new(),
            layouts,
            tops: Vec::new(),
            m,
            word_w: HashMap::new(),
            layout_key: LayoutKey::default(),
            layouts_dirty: false,
            pending_jump: None,
            est_words,
            est_trans,
            fling_vel: 0.0,
            auto_scroll: false,
            auto_speed: 6.0,
            auto_accum: 0.0,
            auto_stall: 0,
            auto_pushed: false,
            bar_visible: true,
            bottom_inset: 0.0,
            last_offset: 0.0,
            scroll_accum: 0.0,
            bar_toggle_time: 0.0,
            search_query: String::new(),
            search_applied: String::new(),
            needle: String::new(),
            hits: Vec::new(),
            hit_heights: Vec::new(),
            surah_hits: Vec::new(),
            search_status: String::new(),
            press: PressTracker::default(),
            long_press_state: None,
            bookmarks: Vec::new(),
            arabic_family,
            arabic_size: 25.0,
            size_edit: 25.0,
            show_harakat: true,
            numeric_verses: false,
            show_translation: false,
            translation_idx: 0,
            trans_ready: TRANSLATIONS.iter().map(|t| t.is_bundled()).collect(),
            trans_dl: Default::default(),
            tafsir_ready: vec![false; TRANSLATIONS.len()],
            tafsir_update: vec![false; TRANSLATIONS.len()],
            tafsir_dl: Default::default(),
            tafsir_msg: None,
            trans_update: vec![false; TRANSLATIONS.len()],
            trans_checked: false,
            trans_auto: false,
            trans_msg: None,
            trans_size: default_trans_size,
            center_translation: true,
            line_spacing: 0.0,
            width_frac: 1.0,
            theme,
            font_found,
            quick_jump: false,
            jump_query: String::new(),
            swipe_start: None,
            swipe_last: None,
            dirty: false,
            last_persist: 0.0,
            restore_position: None,
            last_read: None,
            press_verse: None,
            press_origin: None,
            press_moved: false,
            stopped_fling: false,
            audio_player: audio::Player::new(),
            audio_playing: None,
            audio_end: 0,
            audio_paused: false,
            audio_single: false,
            reciter: 0,
            show_tajweed: false,
            sidebar_section: None,
            surah_list_offset: 0.0,
            guide_page: None,
            guide_seen: false,
            guide_tex: (0..GUIDE_IMAGES.len()).map(|_| None).collect(),
            ar_kbd_open: false,
            ar_target: 1,
            audio_download: None,
            audio_msg: String::new(),
            audio_msg_until: 0.0,
            audio_hint_shown: false,
            audio_ready_cache: (usize::MAX, false, -10.0),
        };

        let saved = read_save_file().or_else(|| cc.storage.and_then(|s| s.get_string(STORE_KEY)));
        if let Some(raw) = saved {
            app.load_save(&raw);
        }
        // `theme::apply` above used the default theme; the saved one is only known now.
        // Without this, a Night app reopened after being closed kept the Sepia widget
        // colours and looked washed out and unreadable.
        theme::apply(&cc.egui_ctx, app.theme);
        app.load_downloaded_translations();
        app.load_downloaded_tafsirs();
        app.recompute_est_trans();
        if !app.guide_seen {
            app.guide_page = Some(0);
        }
        // Reopen at the last verse the user tapped; otherwise where they stopped scrolling.
        let resume = app.last_read.or(app.restore_position.take());
        if let Some(g) = resume {
            app.jump_to_verse(g);
        } else {
            app.select_surah(0);
        }
        app
    }

    fn save_into(&self, storage: &mut dyn eframe::Storage) {
        let out = self.serialize();
        write_save_file(&out);
        storage.set_string(STORE_KEY, out);
    }

    fn save_file_only(&self) {
        write_save_file(&self.serialize());
    }

    fn serialize(&self) -> String {
        let mut out = String::from("v2\n");
        out.push_str(&format!("size|{:.1}\n", self.size_edit));
        out.push_str(&format!("theme|{}\n", self.theme.to_u8()));
        out.push_str(&format!("trans|{}\n", self.show_translation as u8));
        out.push_str(&format!("tid|{}\n", self.current_translation().id));
        out.push_str(&format!("tsize|{:.0}\n", self.trans_size));
        out.push_str(&format!("audiosingle|{}\n", self.audio_single as u8));
        out.push_str(&format!("guide|{}\n", self.guide_seen as u8));
        out.push_str(&format!(
            "reciter|{}\n",
            audio::RECITERS[self.reciter.min(audio::RECITERS.len() - 1)].id
        ));
        out.push_str(&format!("tajweed|{}\n", self.show_tajweed as u8));
        out.push_str(&format!("center|{}\n", self.center_translation as u8));
        out.push_str(&format!("harakat|{}\n", self.show_harakat as u8));
        out.push_str(&format!("numverses|{}\n", self.numeric_verses as u8));
        out.push_str(&format!("autospeed|{}\n", self.auto_speed as u32));
        out.push_str(&format!(
            "spacing|{}\n",
            (self.line_spacing * 1000.0).round() as u32
        ));
        out.push_str(&format!(
            "widthpct|{}\n",
            (self.width_frac * 100.0).round() as u32
        ));
        let bm: Vec<String> = self
            .bookmarks
            .iter()
            .map(|b| b.global.to_string())
            .collect();
        out.push_str(&format!("bookmarks|{}\n", bm.join(",")));
        if let Some((s, v)) = self.current_anchor() {
            let (a, b) = self.data.surah_range(s);
            let i = v.unwrap_or(0).min((b - a).saturating_sub(1) as usize);
            out.push_str(&format!("last|{}\n", a as usize + i));
        }
        if let Some(g) = self.last_read {
            out.push_str(&format!("lastread|{}\n", g));
        }
        out
    }

    fn load_save(&mut self, raw: &str) {
        // Saves from before the Bismillah became verse 1 ("v1") store old verse indexes.
        let old_format = raw.lines().next().map(|l| l.trim()) != Some("v2");
        for line in raw.lines() {
            let Some((k, val)) = line.split_once('|') else {
                continue;
            };
            match k {
                "size" => {
                    if let Ok(v) = val.parse::<f32>() {
                        self.size_edit = v.clamp(MIN_ARABIC, MAX_ARABIC);
                        self.arabic_size = self.size_edit.round();
                    }
                }
                "theme" => self.theme = Theme::from_u8(val.parse().unwrap_or(0)),
                "trans" => self.show_translation = val == "1",
                "tid" => {
                    if let Some(i) = TRANSLATIONS.iter().position(|t| t.id == val.trim()) {
                        self.translation_idx = i;
                    }
                }
                "audiosingle" => self.audio_single = val == "1",
                "guide" => self.guide_seen = val == "1",
                "reciter" => {
                    if let Some(i) = audio::RECITERS.iter().position(|r| r.id == val.trim()) {
                        self.reciter = i;
                    }
                }
                "tajweed" => self.show_tajweed = val == "1",
                "tsize" => {
                    if let Ok(v) = val.parse::<f32>() {
                        self.trans_size = v.clamp(MIN_TRANS, MAX_TRANS).round();
                    }
                }
                "center" => self.center_translation = val == "1",
                "harakat" => self.show_harakat = val == "1",
                "numverses" => self.numeric_verses = val == "1",
                "autospeed" => {
                    if let Ok(n) = val.trim().parse::<f32>() {
                        self.auto_speed = n.clamp(1.0, 20.0);
                    }
                }
                "spacing" => {
                    self.line_spacing =
                        (val.parse::<u32>().unwrap_or(0) as f32 / 1000.0).min(MAX_SPACING);
                }
                "widthpct" => {
                    self.width_frac =
                        (val.parse::<u32>().unwrap_or(100) as f32 / 100.0).max(MIN_WIDTH_FRAC);
                }
                "bookmarks" => {
                    self.bookmarks.clear();
                    for part in val.split(',') {
                        if let Ok(mut g) = part.parse::<u32>() {
                            if old_format {
                                g = self.data.migrate_old_index(g);
                            }
                            if (g as usize) < self.data.verses.len() {
                                self.toggle_bookmark(g);
                            }
                        }
                    }
                }
                "last" => {
                    self.restore_position = val.parse::<u32>().ok().map(|g| {
                        if old_format {
                            self.data.migrate_old_index(g)
                        } else {
                            g
                        }
                    });
                }
                "lastread" => {
                    if let Ok(mut g) = val.parse::<u32>() {
                        if old_format {
                            g = self.data.migrate_old_index(g);
                        }
                        if (g as usize) < self.data.verses.len() {
                            self.last_read = Some(g);
                        }
                    }
                }
                _ => {}
            }
        }
    }

    fn current_translation(&self) -> &'static data::TranslationSource {
        &TRANSLATIONS[self.translation_idx.min(TRANSLATIONS.len() - 1)]
    }

    /// Rough size of each surah's translation text, used to estimate heights of
    /// surahs that are not laid out yet. Depends on the chosen translation.
    fn recompute_est_trans(&mut self) {
        let idx = self.translation_idx;
        self.est_trans = (0..SURAHS.len())
            .map(|s| {
                self.data
                    .surah_translations(idx, s)
                    .iter()
                    .map(|t| t.len() as u32)
                    .sum::<u32>()
            })
            .collect();
    }

    fn set_translation(&mut self, idx: usize) {
        let idx = idx.min(TRANSLATIONS.len() - 1);
        if idx == self.translation_idx {
            return;
        }
        self.translation_idx = idx;
        self.recompute_est_trans();
        if !self.search_applied.trim().is_empty() {
            self.recompute_search();
        }
        self.dirty = true;
    }

    /// Reads the translations downloaded in an earlier run.
    fn load_downloaded_translations(&mut self) {
        if let Some(dir) = translations_dir() {
            for (i, src) in TRANSLATIONS.iter().enumerate() {
                if src.is_bundled() {
                    continue;
                }
                if let Ok(text) = std::fs::read_to_string(dir.join(src.file)) {
                    self.trans_ready[i] = self.data.set_translation_text(i, text);
                }
            }
        }
        // A translation that is not on the phone cannot stay selected.
        if !self.trans_ready.get(self.translation_idx).copied().unwrap_or(false) {
            self.translation_idx = self.trans_ready.iter().position(|r| *r).unwrap_or(0);
        }
    }

    /// Reads the tafsir files downloaded in an earlier run.
    fn load_downloaded_tafsirs(&mut self) {
        if let Some(dir) = translations_dir() {
            for (i, src) in TRANSLATIONS.iter().enumerate() {
                if let Ok(text) = std::fs::read_to_string(dir.join(src.tafsir_file)) {
                    self.tafsir_ready[i] = self.data.set_tafsir_text(i, text);
                }
            }
        }
    }

    /// Starts downloading the tafsir of translation `i` in the background.
    fn start_tafsir_download(&mut self, i: usize) {
        let Some(src) = TRANSLATIONS.get(i) else {
            return;
        };
        if self.tafsir_ready[i] && !self.tafsir_update[i] {
            return;
        }
        let Some(dir) = translations_dir() else {
            self.tafsir_msg = Some("No storage folder to save the tafsir in.".to_string());
            return;
        };
        {
            let Ok(mut g) = self.tafsir_dl.lock() else {
                return;
            };
            if g.active.is_some() {
                return; // one tafsir download at a time
            }
            g.active = Some(i);
            g.done = None;
        }
        self.tafsir_msg = None;
        let url = src.tafsir_url();
        // Saved next to the old file first, so a failed update never breaks it.
        let dest = dir.join(format!("{}.new", src.tafsir_file));
        let key = src.tafsir_key();
        let shared = self.tafsir_dl.clone();
        std::thread::spawn(move || {
            let version = fetch_remote_versions(&dir)
                .and_then(|m| m.get(&key).copied())
                .unwrap_or(1);
            let cancel = std::sync::atomic::AtomicBool::new(false);
            let result = match audio::download(&url, &dest, &cancel) {
                Ok(true) => Ok((dest, version)),
                Ok(false) => Err("cancelled".to_string()),
                Err(e) => Err(e),
            };
            if let Ok(mut g) = shared.lock() {
                g.active = None;
                g.done = Some((i, result));
            }
        });
    }

    /// Picks up a finished tafsir download / update check (called every frame).
    fn poll_tafsir_download(&mut self, ctx: &egui::Context) {
        let (active, done, updates) = match self.tafsir_dl.lock() {
            Ok(mut g) => (g.active, g.done.take(), g.updates.take()),
            Err(_) => return,
        };
        if active.is_some() {
            ctx.request_repaint_after(Duration::from_millis(300));
        }
        if let Some(list) = updates {
            for i in list {
                if self.tafsir_ready[i] {
                    self.tafsir_update[i] = true;
                }
            }
            ctx.request_repaint();
        }
        let Some((i, result)) = done else {
            return;
        };
        let name = TRANSLATIONS[i].name;
        match result {
            Ok((path, version)) => {
                let ok = std::fs::read_to_string(&path)
                    .map(|text| self.data.set_tafsir_text(i, text))
                    .unwrap_or(false);
                let dir = translations_dir();
                match (ok, &dir) {
                    (true, Some(dir)) => {
                        // The new file replaces the old one only now that it is known to be good.
                        let _ = std::fs::rename(&path, dir.join(TRANSLATIONS[i].tafsir_file));
                        write_local_version(dir, &TRANSLATIONS[i].tafsir_key(), version);
                        self.tafsir_ready[i] = true;
                        self.tafsir_update[i] = false;
                        self.tafsir_msg = None;
                    }
                    _ => {
                        let _ = std::fs::remove_file(&path);
                        self.tafsir_msg = Some(format!(
                            "The {name} tafsir file that arrived is not valid. Check its name in the release."
                        ));
                    }
                }
            }
            Err(e) => self.tafsir_msg = Some(format!("Could not download the {name} tafsir: {e}")),
        }
        ctx.request_repaint();
    }

    /// Starts downloading translation `i` in the background.
    fn start_translation_download(&mut self, i: usize) {
        let Some(src) = TRANSLATIONS.get(i) else {
            return;
        };
        if src.is_bundled() || (self.trans_ready[i] && !self.trans_update[i]) {
            return;
        }
        let Some(dir) = translations_dir() else {
            self.trans_msg = Some("No storage folder to save the translation in.".to_string());
            return;
        };
        {
            let Ok(mut g) = self.trans_dl.lock() else {
                return;
            };
            if g.active.is_some() {
                return; // one download at a time
            }
            g.active = Some(i);
            g.done = None;
        }
        self.trans_msg = None;
        let url = src.url();
        // Saved next to the old file first, so a failed update never breaks it.
        let dest = dir.join(format!("{}.new", src.file));
        let id = src.id;
        let shared = self.trans_dl.clone();
        std::thread::spawn(move || {
            // Which version is online right now (1 if the version file is missing).
            let version = fetch_remote_versions(&dir)
                .and_then(|m| m.get(id).copied())
                .unwrap_or(1);
            let cancel = std::sync::atomic::AtomicBool::new(false);
            let result = match audio::download(&url, &dest, &cancel) {
                Ok(true) => Ok((dest, version)),
                Ok(false) => Err("cancelled".to_string()),
                Err(e) => Err(e),
            };
            if let Ok(mut g) = shared.lock() {
                g.active = None;
                g.done = Some((i, result));
            }
        });
    }

    /// Looks once per run (a few seconds after start) whether a downloaded
    /// translation has a newer version online.
    fn check_translation_updates(&mut self, ctx: &egui::Context) {
        // First run (nothing downloaded yet): fetch the default translation by itself.
        if !self.trans_auto && ctx.input(|i| i.time) > 2.0 {
            self.trans_auto = true;
            if !self.trans_ready.iter().any(|r| *r) {
                self.start_translation_download(0);
            }
        }
        if self.trans_checked || ctx.input(|i| i.time) < 5.0 {
            return;
        }
        self.trans_checked = true;
        let any = TRANSLATIONS
            .iter()
            .enumerate()
            .any(|(i, t)| !t.is_bundled() && self.trans_ready[i])
            || self.tafsir_ready.iter().any(|r| *r);
        let Some(dir) = translations_dir() else {
            return;
        };
        if !any {
            return;
        }
        let shared = self.trans_dl.clone();
        let tafsir_shared = self.tafsir_dl.clone();
        std::thread::spawn(move || {
            let Some(remote) = fetch_remote_versions(&dir) else {
                return; // no version file online, or no internet: nothing to do
            };
            let local = read_local_versions(&dir);
            let newer: Vec<usize> = TRANSLATIONS
                .iter()
                .enumerate()
                .filter(|(_, t)| !t.is_bundled())
                .filter(|(_, t)| {
                    remote.get(t.id).copied().unwrap_or(0) > local.get(t.id).copied().unwrap_or(1)
                })
                .map(|(i, _)| i)
                .collect();
            if let Ok(mut g) = shared.lock() {
                g.updates = Some(newer);
            }
            let newer_tafsir: Vec<usize> = TRANSLATIONS
                .iter()
                .enumerate()
                .filter(|(_, t)| {
                    let key = t.tafsir_key();
                    remote.get(&key).copied().unwrap_or(0) > local.get(&key).copied().unwrap_or(1)
                })
                .map(|(i, _)| i)
                .collect();
            if let Ok(mut g) = tafsir_shared.lock() {
                g.updates = Some(newer_tafsir);
            }
        });
    }

    /// Picks up a finished download / update check (called every frame).
    fn poll_translation_download(&mut self, ctx: &egui::Context) {
        self.poll_tafsir_download(ctx);
        self.check_translation_updates(ctx);
        let (active, done, updates) = match self.trans_dl.lock() {
            Ok(mut g) => (g.active, g.done.take(), g.updates.take()),
            Err(_) => return,
        };
        if active.is_some() {
            ctx.request_repaint_after(Duration::from_millis(300));
        }
        if let Some(list) = updates {
            for i in list {
                if self.trans_ready[i] {
                    self.trans_update[i] = true;
                }
            }
            ctx.request_repaint();
        }
        let Some((i, result)) = done else {
            return;
        };
        let name = TRANSLATIONS[i].name;
        match result {
            Ok((path, version)) => {
                let ok = std::fs::read_to_string(&path)
                    .map(|text| self.data.set_translation_text(i, text))
                    .unwrap_or(false);
                let dir = translations_dir();
                match (ok, &dir) {
                    (true, Some(dir)) => {
                        // The new file replaces the old one only now that it is known to be good.
                        let _ = std::fs::rename(&path, dir.join(TRANSLATIONS[i].file));
                        write_local_version(dir, TRANSLATIONS[i].id, version);
                        self.trans_ready[i] = true;
                        self.trans_update[i] = false;
                        self.trans_msg = None;
                        if i == self.translation_idx {
                            // Same translation, new text: rebuild what is on screen.
                            self.recompute_est_trans();
                            self.layouts_dirty = true;
                            if !self.search_applied.trim().is_empty() {
                                self.recompute_search();
                            }
                        } else {
                            self.set_translation(i);
                        }
                    }
                    _ => {
                        let _ = std::fs::remove_file(&path);
                        self.trans_msg = Some(format!(
                            "The {name} file that arrived is not a valid translation. Check the download address."
                        ));
                    }
                }
            }
            Err(e) => self.trans_msg = Some(format!("Could not download {name}: {e}")),
        }
        ctx.request_repaint();
    }

    fn select_surah(&mut self, idx: usize) {
        let idx = idx.min(SURAHS.len() - 1);
        self.selected_surah = idx;
        self.pending_jump = Some((idx, None));
        self.dirty = true;
    }

    fn jump_to_verse(&mut self, global: u32) {
        let v = self.data.verses[global as usize];
        let surah = v.surah as usize - 1;
        let (start, _) = self.data.surah_range(surah);
        self.search_query.clear();
        self.selected_surah = surah;
        self.pending_jump = Some((surah, Some((global - start) as usize)));
        if self.compact {
            self.sidebar_open = false;
        }
        self.dirty = true;
    }

    fn toggle_bookmark(&mut self, global: u32) {
        if let Some(p) = self.bookmarks.iter().position(|b| b.global == global) {
            self.bookmarks.remove(p);
        } else {
            let v = self.data.verses[global as usize];
            let name = SURAHS[v.surah as usize - 1].translit;
            self.bookmarks.push(Bookmark {
                global,
                label: format!("{} {}:{}", name, v.surah, v.num),
            });
        }
        self.dirty = true;
    }

    fn track_scroll(&mut self, ctx: &egui::Context, offset: f32, jumped: bool, adjust: f32) {
        let now = ctx.input(|i| i.time);
        let delta = offset - self.last_offset - adjust;
        self.last_offset = offset;
        // Auto-scroll stops by itself when it reaches the end.
        if self.auto_pushed {
            if delta < 0.01 {
                self.auto_stall += 1;
                if self.auto_stall > 90 {
                    self.auto_scroll = false;
                    self.auto_stall = 0;
                }
            } else {
                self.auto_stall = 0;
            }
        }

        if jumped {
            self.scroll_accum = 0.0;
            self.bar_visible = true;
            self.bar_toggle_time = now;
            return;
        }
        if now - self.bar_toggle_time < 0.4 {
            self.scroll_accum = 0.0;
            return;
        }
        if offset < 48.0 {
            if !self.bar_visible {
                self.bar_visible = true;
                self.bar_toggle_time = now;
            }
            self.scroll_accum = 0.0;
            return;
        }
        if delta * self.scroll_accum < 0.0 {
            self.scroll_accum = 0.0;
        }
        self.scroll_accum += delta;

        let hide_sign = if HIDE_BAR_WHEN_SWIPING_UP { 1.0 } else { -1.0 };
        let moved = self.scroll_accum * hide_sign;
        if moved > BAR_SCROLL_THRESHOLD && self.bar_visible {
            self.bar_visible = false;
            self.bar_toggle_time = now;
            self.scroll_accum = 0.0;
        } else if moved < -BAR_SCROLL_THRESHOLD && !self.bar_visible {
            self.bar_visible = true;
            self.bar_toggle_time = now;
            self.scroll_accum = 0.0;
        }
    }
}

impl QuranApp {
    fn section_height(&self, s: usize) -> f32 {
        match &self.layouts[s] {
            Some(l) => l.height,
            None => self.est_section_height(s),
        }
    }

    fn est_section_height(&self, s: usize) -> f32 {
        let m = &self.m;
        let (a, b) = self.data.surah_range(s);
        let n = (b - a) as f32;
        let has_basmala = false;
        let hdr = header_height(m, has_basmala);
        if n == 0.0 {
            return hdr + MSG_H + SECTION_GAP;
        }
        let words = self.est_words[s] as f32;
        let arabic_lines = (words * (m.size * 3.2 + m.gap) / m.max_w).ceil() + n * 0.4;
        let body = if self.show_translation {
            let chars_per_line = (m.wrap_w / (m.trans_font.size * 0.5)).max(10.0);
            let trans_lines = (self.est_trans[s] as f32 / chars_per_line).ceil().max(n);
            n * (m.body_row_h + REF_GAP + REF_GAP + SEP_BLOCK)
                + arabic_lines * m.line_h
                + trans_lines * m.trans_row_h
        } else {
            arabic_lines * m.line_h
        };
        hdr + body + SECTION_GAP
    }

    fn compute_tops(&mut self) {
        self.tops.clear();
        self.tops.push(0.0);
        let mut y = 0.0f32;
        for s in 0..SURAHS.len() {
            let h = self.section_height(s);
            y += h;
            self.tops.push(y);
        }
    }

    fn ensure_layout(&mut self, ctx: &egui::Context, s: usize) {
        if self.layouts[s].is_some() {
            return;
        }
        let lay = SurahLayout::build(
            &self.data,
            ctx,
            s,
            &self.m,
            self.show_translation,
            self.translation_idx,
            &mut self.word_w,
        );
        self.layouts[s] = Some(lay);
    }

    fn verse_offset(&self, s: usize, verse: Option<usize>) -> f32 {
        let (Some(v), Some(lay)) = (verse, self.layouts[s].as_ref()) else {
            return 0.0;
        };
        let hdr = header_height(&self.m, !lay.basmala.is_empty());
        let inside = if self.show_translation {
            lay.verse_y.get(v).copied().unwrap_or(0.0)
        } else {
            let item = lay
                .verse_items
                .get(v)
                .map(|&(a, _)| a as usize)
                .unwrap_or(0);
            let line = lay
                .flow_lines
                .iter()
                .position(|l| (l.first as usize) <= item && item < l.end as usize)
                .unwrap_or(0);
            line as f32 * self.m.line_h
        };
        hdr + inside
    }

    fn current_anchor(&self) -> Option<(usize, Option<usize>)> {
        if self.tops.len() != SURAHS.len() + 1 {
            return None;
        }
        let off = self.last_offset;
        let s = section_at(&self.tops, off);
        let Some(lay) = self.layouts[s].as_ref() else {
            return Some((s, None));
        };
        let rel = off - self.tops[s] - header_height(&self.m, !lay.basmala.is_empty());
        let n = lay.shaped.len();
        if rel <= 0.0 || n == 0 {
            return Some((s, None));
        }
        let verse = if self.layout_key.translation {
            lay.verse_y
                .partition_point(|&y| y <= rel)
                .saturating_sub(1)
                .min(n - 1)
        } else {
            if lay.flow_lines.is_empty() {
                return Some((s, None));
            }
            let li = ((rel / self.m.line_h.max(1.0)) as usize).min(lay.flow_lines.len() - 1);
            lay.items[lay.flow_lines[li].first as usize].verse as usize
        };
        Some((s, Some(verse)))
    }
}

fn section_at(tops: &[f32], y: f32) -> usize {
    let n = tops.len().saturating_sub(1);
    if n == 0 {
        return 0;
    }
    tops.partition_point(|&t| t <= y)
        .saturating_sub(1)
        .min(n - 1)
}

fn header_height(m: &Metrics, has_basmala: bool) -> f32 {
    let mut y = 10.0 + m.title_h + 2.0 + m.head_h + 2.0 + m.meta_h + 6.0;
    if has_basmala {
        y += 4.0 + m.bas_h + 6.0;
    }
    y + 10.0
}

fn compute_metrics(
    ui: &egui::Ui,
    width: f32,
    size: f32,
    family: &FontFamily,
    keep_marks: bool,
    tajweed: bool,
    line_spacing: f32,
    width_frac: f32,
    center_translation: bool,
    trans_size: f32,
) -> Metrics {
    let ctx = ui.ctx();
    let style = ui.style();
    let font = FontId::new(size, family.clone());
    let title_font = FontId::new(size * 1.25, family.clone());
    let body_font = egui::TextStyle::Body.resolve(style);
    let trans_font = FontId::new(trans_size, body_font.family.clone());
    let head_font = egui::TextStyle::Heading.resolve(style);
    let meta_font = egui::TextStyle::Small.resolve(style);
    let bas_font = FontId::new(size.min(width / 8.5), family.clone());
    let row = |f: &FontId| ctx.fonts_mut(|fo| fo.row_height(f));
    // At least 2.3x the font size, so stacked harakat are never clipped.
    let line_h = (size * 2.3).max(row(&font) + size * (0.35 + line_spacing));
    let body_row_h = row(&body_font);
    let trans_row_h = row(&trans_font);
    let title_h = row(&title_font);
    let head_h = row(&head_font);
    let meta_h = row(&meta_font);
    let bas_h = row(&bas_font);
    let w = width.max(60.0) * width_frac.clamp(MIN_WIDTH_FRAC, 1.0);
    Metrics {
        size,
        line_h,
        gap: size * 0.4,
        marker_d: size * 1.15,
        numeric_verses: false,
        max_w: w,
        wrap_w: w,
        keep_marks,
        tajweed,
        center_translation,
        font,
        ref_font: FontId::new(REF_SIZE, family.clone()),
        body_font,
        body_row_h,
        trans_font,
        trans_row_h,
        title_font,
        title_h,
        head_font,
        head_h,
        meta_font,
        meta_h,
        bas_font,
        bas_h,
    }
}

fn translation_job(text: &str, m: &Metrics) -> egui::text::LayoutJob {
    let mut job = egui::text::LayoutJob::simple(
        text.to_owned(),
        m.trans_font.clone(),
        Color32::PLACEHOLDER,
        m.wrap_w,
    );
    job.halign = if m.center_translation {
        Align::Center
    } else {
        Align::Min
    };
    job
}

fn translation_galley_height(ctx: &egui::Context, text: &str, m: &Metrics) -> f32 {
    ctx.fonts_mut(|f| f.layout_job(translation_job(text, m)).size().y)
}

impl QuranApp {
    fn recompute_search(&mut self) {
        self.search_applied.clear();
        self.search_applied.push_str(&self.search_query);
        self.hits.clear();
        self.hit_heights.clear();
        self.surah_hits.clear();
        self.search_status.clear();

        let q = self.search_query.trim();
        if q.is_empty() {
            return;
        }

        for (i, info) in SURAHS.iter().enumerate() {
            if self.surah_hits.len() >= 8 {
                break;
            }
            if surah_matches(info, q) {
                self.surah_hits.push(i as u16);
            }
        }

        let mut direct: Option<u32> = None;
        if let Some((s, a)) = parse_reference(q) {
            if let Ok(i) = self
                .data
                .verses
                .binary_search_by_key(&(s, a), |v| (v.surah, v.num))
            {
                direct = Some(i as u32);
            }
        }

        let needle_is_arabic = q.chars().any(arabic::is_arabic_char);
        self.needle.clear();
        if needle_is_arabic {
            arabic::normalize_into(q, &mut self.needle);
        }
        let mut scratch = String::new();
        for (i, v) in self.data.verses.iter().enumerate() {
            if self.hits.len() >= MAX_HITS {
                break;
            }
            let g = i as u32;
            if Some(g) == direct {
                continue;
            }
            let found = contains_ci(self.data.translation(self.translation_idx, i), q)
                || (needle_is_arabic && {
                    scratch.clear();
                    arabic::normalize_into(v.arabic, &mut scratch);
                    scratch.contains(self.needle.as_str())
                });
            if found {
                self.hits.push(make_hit(&self.data, g, self.show_harakat, self.show_tajweed));
            }
        }
        if let Some(g) = direct {
            self.hits
                .insert(0, make_hit(&self.data, g, self.show_harakat, self.show_tajweed));
        }

        self.hit_heights.resize(self.hits.len(), 160.0);
        self.search_status = format!(
            "{} verse match(es){} \u{b7} {} surah match(es)",
            self.hits.len(),
            if self.hits.len() >= MAX_HITS { "+" } else { "" },
            self.surah_hits.len()
        );
    }
}

fn make_hit(data: &QuranDataset, global: u32, keep_marks: bool, tajweed: bool) -> Hit {
    let v = &data.verses[global as usize];
    Hit {
        verse: global,
        label: format!(
            "{} {}:{}",
            SURAHS[v.surah as usize - 1].translit,
            v.surah,
            v.num
        ),
        shaped_ar: arabic::shape_words_ex(v.arabic, keep_marks, tajweed),
    }
}

/// The app's own button font (family and size), as set in the theme.
fn system_button_font(ui: &egui::Ui) -> FontId {
    ui.style()
        .text_styles
        .get(&egui::TextStyle::Button)
        .cloned()
        .unwrap_or_else(|| FontId::proportional(17.0))
}

/// Width of `text` at `size`, in points (in the button font family).
fn text_width(ui: &egui::Ui, text: &str, size: f32) -> f32 {
    let family = system_button_font(ui).family;
    ui.fonts_mut(|f| {
        f.layout_no_wrap(text.to_owned(), FontId::new(size, family), Color32::WHITE)
            .size()
            .x
    })
}

/// Picks a font size and side padding so that a row of pill buttons fits in the
/// available width (the labels are never cut off or squeezed).
fn fit_button_row(ui: &egui::Ui, labels: &[&str], gap: f32) -> (f32, f32) {
    let avail = ui.available_width();
    // Start from the system (theme) button size; only shrink if the row would not fit.
    let mut size = system_button_font(ui).size.round();
    let mut pad = 10.0f32;
    loop {
        let total: f32 = labels
            .iter()
            .map(|l| text_width(ui, l, size) + pad * 2.0)
            .sum::<f32>()
            + gap * (labels.len().saturating_sub(1)) as f32;
        if total <= avail || (size <= 11.0 && pad <= 4.0) {
            return (size, pad);
        }
        if pad > 4.0 {
            pad -= 1.0;
        } else {
            size -= 1.0;
        }
    }
}

/// A round (pill) button whose text is placed on whole pixels, so small text
/// stays sharp and always sits fully inside the button.
// ---- First-use guide -----------------------------------------------------------
//
// The pictures are raw pixel files made from your annotated screenshots (no image
// decoder needed): `.rgb` = 3 bytes per pixel, `.rgba` = 4 bytes per pixel.

struct GuideImage {
    bytes: &'static [u8],
    w: usize,
    h: usize,
    alpha: bool,
}

const GUIDE_IMAGES: &[GuideImage] = &[
    GuideImage {
        bytes: include_bytes!("../assets/guide/guide_logo.rgba"),
        w: 400,
        h: 400,
        alpha: true,
    },
    GuideImage {
        bytes: include_bytes!("../assets/guide/guide_1.rgb"),
        w: 800,
        h: 800,
        alpha: false,
    },
    GuideImage {
        bytes: include_bytes!("../assets/guide/guide_2.rgb"),
        w: 800,
        h: 600,
        alpha: false,
    },
    GuideImage {
        bytes: include_bytes!("../assets/guide/guide_3.rgb"),
        w: 800,
        h: 800,
        alpha: false,
    },
    GuideImage {
        bytes: include_bytes!("../assets/guide/guide_4.rgb"),
        w: 800,
        h: 800,
        alpha: false,
    },
];

struct GuidePage {
    title: &'static str,
    text: &'static str,
    /// Index into `GUIDE_IMAGES`.
    image: Option<usize>,
}

const GUIDE_PAGES: &[GuidePage] = &[
    GuidePage {
        title: "Welcome",
        text: "Al-Qur'an Al-Kareem, Ekur'ani entukuvu mu Luganda. A one-minute tour shows you how to read, listen and search.",
        image: Some(0),
    },
    GuidePage {
        title: "Two ways to read",
        text: "Turn the switch in the top bar ON to see a translation under every verse. Turn it OFF for a single-page Quran. Slide anywhere to the right to open the side bar.",
        image: Some(1),
    },
    GuidePage {
        title: "The side bar",
        text: "The settings are at the top: Translation, Audio, Reader settings, Jump to Juz' and Bookmarks. Type a surah name (English or Arabic) or its number in the filter, or scroll the list and tap a surah.",
        image: Some(2),
    },
    GuidePage {
        title: "Change the text size",
        text: "Zoom in and out with two fingers to make the Arabic text bigger or smaller.",
        image: Some(3),
    },
    GuidePage {
        title: "Long-press a verse",
        text: "Press and hold a verse to see its translation. Switch between Luganda, English, Runyankole and Kiswahili, share the verse, open its tafsir or bookmark it. Tap Day, Sepia or Night at the top right to change the theme.",
        image: Some(4),
    },
    GuidePage {
        title: "Listen and continue",
        text: "Open the side bar, then Audio, to choose a reciter and download a surah. Tap any verse to hear it. The app always reopens at the last verse you tapped.",
        image: None,
    },
];

impl QuranApp {
    /// The welcome guide: a dimmed screen with one card per page.
    fn guide_overlay(&mut self, ctx: &egui::Context) {
        let Some(page_idx) = self.guide_page else {
            return;
        };
        let page_idx = page_idx.min(GUIDE_PAGES.len() - 1);
        let page = &GUIDE_PAGES[page_idx];
        let screen = ctx.content_rect();

        // Dim everything and swallow touches, so the reader underneath stays still.
        egui::Area::new(egui::Id::new("guide_dim"))
            .order(egui::Order::Foreground)
            .fixed_pos(screen.min)
            .show(ctx, |ui| {
                let (rect, _) = ui.allocate_exact_size(screen.size(), Sense::click_and_drag());
                ui.painter()
                    .rect_filled(rect, CornerRadius::ZERO, Color32::from_black_alpha(190));
            });

        // Load this page's picture once (freed again when the guide closes).
        if let Some(i) = page.image {
            if self.guide_tex[i].is_none() {
                let img = &GUIDE_IMAGES[i];
                let per_pixel = if img.alpha { 4 } else { 3 };
                if img.bytes.len() == img.w * img.h * per_pixel {
                    let color = if img.alpha {
                        egui::ColorImage::from_rgba_unmultiplied([img.w, img.h], img.bytes)
                    } else {
                        egui::ColorImage::from_rgb([img.w, img.h], img.bytes)
                    };
                    self.guide_tex[i] = Some(ctx.load_texture(
                        format!("guide_{i}"),
                        color,
                        egui::TextureOptions::LINEAR,
                    ));
                }
            }
        }

        let th = self.theme;
        let last = page_idx + 1 == GUIDE_PAGES.len();
        let card_w = (screen.width() * 0.92).min(480.0);
        let max_img_h = screen.height() * 0.45;
        let mut action = 0; // -1 back, 1 next, 2 finish
        egui::Window::new("guide_card")
            .title_bar(false)
            .collapsible(false)
            .resizable(false)
            .movable(false)
            .order(egui::Order::Tooltip)
            .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
            .default_width(card_w)
            .max_width(card_w)
            .frame(theme::window_frame(th))
            .show(ctx, |ui| {
                if let Some(i) = page.image {
                    if let Some(tex) = self.guide_tex[i].as_ref() {
                        let img = &GUIDE_IMAGES[i];
                        let mut w = ui.available_width();
                        let mut h = w * img.h as f32 / img.w as f32;
                        if h > max_img_h {
                            w *= max_img_h / h;
                            h = max_img_h;
                        }
                        ui.vertical_centered(|ui| {
                            ui.image(egui::load::SizedTexture::new(tex.id(), egui::vec2(w, h)));
                        });
                        ui.add_space(6.0);
                    }
                }
                ui.label(RichText::new(page.title).size(22.0).strong());
                ui.add_space(4.0);
                ui.label(RichText::new(page.text).size(16.0));
                ui.add_space(10.0);

                // Page dots.
                ui.vertical_centered(|ui| {
                    let n = GUIDE_PAGES.len();
                    let (rect, _) =
                        ui.allocate_exact_size(egui::vec2(n as f32 * 16.0, 12.0), Sense::hover());
                    for k in 0..n {
                        let c = egui::pos2(rect.left() + 8.0 + k as f32 * 16.0, rect.center().y);
                        let color = if k == page_idx {
                            theme::accent(th)
                        } else {
                            ui.visuals().weak_text_color()
                        };
                        ui.painter()
                            .circle_filled(c, if k == page_idx { 5.0 } else { 3.5 }, color);
                    }
                });
                ui.add_space(8.0);

                ui.horizontal(|ui| {
                    if !last && pill_button(ui, "Skip", false, 15.0, 12.0).clicked() {
                        action = 2;
                    }
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        let next = if last { "Start reading" } else { "Next" };
                        if pill_button(ui, next, true, 15.0, 14.0).clicked() {
                            action = if last { 2 } else { 1 };
                        }
                        if page_idx > 0 && pill_button(ui, "Back", false, 15.0, 12.0).clicked() {
                            action = -1;
                        }
                    });
                });
            });

        match action {
            1 => self.guide_page = Some((page_idx + 1).min(GUIDE_PAGES.len() - 1)),
            -1 => self.guide_page = Some(page_idx.saturating_sub(1)),
            2 => {
                self.guide_page = None;
                self.guide_seen = true;
                self.dirty = true;
                self.save_file_only();
                for t in self.guide_tex.iter_mut() {
                    *t = None;
                }
            }
            _ => {}
        }
    }
}

fn pill_button(
    ui: &mut egui::Ui,
    text: &str,
    selected: bool,
    font_size: f32,
    pad_x: f32,
) -> egui::Response {
    let ppp = ui.ctx().pixels_per_point();
    let snap = |v: f32| (v * ppp).round() / ppp;
    let font = FontId::new(font_size, system_button_font(ui).family);
    // Measure first, so the pill is always wide enough for its text.
    let measure = ui.fonts_mut(|f| f.layout_no_wrap(text.to_owned(), font.clone(), Color32::WHITE));
    let height = snap(measure.size().y + 14.0).max(32.0);
    let size = egui::vec2(snap(measure.size().x + pad_x * 2.0).max(height), height);
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
    let visuals = ui.style().interact_selectable(&resp, selected);
    let (fill, stroke, text_color) = if selected {
        let sel = ui.visuals().selection;
        (sel.bg_fill, sel.stroke, sel.stroke.color)
    } else {
        (visuals.weak_bg_fill, visuals.bg_stroke, visuals.text_color())
    };
    ui.painter().rect(
        rect,
        CornerRadius::same(255),
        fill,
        stroke,
        egui::StrokeKind::Inside,
    );
    // Lay the text out again in its real colour (the same way plain labels are
    // drawn) and put it on whole pixels, centred in the pill.
    let galley = ui.fonts_mut(|f| f.layout_no_wrap(text.to_owned(), font, text_color));
    let pos = egui::pos2(
        snap(rect.center().x - galley.size().x * 0.5),
        snap(rect.center().y - galley.size().y * 0.5),
    );
    ui.painter().galley(pos, galley, text_color);
    resp
}

/// Arabic-Indic (\u{0660}-\u{0669}) and Persian digits to plain digits, so a search
/// typed with an Arabic keyboard ("\u{0662}:\u{0662}\u{0665}\u{0665}") works like "2:255".
fn ascii_digits(q: &str) -> String {
    q.chars()
        .map(|c| match c {
            '\u{0660}'..='\u{0669}' => char::from(b'0' + (c as u32 - 0x0660) as u8),
            '\u{06F0}'..='\u{06F9}' => char::from(b'0' + (c as u32 - 0x06F0) as u8),
            other => other,
        })
        .collect()
}

fn parse_reference(q: &str) -> Option<(u16, u16)> {
    let q = ascii_digits(q);
    let (a, b) = q.split_once(':')?;
    Some((a.trim().parse().ok()?, b.trim().parse().ok()?))
}

fn parse_juz(q: &str) -> Option<u8> {
    let t = ascii_digits(q).trim().to_ascii_lowercase();
    let stripped = t
        .strip_prefix("juz")
        .or_else(|| t.strip_prefix("جزء"))
        .or_else(|| t.strip_prefix("para"));
    let n: u8 = stripped.unwrap_or(&t).trim().parse().ok()?;
    (1..=30).contains(&n).then_some(n)
}

fn surah_matches(info: &SurahInfo, filter: &str) -> bool {
    let f = ascii_digits(filter.trim());
    let f = f.as_str();
    if f.is_empty() {
        return true;
    }
    if let Ok(n) = f.parse::<u16>() {
        return info.number == n;
    }
    // Arabic name: ignore harakat and alef/ya/ta-marbuta variants, like verse search.
    if f.chars().any(arabic::is_arabic_char) {
        let (mut name, mut want) = (String::new(), String::new());
        arabic::normalize_into(info.arabic, &mut name);
        arabic::normalize_into(f, &mut want);
        if !want.is_empty() && name.contains(want.as_str()) {
            return true;
        }
    }
    contains_ci(info.translit, f) || contains_ci(info.english, f) || info.arabic.contains(f)
}

impl eframe::App for QuranApp {
    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        self.save_into(storage);
    }

    fn auto_save_interval(&self) -> Duration {
        Duration::from_secs(15)
    }

    fn update(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        theme::paint_backdrop(ctx, self.theme);

        if !ctx.input(|i| i.pointer.any_down()) {
            self.press.target = None;
        }
        if self.search_query != self.search_applied {
            self.recompute_search();
        }
        self.poll_translation_download(ctx);
        self.handle_back(ctx);
        self.sidebar_swipe(ctx);
        self.audio_tick(ctx);
        if self.sidebar_open != self.prev_sidebar_open {
            self.prev_sidebar_open = self.sidebar_open;
            self.sidebar_toggle_time = ctx.input(|i| i.time);
        }

        self.status_inset(ctx);
        // Bottom panels stack upwards: the keyboard is lowest, the bar sits above it.
        self.arabic_keyboard(ctx);
        self.top_bar(ctx);
        self.sidebar(ctx);
        // Height used at the bottom (bar / keyboard, animated): floating pills sit above it.
        self.bottom_inset = (ctx.content_rect().bottom() - ctx.available_rect().bottom()).max(0.0);
        self.central(ctx);
        self.position_pill(ctx);
        self.quick_jump_window(ctx);
        self.verse_menu(ctx);
        self.audio_bar(ctx);
        self.guide_overlay(ctx);

        let now = ctx.input(|i| i.time);
        if self.dirty && now - self.last_persist > 3.0 {
            self.last_persist = now;
            if let Some(storage) = frame.storage_mut() {
                self.save_into(storage);
            } else {
                self.save_file_only();
            }
            self.dirty = false;
        }
    }
}

impl QuranApp {
    // ------------------------------------------------------------------ audio
    fn toast(&mut self, ctx: &egui::Context, msg: impl Into<String>) {
        let mut m: String = msg.into();
        if m.chars().count() > 70 {
            m = m.chars().take(70).collect::<String>() + "\u{2026}";
        }
        self.audio_msg = m;
        self.audio_msg_until = ctx.input(|i| i.time) + 3.0;
        ctx.request_repaint_after(Duration::from_millis(3100));
    }

    fn surah_audio_verses(&self, s: usize) -> Vec<(u16, u16)> {
        self.data
            .surah_verses(s)
            .iter()
            .map(|v| (v.surah, v.ayah))
            .collect()
    }

    /// Is the whole surah downloaded? Checked at most once a second.
    fn surah_audio_ready(&mut self, ctx: &egui::Context, s: usize) -> bool {
        let now = ctx.input(|i| i.time);
        if self.audio_ready_cache.0 != s || now - self.audio_ready_cache.2 > 1.0 {
            let ready = audio::all_exist(self.reciter, &self.surah_audio_verses(s));
            self.audio_ready_cache = (s, ready, now);
        }
        self.audio_ready_cache.1
    }

    fn stop_audio(&mut self) {
        self.audio_player.stop();
        self.audio_playing = None;
        self.audio_paused = false;
    }

    /// Plays one verse, then (unless "one verse only" is on) the rest of the surah.
    fn play_audio(&mut self, ctx: &egui::Context, global: u32, follow: bool) {
        if !audio::AVAILABLE {
            return;
        }
        let Some(v) = self.data.verses.get(global as usize).copied() else {
            return;
        };
        let s = v.surah as usize - 1;
        let (start, end) = self.data.surah_range(s);
        let Some(path) = audio::verse_path(self.reciter, v.surah, v.ayah) else {
            return;
        };
        if !audio::verse_exists(self.reciter, v.surah, v.ayah) {
            self.stop_audio();
            if !self.audio_hint_shown {
                self.audio_hint_shown = true;
                self.toast(ctx, "Download this surah's audio first: menu > Audio");
            }
            return;
        }
        match self.audio_player.play(&path) {
            Ok(()) => {
                self.audio_playing = Some(global);
                self.audio_end = end;
                self.audio_paused = false;
                if follow {
                    self.pending_jump = Some((s, Some((global - start) as usize)));
                }
                ctx.request_repaint();
            }
            Err(e) => {
                self.stop_audio();
                self.toast(ctx, format!("Cannot play audio: {e}"));
            }
        }
    }

    /// Called every frame: moves on to the next verse and tracks downloads.
    fn audio_tick(&mut self, ctx: &egui::Context) {
        let mut finished_msg: Option<String> = None;
        if let Some(d) = &self.audio_download {
            if d.is_finished() {
                finished_msg = Some(match d.error() {
                    Some(e) => format!("Download failed: {e}"),
                    None if d.is_cancelled() => "Download cancelled".to_owned(),
                    None => "Audio downloaded".to_owned(),
                });
            } else {
                ctx.request_repaint_after(Duration::from_millis(250));
            }
        }
        if let Some(m) = finished_msg {
            self.audio_download = None;
            self.audio_ready_cache.2 = -10.0;
            self.toast(ctx, m);
        }

        if let Some(cur) = self.audio_playing {
            if !self.audio_paused {
                if self.audio_player.is_finished() {
                    let next = cur + 1;
                    if self.audio_single || next >= self.audio_end {
                        self.stop_audio();
                    } else {
                        self.play_audio(ctx, next, true);
                    }
                }
                ctx.request_repaint_after(Duration::from_millis(150));
            }
        }

        if !self.audio_msg.is_empty() && ctx.input(|i| i.time) > self.audio_msg_until {
            self.audio_msg.clear();
        }
    }

    /// Small floating bar with Pause / Stop while audio plays, and short messages.
    fn audio_bar(&mut self, ctx: &egui::Context) {
        let playing = self.audio_playing.is_some();
        if !playing && self.audio_msg.is_empty() {
            return;
        }
        let th = self.theme;
        let paused = self.audio_paused;
        let msg = self.audio_msg.clone();
        let mut toggle = false;
        let mut stop = false;
        let lift = self.bottom_inset;
        egui::Area::new(egui::Id::new("audio_bar"))
            .order(egui::Order::Foreground)
            .anchor(egui::Align2::CENTER_BOTTOM, egui::vec2(0.0, -76.0 - lift))
            .show(ctx, |ui| {
                theme::window_frame(th).show(ui, |ui| {
                    ui.horizontal(|ui| {
                        if playing {
                            let label = if paused { "Resume" } else { "Pause" };
                            if pill_button(ui, label, false, 14.0, 10.0).clicked() {
                                toggle = true;
                            }
                            if pill_button(ui, "Stop", false, 14.0, 10.0).clicked() {
                                stop = true;
                            }
                        }
                        if !msg.is_empty() {
                            ui.label(RichText::new(msg.as_str()).size(14.0));
                        }
                    });
                });
            });
        if toggle {
            if self.audio_paused {
                self.audio_player.resume();
                self.audio_paused = false;
            } else {
                self.audio_player.pause();
                self.audio_paused = true;
            }
        }
        if stop {
            self.stop_audio();
        }
    }

    fn run_audio_action(&mut self, ctx: &egui::Context, act: AudioAction) {
        match act {
            AudioAction::Download(s) => {
                if self.audio_download.is_some() {
                    return;
                }
                let verses = self.surah_audio_verses(s);
                match audio::start_download(self.reciter, s, &verses) {
                    Some(d) => {
                        self.audio_download = Some(d);
                        ctx.request_repaint();
                    }
                    None => self.toast(ctx, "Audio storage is not available"),
                }
            }
            AudioAction::Cancel => {
                if let Some(d) = &self.audio_download {
                    d.cancel();
                }
            }
            AudioAction::PlaySurah(s) => {
                let (start, _) = self.data.surah_range(s);
                self.play_audio(ctx, start, true);
                if self.compact {
                    self.sidebar_open = false;
                }
            }
            AudioAction::Delete(s) => {
                let (start, end) = self.data.surah_range(s);
                if let Some(p) = self.audio_playing {
                    if p >= start && p < end {
                        self.stop_audio();
                    }
                }
                // The Bismillah entries share the recording of 1:1, so keep it.
                let verses: Vec<(u16, u16)> = self
                    .surah_audio_verses(s)
                    .into_iter()
                    .filter(|&(_, a)| a != 0)
                    .collect();
                audio::delete(self.reciter, &verses);
                self.audio_ready_cache.2 = -10.0;
                self.toast(ctx, "Audio deleted");
            }
        }
    }

    /// Android only: horizontal swipe gestures open/close the sidebar (it still
    /// uses the normal slide animation). Other platforms keep the button only.
    fn sidebar_swipe(&mut self, ctx: &egui::Context) {
        if !cfg!(target_os = "android") {
            return;
        }
        let (pressed, down, released, pos, multi) = ctx.input(|i| {
            (
                i.pointer.any_pressed(),
                i.pointer.any_down(),
                i.pointer.any_released(),
                i.pointer.interact_pos(),
                i.multi_touch().is_some(),
            )
        });
        if pressed {
            self.swipe_start = pos;
            self.swipe_last = pos;
        }
        if down {
            if let Some(p) = pos {
                self.swipe_last = Some(p);
            }
        }
        if multi {
            self.swipe_start = None;
        }
        if !released {
            return;
        }
        let (start, last) = (self.swipe_start.take(), self.swipe_last.take());
        let blocked =
            self.quick_jump || self.long_press_state.is_some() || self.guide_page.is_some();
        let (Some(a), Some(b)) = (start, last) else {
            return;
        };
        if blocked {
            return;
        }
        let d = b - a;
        if d.x.abs() < SWIPE_MIN || d.x.abs() < d.y.abs() * 2.0 {
            return;
        }
        if d.x > 0.0 && !self.sidebar_open && a.x < SWIPE_EDGE {
            self.sidebar_open = true;
            ctx.request_repaint();
        } else if d.x < 0.0 && self.sidebar_open {
            self.sidebar_open = false;
            ctx.request_repaint();
        }
    }

    /// Android's Back button (Esc on desktop). Closes whatever is open, one layer
    /// at a time; when nothing is open it leaves the app.
    fn handle_back(&mut self, ctx: &egui::Context) {
        let pressed = ctx.input(|i| {
            i.events.iter().any(|e| {
                matches!(
                    e,
                    egui::Event::Key {
                        key: egui::Key::BrowserBack | egui::Key::Escape,
                        pressed: true,
                        ..
                    }
                )
            })
        });
        if !pressed {
            return;
        }
        if self.guide_page.is_some() {
            // Closing the guide counts as finishing it.
            self.guide_page = None;
            self.guide_seen = true;
            self.dirty = true;
            self.save_file_only();
            for t in self.guide_tex.iter_mut() {
                *t = None;
            }
        } else if self.long_press_state.is_some() {
            self.long_press_state = None;
        } else if self.quick_jump {
            self.quick_jump = false;
        } else if self.ar_kbd_open {
            self.ar_kbd_open = false;
        } else if self.sidebar_open {
            self.sidebar_open = false;
        } else if !self.search_query.is_empty() || !self.search_applied.is_empty() {
            self.search_query.clear();
        } else {
            // Nothing left to close: leave the app.
            self.stop_audio();
            self.save_file_only();
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
        ctx.request_repaint();
    }

    fn status_inset(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::top("status_inset")
            .frame(theme::inset_frame(self.theme))
            .exact_height(TOP_INSET)
            .show(ctx, |_ui| {});
    }

    fn top_bar(&mut self, ctx: &egui::Context) {
        let visible = self.bar_visible
            || self.sidebar_open
            || self.ar_kbd_open
            || !self.search_applied.trim().is_empty();

        egui::TopBottomPanel::bottom("top_bar")
            .frame(
                egui::Frame::new()
                    .fill(theme::bar_frame(self.theme).fill)
                    .stroke(theme::bar_frame(self.theme).stroke)
                    .inner_margin(egui::Margin::symmetric(12, 2)), // Slimmer height
            )
            .show_animated(ctx, visible, |ui| {
                theme::square_buttons(ui.visuals_mut());
                ui.horizontal(|ui| {
                    // Slim Menu Button: shows the surah the reader is in right now.
                    let cur = self
                        .current_anchor()
                        .map(|(s, _)| s)
                        .unwrap_or(self.selected_surah)
                        .min(SURAHS.len() - 1);
                    let name = SURAHS[cur].translit;
                    let name = if name.chars().count() > 10 {
                        name.chars().take(9).collect::<String>() + "\u{2026}"
                    } else {
                        name.to_owned()
                    };
                    if ui
                        .add(
                            egui::Button::new(RichText::new(format!("\u{2630} {name}")).size(18.0))
                                .frame(false),
                        )
                        .clicked()
                    {
                        self.sidebar_open = !self.sidebar_open;
                    }

                    // Modern Search Bar (Fills available space)
                    let avail_w = ui.available_width();
                    let search_w = (avail_w - 175.0).max(70.0);
                    let search_resp = ui.add_sized(
                        [search_w, 26.0],
                        egui::TextEdit::singleline(&mut self.search_query)
                            .hint_text(RichText::new("Search anything...").size(14.0))
                            .margin(egui::Margin::symmetric(8, 4)), // Removed corner_radius()
                    );
                    if search_resp.has_focus() {
                        self.ar_target = 1;
                    }
                    if search_resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                        self.quick_jump = true;
                    }
                    // Opens the on-screen Arabic keyboard (drawn keyboard icon).
                    let (kbd_rect, kbd_resp) =
                        ui.allocate_exact_size(egui::vec2(32.0, 26.0), Sense::click());
                    if self.ar_kbd_open || kbd_resp.hovered() {
                        ui.painter().rect_filled(
                            kbd_rect,
                            CornerRadius::same(4),
                            ui.visuals().widgets.hovered.bg_fill,
                        );
                    }
                    paint_keyboard_icon(ui.painter(), kbd_rect.center(), ui.visuals().text_color());
                    if kbd_resp.clicked() {
                        self.ar_kbd_open = !self.ar_kbd_open;
                        self.ar_target = 1;
                    }



                    // Translation Toggle & Theme Toggle (Text only)
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui
                            .add(
                                egui::Button::new(RichText::new(self.theme.label()).size(20.0))
                                    .frame(false),
                            )
                            .clicked()
                        {
                            self.theme = self.theme.next();
                            theme::apply(ctx, self.theme);
                            self.dirty = true;
                            ctx.request_repaint();
                        }
                        ui.separator();
                        toggle_switch(ui, &mut self.show_translation);
                    });
                });
            });
    }

    fn sidebar(&mut self, ctx: &egui::Context) {
        let screen_w = ctx.screen_rect().width();
        let compact = screen_w < 700.0;
        self.compact = compact;

        let mut panel =
            egui::SidePanel::left("surah_panel").frame(theme::sidebar_frame(self.theme));
        panel = if compact {
            panel
                .resizable(false)
                .exact_width((screen_w - 56.0).max(220.0))
        } else {
            panel
                .resizable(true)
                .default_width(340.0)
                .width_range(260.0..=520.0)
        };

        let ar_font = FontId::new(20.0, self.arabic_family.clone());
        let mut chosen: Option<usize> = None;
        let mut jump: Option<u32> = None;
        let mut marks_changed = false;
        let mut settings_changed = false;
        let mut pick_translation: Option<usize> = None;
        let mut start_download: Option<usize> = None;
        let mut start_tafsir: Option<usize> = None;
        let mut audio_action: Option<AudioAction> = None;
        let mut reciter_changed = false;
        let mut show_guide = false;
        let audio_s = self
            .current_anchor()
            .map(|(s, _)| s)
            .unwrap_or(self.selected_surah);
        let audio_ready = if audio::AVAILABLE && self.sidebar_open {
            self.surah_audio_ready(ctx, audio_s)
        } else {
            false
        };

        let mut sidebar_section = self.sidebar_section;
        panel.show_animated(ctx, self.sidebar_open, |ui| {
            section(ui, &mut sidebar_section, 0, "Translation", |ui| {
                    let downloading = self.trans_dl.lock().ok().and_then(|g| g.active);
                    let tafsir_busy = self.tafsir_dl.lock().ok().and_then(|g| g.active);
                    for (i, t) in TRANSLATIONS.iter().enumerate() {
                        let label = if t.language.is_empty() {
                            t.name.to_owned()
                        } else {
                            format!("{} \u{b7} {}", t.name, t.language)
                        };
                        if downloading == Some(i) {
                            ui.horizontal(|ui| {
                                ui.add(egui::Spinner::new());
                                let what = if self.trans_ready[i] { "updating" } else { "downloading" };
                                ui.label(format!("{label} \u{2013} {what}..."));
                            });
                        } else if self.trans_ready[i] {
                            ui.horizontal(|ui| {
                                if ui
                                    .selectable_label(i == self.translation_idx, label)
                                    .clicked()
                                {
                                    pick_translation = Some(i);
                                }
                                if self.trans_update[i]
                                    && ui
                                        .add_enabled(downloading.is_none(), egui::Button::new("Update available").small())
                                        .clicked()
                                {
                                    start_download = Some(i);
                                }
                            });
                        } else if ui
                            .add_enabled(
                                downloading.is_none(),
                                egui::Button::new(format!("{label} \u{2013} Download ({})", t.size)),
                            )
                            .clicked()
                        {
                            start_download = Some(i);
                        }
                        // The tafsir that goes with this translation.
                        if self.trans_ready[i] {
                            ui.horizontal(|ui| {
                                ui.add_space(18.0);
                                if tafsir_busy == Some(i) {
                                    ui.add(egui::Spinner::new());
                                    ui.label(RichText::new("tafsir downloading...").small());
                                } else if self.tafsir_ready[i] {
                                    ui.label(RichText::new("Tafsir ready").small().weak());
                                    if self.tafsir_update[i]
                                        && ui
                                            .add_enabled(
                                                tafsir_busy.is_none(),
                                                egui::Button::new("Tafsir update available").small(),
                                            )
                                            .clicked()
                                    {
                                        start_tafsir = Some(i);
                                    }
                                } else {
                                    let label = if t.tafsir_size.is_empty() {
                                        "Download tafsir".to_string()
                                    } else {
                                        format!("Download tafsir ({})", t.tafsir_size)
                                    };
                                    if ui
                                        .add_enabled(tafsir_busy.is_none(), egui::Button::new(label).small())
                                        .clicked()
                                    {
                                        start_tafsir = Some(i);
                                    }
                                }
                            });
                        }
                    }
                    if downloading.is_none()
                        && self.trans_msg.is_none()
                        && !self.trans_ready.iter().any(|r| *r)
                    {
                        ui.weak("Download a translation to read it (internet is needed once).");
                    }
                    if let Some(msg) = &self.trans_msg {
                        ui.colored_label(Color32::from_rgb(220, 120, 40), msg.as_str());
                    }
                    if let Some(msg) = &self.tafsir_msg {
                        ui.colored_label(Color32::from_rgb(220, 120, 40), msg.as_str());
                    }
                    if !self.show_translation {
                        ui.weak("Use the switch in the top bar to show translations.");
                    }
                });

            if audio::AVAILABLE {
                section(ui, &mut sidebar_section, 1, "Audio", |ui| {
                    // Reciter choice (locked while a download is running).
                    ui.add_enabled_ui(self.audio_download.is_none(), |ui| {
                        egui::ComboBox::from_id_salt("reciter_pick")
                            .selected_text(audio::RECITERS[self.reciter].name)
                            .show_ui(ui, |ui| {
                                for (i, r) in audio::RECITERS.iter().enumerate() {
                                    if ui.selectable_value(&mut self.reciter, i, r.name).changed() {
                                        reciter_changed = true;
                                    }
                                }
                            });
                    });
                    ui.weak(SURAHS[audio_s].translit);
                    if let Some(d) = &self.audio_download {
                        let frac = d.done() as f32 / d.total.max(1) as f32;
                        ui.add(
                            egui::ProgressBar::new(frac)
                                .text(format!("Downloading {}/{}", d.done(), d.total)),
                        );
                        if pill_button(ui, "Cancel", false, 14.0, 10.0).clicked() {
                            audio_action = Some(AudioAction::Cancel);
                        }
                    } else if audio_ready {
                        ui.horizontal_wrapped(|ui| {
                            if pill_button(ui, "Play surah", false, 14.0, 10.0).clicked() {
                                audio_action = Some(AudioAction::PlaySurah(audio_s));
                            }
                            if pill_button(ui, "Delete", false, 14.0, 10.0).clicked() {
                                audio_action = Some(AudioAction::Delete(audio_s));
                            }
                        });
                    } else if pill_button(ui, "Download this surah", false, 14.0, 10.0).clicked() {
                        audio_action = Some(AudioAction::Download(audio_s));
                    }
                    if ui
                        .checkbox(&mut self.audio_single, "Play one verse only")
                        .changed()
                    {
                        settings_changed = true;
                    }
                    let own = audio::RECITERS[self.reciter]
                        .folder
                        .starts_with("https://");
                    ui.weak(if own {
                        "Tick \"Play one verse only\" to tap a verse and hear it. Audio: recorded for this app."
                    } else {
                        "Tick \"Play one verse only\" to tap a verse and hear it. Audio: everyayah.com"
                    });
                });
            }

            section(ui, &mut sidebar_section, 2, "Reader settings", |ui| {
                // The settings can be taller than the screen (e.g. with the tajweed
                // legend open), so they scroll inside their own area.
                let max_h = (ui.ctx().content_rect().height() * 0.3).max(160.0);
                egui::ScrollArea::vertical()
                    .id_salt("reader_settings_scroll")
                    .max_height(max_h)
                    .show(ui, |ui| {
                if ui
                    .add(
                        egui::Slider::new(&mut self.trans_size, MIN_TRANS..=MAX_TRANS)
                            .step_by(1.0)
                            .text("Translation size"),
                    )
                    .changed()
                {
                    settings_changed = true;
                }
                if pill_button(ui, "Show guide", false, 14.0, 10.0).clicked() {
                    show_guide = true;
                }
                if ui
                    .checkbox(&mut self.center_translation, "Center translation")
                    .changed()
                {
                    settings_changed = true;
                }
                
                marks_changed |= ui
                    .checkbox(&mut self.show_harakat, "Quranic marks (harakat)")
                    .changed();
                if ui
                    .checkbox(&mut self.numeric_verses, "Numeric verse numbers (1, 2, 3)")
                    .changed()
                {
                    settings_changed = true;
                }
                marks_changed |= ui
                    .checkbox(&mut self.show_tajweed, "Tajweed colours")
                    .changed();
                if self.show_tajweed {
                    let dark = ui.visuals().dark_mode;
                    for &(class, name) in arabic::TAJWEED_LEGEND {
                        ui.horizontal(|ui| {
                            let c = tajweed_color(class, dark).unwrap_or(Color32::GRAY);
                            let (r, _) = ui.allocate_exact_size(egui::vec2(12.0, 12.0), Sense::hover());
                            ui.painter().circle_filled(r.center(), 5.5, c);
                            ui.label(RichText::new(name).small());
                        });
                    }
                    ui.weak("Colours follow the rules of the printed tajweed mushaf. Turn on the harakat to see the tanween and madd signs coloured.");
					ui.weak("");
					ui.weak("Tip: pinch the page with two fingers to zoom.");
                }
                    });
            });

            section(ui, &mut sidebar_section, 4, "Bookmarks", |ui| {
                if self.bookmarks.is_empty() {
                    ui.weak("Long-press a verse to bookmark it.");
                }
                for b in &self.bookmarks {
                    if ui.button(b.label.as_str()).clicked() {
                        jump = Some(b.global);
                    }
                }
            });

			            if ui
                .add(
                    egui::TextEdit::singleline(&mut self.surah_filter)
                        .hint_text("Filter surahs (name or number)")
                        .desired_width(f32::INFINITY),
                )
                .has_focus()
            {
                self.ar_target = 2;
            }

            ui.separator();
            let list_out = egui::ScrollArea::vertical()
                .id_salt("surah_list")
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    for (i, info) in SURAHS.iter().enumerate() {
                        if !surah_matches(info, &self.surah_filter) {
                            continue;
                        }
                        let selected = i == self.selected_surah;
                        if surah_row(ui, info, &self.labels[i], &ar_font, selected).clicked() {
                            chosen = Some(i);
                        }
                    }
                    ui.add_space(BOTTOM_PAD);
                });
            // Scrolling the surah list closes whichever section is open.
            let list_y = list_out.state.offset.y;
            if (list_y - self.surah_list_offset).abs() > 0.5 {
                sidebar_section = None;
            }
            self.surah_list_offset = list_y;
        });

        self.sidebar_section = sidebar_section;
        if show_guide {
            self.guide_page = Some(0);
        }
        if reciter_changed {
            // The audio on the phone belongs to the old reciter: stop it and re-check.
            self.stop_audio();
            self.audio_ready_cache.2 = -10.0;
            self.dirty = true;
        }
        if marks_changed {
            self.layouts_dirty = true;
            if !self.search_applied.trim().is_empty() {
                self.recompute_search();
            }
            self.dirty = true;
        }
        if settings_changed {
            self.dirty = true;
        }
        if let Some(i) = pick_translation {
            self.set_translation(i);
        }
        if let Some(i) = start_download {
            self.start_translation_download(i);
        }
        if let Some(i) = start_tafsir {
            self.start_tafsir_download(i);
        }
        if let Some(act) = audio_action {
            self.run_audio_action(ctx, act);
        }
        if let Some(i) = chosen {
            self.search_query.clear();
            self.select_surah(i);
            if compact {
                self.sidebar_open = false;
            }
        }
        if let Some(g) = jump {
            self.jump_to_verse(g);
        }
    }

    fn central(&mut self, ctx: &egui::Context) {
        egui::CentralPanel::default()
            .frame(theme::page_frame(self.theme))
            .show(ctx, |ui| {
                if ui.available_width() < 160.0 {
                    return;
                }
                if !self.font_found {
                    ui.colored_label(
                        Color32::from_rgb(220, 140, 40),
                        "No Arabic font found: put quran_font.ttf in assets/.",
                    );
                }
                if self.search_applied.trim().is_empty() {
                    self.reader(ui);
                } else {
                    self.search_results(ui);
                }
            });
    }
}

impl QuranApp {
    fn position_pill(&mut self, ctx: &egui::Context) {
        if !self.search_applied.trim().is_empty() || self.quick_jump {
            return;
        }
        let Some((s, v)) = self.current_anchor() else {
            return;
        };
        let (a, b) = self.data.surah_range(s);
        if b <= a {
            return;
        }
        let total = (b - a) as usize;
        let vi = v.unwrap_or(0).min(total - 1);
        let verse = self.data.verses[a as usize + vi];
        let juz = juz_of(verse.surah, verse.ayah.max(1));
        let frac = (vi + 1) as f32 / total as f32;
        let th = self.theme;

        let mut clicked = false;
        let lift = self.bottom_inset;
        egui::Area::new(egui::Id::new("pos_pill"))
            .order(egui::Order::Foreground)
            .anchor(egui::Align2::CENTER_BOTTOM, egui::vec2(0.0, -10.0 - lift))
            .show(ctx, |ui| {
                let frame = egui::Frame::new()
                    .fill(theme::glass(th, if th.dark() { 40 } else { 120 }))
                    .stroke(Stroke::new(1.0, theme::border(th, 80)))
                    .corner_radius(CornerRadius::same(18))
                    .inner_margin(egui::Margin::symmetric(14, 0));
                let out = frame.show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(
                            RichText::new(self.labels[s].arabic.as_str())
                                .font(FontId::new(12.0, self.arabic_family.clone()))
                                .color(theme::arabic_text(th)),
                        );
                        ui.label(RichText::new(format!("{}:{}", verse.surah, verse.num)).weak());
                        ui.label(
                            RichText::new(format!("{} juzu", arabic::to_arabic_indic(juz as u16)))
                                .weak(),
                        );
                    });
                    ui.add(
                        egui::ProgressBar::new(frac)
                            .desired_width(120.0)
                            .desired_height(3.0)
                            .animate(false),
                    );
                });
                clicked = ui
                    .interact(
                        out.response.rect,
                        egui::Id::new("pos_pill_hit"),
                        Sense::click(),
                    )
                    .clicked();
            });
        if clicked {
            self.quick_jump = true;
        }
    }
}

impl QuranApp {
    fn reader(&mut self, ui: &mut egui::Ui) {
        let th = self.theme;
        let text_color = theme::arabic_text(th);
        let trans_color = theme::translation_text(th);
        let accent = theme::accent(th);
        let strong = ui.visuals().text_color();
        let weak = ui.visuals().weak_text_color();
        let sep_color = ui.visuals().widgets.noninteractive.bg_stroke.color;
        let page_color = theme::page_fill(th);
        let (now, pinching, zoom) =
            ui.input(|i| (i.time, i.multi_touch().is_some(), i.zoom_delta()));
        let avail = ui.available_width();
        let view_h = ui.available_height();

        if (zoom - 1.0).abs() > 1e-4 {
            self.size_edit = (self.size_edit * zoom).clamp(MIN_ARABIC, MAX_ARABIC);
            self.arabic_size = self.size_edit.round();
            self.dirty = true;
        }

        // On phones the open sidebar covers almost the whole page, so keep the
        // page layout at its closed width while the sidebar is open or sliding.
        // This avoids a heavy re-layout (the visible hitch) on every open/close.
        let settled = if self.compact && self.sidebar_open && self.layout_key.width != 0 {
            false
        } else {
            now - self.sidebar_toggle_time > 0.4
        };
        let width_key = if settled {
            avail.round() as i32
        } else {
            self.layout_key.width
        };
        let key = LayoutKey {
            width: width_key,
            font_bits: self.arabic_size.to_bits(),
            harakat: self.show_harakat,
            translation: self.show_translation,
            translation_idx: self.translation_idx,
            trans_size: self.trans_size.round() as u32,
            spacing_e3: (self.line_spacing * 1000.0).round() as u32,
            width_pct: (self.width_frac * 100.0).round() as u32,
        };
        if key != self.layout_key || self.layouts_dirty {
            if self.pending_jump.is_none() {
                self.pending_jump = self.current_anchor();
            }
            let anchor_s = match self.pending_jump {
                Some((s, _)) => s,
                None => section_at(&self.tops, self.last_offset),
            };
            let reshape = self.layouts_dirty;
            self.layout_key = key;
            self.layouts_dirty = false;
            self.m = compute_metrics(
                ui,
                width_key as f32 - 8.0,
                self.arabic_size,
                &self.arabic_family,
                self.show_harakat,
                self.show_tajweed,
                self.line_spacing,
                self.width_frac,
                self.center_translation,
                self.trans_size,
            );
            let ctx = ui.ctx().clone();
            for s in 0..SURAHS.len() {
                if reshape || s + 2 < anchor_s || s > anchor_s + 2 {
                    self.layouts[s] = None;
                } else if let Some(l) = self.layouts[s].as_mut() {
                    l.relayout(
                        &ctx,
                        self.data.surah_verses(s),
                        self.data.surah_translations(self.translation_idx, s),
                        &self.m,
                        self.show_translation,
                    );
                }
            }
        }

        // Only changes how the number is drawn, so no relayout is needed.
        self.m.numeric_verses = self.numeric_verses;

        let reader_rect = ui.available_rect_before_wrap();
        let drag = ui.interact(reader_rect, egui::Id::new("reader_drag"), Sense::drag());
        let mut scroll_delta = 0.0f32;

        if pinching {
            self.fling_vel = 0.0;
        } else if drag.dragged() {
            scroll_delta = -drag.drag_delta().y;
            let v = -ui.input(|i| i.pointer.velocity().y) * FLING_BOOST;
            self.fling_vel = v.clamp(-FLING_MAX, FLING_MAX);
        } else if drag.drag_stopped() {
            // Finger lifted: keep the last velocity and start rolling next frame.
            ui.ctx().request_repaint();
        } else if ui.input(|i| i.pointer.any_down()) {
            // A touch stops the roll (and that touch is not a verse tap).
            if self.fling_vel != 0.0 {
                self.stopped_fling = true;
            }
            self.fling_vel = 0.0;
        } else if self.fling_vel.abs() > FLING_MIN {
            let dt = ui.input(|i| i.stable_dt).min(0.05);
            scroll_delta = self.fling_vel * dt;
            self.fling_vel *= (-FLING_FRICTION * dt).exp();
            if self.last_offset <= 0.0 && self.fling_vel < 0.0 {
                self.fling_vel = 0.0;
            }
            ui.ctx().request_repaint();
        } else {
            self.fling_vel = 0.0;
        }

        // Auto-scroll: the page moves by itself at the chosen pace. A finger on the
        // screen pauses it; it carries on when the finger is lifted.
        self.auto_pushed = false;
        if self.auto_scroll
            && !pinching
            && !drag.dragged()
            && self.fling_vel == 0.0
            && !ui.input(|i| i.pointer.any_down())
        {
            let dt = ui.input(|i| i.stable_dt).min(0.05);
            self.auto_accum += AUTO_PX_PER_LEVEL * self.auto_speed * dt;
            // Whole physical pixels only, so the text stays sharp.
            let ppp = ui.ctx().pixels_per_point();
            let whole = (self.auto_accum * ppp).floor() / ppp;
            if whole > 0.0 {
                scroll_delta += whole;
                self.auto_accum -= whole;
                self.auto_pushed = true;
            }
            ui.ctx().request_repaint();
        }

        self.compute_tops();
        let mut forced: Option<f32> = None;
        let mut vp_top = self.last_offset;
        let jumped = self.pending_jump.is_some();
        if let Some((s, v)) = self.pending_jump.take() {
            self.fling_vel = 0.0;
            self.ensure_layout(ui.ctx(), s);
            self.compute_tops();
            vp_top = self.tops[s] + self.verse_offset(s, v);
            forced = Some(vp_top);
        } else if scroll_delta != 0.0 {
            vp_top = (self.last_offset + scroll_delta).max(0.0);
            forced = Some(vp_top);
        }
        let start_top = vp_top;

        let margin = view_h * 0.5;
        for _ in 0..4 {
            self.compute_tops();
            let fv = section_at(&self.tops, vp_top);
            let lo = section_at(&self.tops, (vp_top - margin).max(0.0));
            let hi = section_at(&self.tops, vp_top + view_h + margin);
            let mut shift = 0.0f32;
            let mut changed = false;
            for s in 0..SURAHS.len() {
                if s + KEEP_SECTIONS < fv || s > fv + KEEP_SECTIONS {
                    if let Some(l) = self.layouts[s].take() {
                        if s < fv {
                            shift += self.est_section_height(s) - l.height;
                        }
                        changed = true;
                    }
                } else if s >= lo && s <= hi && self.layouts[s].is_none() {
                    let old = self.est_section_height(s);
                    self.ensure_layout(ui.ctx(), s);
                    let new = self.section_height(s);
                    if s < fv {
                        shift += new - old;
                    }
                    changed = true;
                }
            }
            if shift != 0.0 {
                vp_top += shift;
                forced = Some(vp_top);
            }
            if !changed {
                break;
            }
        }
        self.compute_tops();
        let adjust = vp_top - start_top;

        let pointer = ui.input(|i| i.pointer.interact_pos());
        let held = self.press.target;
        let tapped_color = theme::tapped_text(self.theme);
        let playing_color = theme::playing_text(self.theme);
        let color_of = |g: usize| -> Color32 {
            if self.audio_playing == Some(g as u32) {
                playing_color
            } else if held == Some(g) || self.bookmarks.iter().any(|b| b.global as usize == g) {
                accent
            } else if self.last_read == Some(g as u32) {
                tapped_color
            } else {
                text_color
            }
        };

        let mut area = egui::ScrollArea::vertical()
            .id_salt("reader")
            .auto_shrink([false, false])
            // Dragging is handled by `reader_drag` above (with momentum), so the
            // scroll area must not grab touch drags itself.
            .drag_to_scroll(false);
        if let Some(f) = forced {
            // Whole physical pixels only, so text is never drawn between pixels.
            let ppp = ui.ctx().pixels_per_point();
            area = area.vertical_scroll_offset((f * ppp).round() / ppp);
        }

        let mut opened: Option<usize> = None;
        let mut tapped: Option<usize> = None;
        let out = area.show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            let w = ui.available_width();
            let top0 = ui.cursor().top();
            let clip = ui.clip_rect();
            let m = &self.m;

            for s in 0..SURAHS.len() {
                let h = self.tops[s + 1] - self.tops[s];
                let sec_top = top0 + self.tops[s];
                let near =
                    sec_top + h >= clip.top() - OVERSCAN && sec_top <= clip.bottom() + OVERSCAN;
                let Some(lay) = self.layouts[s].as_ref() else {
                    ui.allocate_exact_size(egui::vec2(w, h), Sense::hover());
                    continue;
                };
                if !near {
                    ui.allocate_exact_size(egui::vec2(w, h), Sense::hover());
                    continue;
                }

                let base = self.data.surah_range(s).0 as usize;
                let hdr_h = header_height(m, !lay.basmala.is_empty());
                let juz_note = if data::JUZ_STARTS.contains(&((s + 1) as u16, 1)) {
                    format!("  \u{b7}  Juz' {} begins here", juz_of(s as u16 + 1, 1))
                } else {
                    String::new()
                };
                draw_header(
                    ui,
                    m,
                    hdr_h,
                    &self.labels[s].arabic,
                    SURAHS[s].translit,
                    &self.labels[s].meta,
                    &juz_note,
                    &lay.basmala,
                    page_color,
                    text_color,
                    strong,
                    weak,
                    sep_color,
                    accent,
                );

                let n_verses = lay.shaped.len();
                if n_verses == 0 {
                    let (rect, _) = ui.allocate_exact_size(egui::vec2(w, MSG_H), Sense::hover());
                    ui.painter().text(
                        rect.center(),
                        egui::Align2::CENTER_CENTER,
                        "Text of this surah is not bundled.",
                        m.meta_font.clone(),
                        weak,
                    );
                } else if self.show_translation {
                    let verses = self.data.surah_verses(s);
                    let trans = self.data.surah_translations(self.translation_idx, s);
                    for i in 0..n_verses {
                        let vh = lay.verse_h[i];
                        let top = ui.cursor().top();
                        if !(top + vh >= clip.top() - OVERSCAN && top <= clip.bottom() + OVERSCAN) {
                            ui.allocate_exact_size(egui::vec2(w, vh), Sense::hover());
                            continue;
                        }
                        let global = base + i;
                        let marked = self.bookmarks.iter().any(|b| b.global as usize == global);
                        let v_obj = verses[i];

                        if let Some(ruku_num) = get_ruku_number(v_obj.surah, v_obj.ayah) {
                            let ruku_text = format!("— Ruku {} —", ruku_num);
                            let (rr, _) =
                                ui.allocate_exact_size(egui::vec2(w, 28.0), Sense::hover());
                            ui.painter().text(
                                rr.center(),
                                egui::Align2::CENTER_CENTER,
                                ruku_text,
                                m.meta_font.clone(),
                                accent,
                            );
                        }

                        let (r0, _) = ui.allocate_exact_size(
                            egui::vec2(w, m.body_row_h + REF_GAP),
                            Sense::hover(),
                        );
                        ui.painter().text(
                            egui::pos2(r0.center().x, r0.top() + m.body_row_h * 0.5),
                            egui::Align2::CENTER_CENTER,
                            lay.ref_labels[i].as_str(),
                            m.body_font.clone(),
                            if marked { accent } else { weak },
                        );

                        let (l0, l1) = lay.verse_line_range[i];
                        for li in l0..l1 {
                            draw_line(
                                ui,
                                &lay.verse_lines[li as usize],
                                lay,
                                m,
                                base,
                                &color_of,
                                accent,
                                None,
                                egui::Id::new(("tline", global, li)),
                                false,
                            );
                        }

                        if lay.trans_h[i] > 0.0 {
                            let (tr, _) = ui.allocate_exact_size(
                                egui::vec2(w, REF_GAP + lay.trans_h[i]),
                                Sense::hover(),
                            );
                            let tr_text = trans.get(i).copied().unwrap_or("");
                            let galley = ui.fonts_mut(|f| f.layout_job(translation_job(tr_text, m)));
                            // Snap the text origin to whole physical pixels: glyphs drawn
                            // between pixels lose their thin strokes and look broken.
                            let ppp = ui.ctx().pixels_per_point();
                            let snap = |v: f32| (v * ppp).round() / ppp;
                            ui.painter().galley(
                                egui::pos2(
                                    snap(if m.center_translation {
                                        tr.center().x
                                    } else {
                                        tr.left()
                                    }),
                                    snap(tr.top() + REF_GAP),
                                ),
                                galley,
                                trans_color,
                            );
                        }

                        let (sr, _) =
                            ui.allocate_exact_size(egui::vec2(w, SEP_BLOCK), Sense::hover());
                        ui.painter().line_segment(
                            [
                                egui::pos2(sr.left(), sr.top() + 6.0),
                                egui::pos2(sr.right(), sr.top() + 6.0),
                            ],
                            Stroke::new(1.0, sep_color),
                        );

                        let block =
                            egui::Rect::from_min_max(r0.min, egui::pos2(r0.max.x, sr.max.y));
                        let resp =
                            ui.interact(block, egui::Id::new(("verse", global)), Sense::click());
                        if !pinching
                            && resp.is_pointer_button_down_on()
                            && self.press.update(ui, global)
                        {
                            opened = Some(global);
                        }
                        if !pinching && resp.is_pointer_button_down_on() {
                            self.press_verse = Some(global);
                        }
                    }
                } else {
                    let n = lay.flow_lines.len();
                    let line_h = m.line_h.max(1.0);
                    let top = ui.cursor().top();
                    let first =
                        (((clip.top() - OVERSCAN - top) / line_h).floor().max(0.0) as usize).min(n);
                    let last = (((clip.bottom() + OVERSCAN - top) / line_h).ceil().max(0.0)
                        as usize)
                        .min(n)
                        .max(first);
                    if first > 0 {
                        ui.allocate_exact_size(
                            egui::vec2(w, first as f32 * m.line_h),
                            Sense::hover(),
                        );
                    }
                    let verses = self.data.surah_verses(s);
                    let mut last_verse_checked: Option<usize> = None;

                    for li in first..last {
                        let line = &lay.flow_lines[li];
                        let v_idx = lay.items[line.first as usize].verse as usize;

                        if last_verse_checked != Some(v_idx) {
                            last_verse_checked = Some(v_idx);
                            if let Some(v_obj) = verses.get(v_idx) {
                                if let Some(ruku_num) = get_ruku_number(v_obj.surah, v_obj.ayah) {
                                    let ruku_text = format!("— Ruku {} —", ruku_num);
                                    let (rr, _) =
                                        ui.allocate_exact_size(egui::vec2(w, 28.0), Sense::hover());
                                    ui.painter().text(
                                        rr.center(),
                                        egui::Align2::CENTER_CENTER,
                                        ruku_text,
                                        m.meta_font.clone(),
                                        accent,
                                    );
                                }
                            }
                        }

                        let (resp, hit) = draw_line(
                            ui,
                            line,
                            lay,
                            m,
                            base,
                            &color_of,
                            accent,
                            pointer,
                            egui::Id::new(("fline", s, li)),
                            true,
                        );
                        if !pinching && resp.is_pointer_button_down_on() {
                            if let Some(g) = hit {
                                self.press_verse = Some(g);
                                if self.press.update(ui, g) {
                                    opened = Some(g);
                                }
                            }
                        }

                    }
                    if last < n {
                        ui.allocate_exact_size(
                            egui::vec2(w, (n - last) as f32 * m.line_h),
                            Sense::hover(),
                        );
                    }
                }

                ui.allocate_exact_size(egui::vec2(w, SECTION_GAP), Sense::hover());
            }
            ui.add_space(BOTTOM_PAD);
        });

        let offset = out.state.offset.y;
        if self.fling_vel != 0.0 && forced.is_some() && offset + 1.0 < vp_top {
            self.fling_vel = 0.0;
        }
        self.track_scroll(ui.ctx(), offset, jumped, adjust);
        self.selected_surah = section_at(&self.tops, offset);

        // A tap is: finger down on a verse, no movement, then released. It is not a
        // tap if the page was rolling, the finger moved (scroll) or two fingers pinched.
        let (down, released, ppos) = ui.input(|i| {
            (
                i.pointer.any_down(),
                i.pointer.any_released(),
                i.pointer.interact_pos(),
            )
        });
        if down {
            if self.press_origin.is_none() {
                self.press_origin = ppos;
            }
            if let (Some(o), Some(p)) = (self.press_origin, ppos) {
                if o.distance(p) > MOVE_SLOP {
                    self.press_moved = true;
                }
            }
            if pinching {
                self.press_moved = true;
            }
        }
        if released {
            if !self.press_moved && !self.stopped_fling {
                tapped = self.press_verse;
            }
            self.press_verse = None;
            self.press_origin = None;
            self.press_moved = false;
            self.stopped_fling = false;
        }

        // The tapped (or long-pressed) verse is the new "last stop": it is the only
        // verse shown in the tapped colour and the one the app reopens at.
        if let Some(g) = tapped.or(opened) {
            let g32 = g as u32;
            if self.last_read != Some(g32) {
                self.last_read = Some(g32);
                self.dirty = true;
                self.save_file_only();
            }
        }
        // Tapping a verse plays its audio (when it has been downloaded), but only
        // while "Play one verse only" is ticked in the sidebar Audio menu.
        if let (Some(g), true) = (tapped, self.audio_single) {
            let ctx2 = ui.ctx().clone();
            self.play_audio(&ctx2, g as u32, false);
        }

        if let Some(g) = opened {
            let v = self.data.verses[g];
            let s = v.surah as usize - 1;
            let i = g - self.data.surah_range(s).0 as usize;
            if let Some(lay) = self.layouts[s].as_ref() {
                self.long_press_state = Some(LongPressContext {
                    global: g as u32,
                    title: format!("{} {}:{}", SURAHS[s].translit, v.surah, v.num),
                    shaped_ar: lay.shaped[i].clone(),
                    show_tafsir: false,
                });
            }
        }
    }

    fn search_results(&mut self, ui: &mut egui::Ui) {
        let th = self.theme;
        let text_color = theme::arabic_text(th);
        let trans_color = theme::translation_text(th);
        let ar_font = FontId::new(self.arabic_size, self.arabic_family.clone());
        let mut pick_surah: Option<usize> = None;
        let mut pick_verse: Option<u32> = None;
        let tidx = self.translation_idx;
        let query = self.search_applied.trim().to_owned();
        let hl_color = theme::accent(th).gamma_multiply(0.4);

        ui.label(RichText::new(self.search_status.as_str()).weak());
        if !self.surah_hits.is_empty() {
            ui.horizontal_wrapped(|ui| {
                for &s in &self.surah_hits {
                    if ui.button(SURAHS[s as usize].translit).clicked() {
                        pick_surah = Some(s as usize);
                    }
                }
            });
        }

        egui::ScrollArea::vertical()
            .id_salt("search_scroll")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                virtual_list(ui, &mut self.hit_heights, |ui, i| {
                    let hit = &self.hits[i];
                    let tr = self.data.translation(tidx, hit.verse as usize);
                    let card = theme::glass_card(ui, false, |ui| {
                        ui.label(RichText::new(hit.label.as_str()).weak());
                        let words = arabic_match_words(
                            self.data.verses[hit.verse as usize].arabic,
                            &self.needle,
                        );
                        arabic_block_hl(ui, &hit.shaped_ar, &ar_font, text_color, &words, hl_color);
                        if !tr.is_empty() {
                            ui.label(highlight_job(ui, tr, &query, trans_color, hl_color));
                        }
                    });
                    let resp = ui.interact(
                        card.response.rect,
                        egui::Id::new(("hit", i)),
                        Sense::click(),
                    );
                    if resp.clicked() {
                        pick_verse = Some(hit.verse);
                    }
                    card.response.rect.height()
                });
                ui.add_space(BOTTOM_PAD);
            });

        if let Some(s) = pick_surah {
            self.search_query.clear();
            self.select_surah(s);
        }
        if let Some(g) = pick_verse {
            self.jump_to_verse(g);
        }
    }
}

/// Sidebar accordion section: only one section is open at a time.
fn section(
    ui: &mut egui::Ui,
    current: &mut Option<u8>,
    id: u8,
    title: &str,
    body: impl FnOnce(&mut egui::Ui),
) {
    let open = *current == Some(id);
    let resp = egui::CollapsingHeader::new(title)
        .open(Some(open))
        .show(ui, body);
    if resp.header_response.clicked() {
        *current = if open { None } else { Some(id) };
    }
}

/// One key of the on-screen keyboard, drawn by hand so it never grows past `size`.
fn kbd_key(ui: &mut egui::Ui, size: egui::Vec2, label: &str, font: FontId) -> bool {
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
    let vis = ui.style().interact(&resp);
    ui.painter().rect(
        rect,
        vis.corner_radius,
        vis.bg_fill,
        vis.bg_stroke,
        egui::StrokeKind::Inside,
    );
    ui.painter().text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        label,
        font,
        vis.fg_stroke.color,
    );
    resp.clicked()
}

/// A small keyboard icon centred on `c`.
fn paint_keyboard_icon(p: &egui::Painter, c: Pos2, color: Color32) {
    let body = egui::Rect::from_center_size(c, egui::vec2(20.0, 13.0));
    p.rect_stroke(
        body,
        CornerRadius::same(2),
        Stroke::new(1.5, color),
        egui::StrokeKind::Inside,
    );
    for row in 0..2 {
        for col in 0..4 {
            let pos = Pos2::new(
                body.left() + 4.0 + col as f32 * 4.0,
                body.top() + 3.5 + row as f32 * 3.0,
            );
            p.circle_filled(pos, 0.9, color);
        }
    }
    p.line_segment(
        [
            Pos2::new(body.left() + 5.0, body.bottom() - 3.0),
            Pos2::new(body.right() - 5.0, body.bottom() - 3.0),
        ],
        Stroke::new(1.3, color),
    );
}

impl QuranApp {
    /// On-screen Arabic keyboard. winit 0.30 on Android cannot receive text from
    /// non-Latin soft keyboards, so Arabic is typed with these keys instead, or
    /// typed anywhere with any installed keyboard, copied, and pasted with "Paste".
    fn arabic_keyboard(&mut self, ctx: &egui::Context) {
        if !self.ar_kbd_open {
            return;
        }
        const ROWS: [&str; 3] = [
            "ضصثقفغعهخحجد",
            "شسيبلاتنمكط",
            "ئءؤرىةوزظذ",
        ];
        let key_font = FontId::new(21.0, self.arabic_family.clone());
        let bar = theme::bar_frame(self.theme);
        let mut typed: Option<char> = None;
        let mut backspace = false;
        let mut space = false;
        let mut close = false;
        let mut paste = false;

        egui::TopBottomPanel::bottom("arabic_keyboard")
            .frame(
                egui::Frame::new()
                    .fill(bar.fill)
                    .stroke(bar.stroke)
                    .inner_margin(egui::Margin::symmetric(6, 6)),
            )
            .show(ctx, |ui| {
                let gap = 4.0;
                ui.spacing_mut().item_spacing = egui::vec2(gap, gap);
                let avail = ui.available_width();
                // One key width for every row, so the widest row (12 keys) always fits.
                let w = ((avail - gap * 12.0) / 12.0).floor().clamp(18.0, 34.0);
                for row in ROWS {
                    let n = row.chars().count() as f32;
                    let pad = ((avail - (n * w + n * gap)) / 2.0).max(0.0);
                    ui.horizontal(|ui| {
                        // Arabic reads right to left: the first key sits at the right edge.
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            ui.add_space(pad);
                            for ch in row.chars() {
                                if kbd_key(ui, egui::vec2(w, 42.0), &ch.to_string(), key_font.clone())
                                {
                                    typed = Some(ch);
                                }
                            }
                        });
                    });
                }
                // Letters the phone's keyboard cannot send to this app (needed for Luganda).
                let latin = FontId::proportional(21.0);
                let extra_w = w * 1.6;
                let extra_pad = ((avail - (2.0 * extra_w + gap)) / 2.0).max(0.0);
                ui.horizontal(|ui| {
                    ui.add_space(extra_pad);
                    for ch in ['\u{14B}', '\u{14A}'] {
                        if kbd_key(ui, egui::vec2(extra_w, 42.0), &ch.to_string(), latin.clone()) {
                            typed = Some(ch);
                        }
                    }
                });
                let side = (avail * 0.19).floor();
                let space_w = (avail - 3.0 * side - 4.0 * gap).max(60.0);
                let small = FontId::proportional(15.0);
                ui.horizontal(|ui| {
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if kbd_key(ui, egui::vec2(side, 42.0), "Del", small.clone()) {
                            backspace = true;
                        }
                        if kbd_key(ui, egui::vec2(side, 42.0), "Paste", small.clone()) {
                            paste = true;
                        }
                        if kbd_key(ui, egui::vec2(space_w, 42.0), "Space", small.clone()) {
                            space = true;
                        }
                        if kbd_key(ui, egui::vec2(side, 42.0), "Close", small) {
                            close = true;
                        }
                    });
                });
            });

        // Text copied from any app (typed there with any installed keyboard).
        let pasted = if paste { Some(audio::clipboard_text()) } else { None };
        let target = match self.ar_target {
            2 => &mut self.surah_filter,
            3 => &mut self.jump_query,
            _ => &mut self.search_query,
        };
        if let Some(Ok(Some(t))) = &pasted {
            let one_line = t.replace('\n', " ").replace('\r', " ");
            target.push_str(one_line.trim());
        }
        if let Some(c) = typed {
            target.push(c);
        }
        if space {
            target.push(' ');
        }
        if backspace {
            target.pop();
        }
        if close {
            self.ar_kbd_open = false;
        }
        if typed.is_some() || space || backspace || close || paste {
            ctx.request_repaint();
        }
        match pasted {
            Some(Err(e)) => self.toast(ctx, e),
            Some(Ok(None)) => self.toast(ctx, "Nothing to paste"),
            _ => {}
        }
    }
}

impl QuranApp {
    fn quick_jump_window(&mut self, ctx: &egui::Context) {
        if !self.quick_jump {
            return;
        }
        let th = self.theme;
        let screen = ctx.screen_rect();
        let mut action: Option<(usize, Option<usize>)> = None;
        let mut open = true;

        egui::Window::new("quick_jump")
            .id(egui::Id::new("quick_jump_window"))
            .title_bar(false)
            .order(egui::Order::Foreground)
            .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, -30.0))
            .resizable(false)
            .collapsible(false)
            .default_width((screen.width() * 0.92).min(480.0))
            .frame(theme::window_frame(th))
            .show(ctx, |ui| {
                ui.label(RichText::new("Jump to").strong());
                if ui
                    .add(
                        egui::TextEdit::singleline(&mut self.jump_query)
                            .hint_text("2:255 \u{b7} Yusuf \u{b7} juz 5")
                            .desired_width(f32::INFINITY),
                    )
                    .has_focus()
                {
                    self.ar_target = 3;
                }

                let q = self.jump_query.trim();
                if !q.is_empty() {
                    if let Some((s, a)) = parse_reference(q) {
                        if let Some(g) = self.data.find_num(s, a) {
                            let v = self.data.verses[g as usize];
                            if ui
                                .button(format!(
                                    "\u{2192} {} {}:{}",
                                    SURAHS[s as usize - 1].translit,
                                    v.surah,
                                    v.num
                                ))
                                .clicked()
                            {
                                let (start, _) = self.data.surah_range(s as usize - 1);
                                action = Some((s as usize - 1, Some((g - start) as usize)));
                            }
                        }
                    }
                    if let Some(j) = parse_juz(q) {
                        let (s, a) = juz_start(j);
                        if let Some(g) = self.data.find(s, a) {
                            let shown = self.data.verses[g as usize].num;
                            if ui
                                .button(format!("\u{2192} Juz' {j} ({}:{})", s, shown))
                                .clicked()
                            {
                                let (start, _) = self.data.surah_range(s as usize - 1);
                                action = Some((s as usize - 1, Some((g - start) as usize)));
                            }
                        }
                    }
                    ui.horizontal_wrapped(|ui| {
                        let mut n = 0;
                        for (i, info) in SURAHS.iter().enumerate() {
                            if n >= 8 {
                                break;
                            }
                            if surah_matches(info, q) {
                                n += 1;
                                if ui.button(info.translit).clicked() {
                                    action = Some((i, None));
                                }
                            }
                        }
                    });
                }

                ui.separator();
                ui.label(RichText::new("Juz' (para)").weak());
                ui.horizontal_wrapped(|ui| {
                    for j in 1..=30u8 {
                        let b = ui
                            .add(egui::Button::new(arabic::to_arabic_indic(j as u16)).small())
                            .on_hover_text(format!("Juz' {j}"));
                        if b.clicked() {
                            let (s, a) = juz_start(j);
                            if let Some(g) = self.data.find(s, a) {
                                let (start, _) = self.data.surah_range(s as usize - 1);
                                action = Some((s as usize - 1, Some((g - start) as usize)));
                            }
                        }
                    }
                });
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    if ui.button("Close").clicked() {
                        open = false;
                    }
                    // Auto-scroll sits on the same line, at the right: [x] auto scroll ---o--- 6
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.spacing_mut().slider_width = 90.0;
                        if ui
                            .add(egui::Slider::new(&mut self.auto_speed, 1.0..=20.0).step_by(1.0))
                            .changed()
                        {
                            self.dirty = true;
                        }
                        if ui.checkbox(&mut self.auto_scroll, "auto scroll").changed() {
                            self.auto_accum = 0.0;
                            self.auto_stall = 0;
                            if self.auto_scroll {
                                open = false; // back to the reading
                            }
                        }
                    });
                });
            });

        if let Some((s, v)) = action {
            self.pending_jump = Some((s, v));
            self.selected_surah = s;
            self.quick_jump = false;
            self.jump_query.clear();
            self.dirty = true;
        } else if !open {
            self.quick_jump = false;
            self.jump_query.clear();
        }
    }
}

impl QuranApp {
    fn verse_menu(&mut self, ctx: &egui::Context) {
        let Some(mut st) = self.long_press_state.take() else {
            return;
        };
        let v = self.data.verses[st.global as usize];
        let s = v.surah as usize - 1;
        let tr = self.data.translation(self.translation_idx, st.global as usize);
        let tr_idx = self.translation_idx;
        let tr_name = self.current_translation().name;
        // The tafsir that goes with the selected translation.
        let tafsir_text = self.data.tafsir(tr_idx, st.global as usize);
        let tafsir_have = self.tafsir_ready[tr_idx];
        let tafsir_busy = self.tafsir_dl.lock().ok().and_then(|g| g.active);
        let mut start_tafsir: Option<usize> = None;
        let mut pick_tr: Option<usize> = None;
        let is_marked = self.bookmarks.iter().any(|b| b.global == st.global);
        let th = self.theme;
        let text_color = theme::arabic_text(th);
        let trans_color = theme::translation_text(th);
        let ar_font = FontId::new(self.arabic_size, self.arabic_family.clone());
        let screen = ctx.screen_rect();

        let mut close = false;
        let mut do_share = false;
        let mut do_bookmark = false;
        let mut do_surah_top = false;

        egui::Area::new(egui::Id::new("verse_scrim"))
            .order(egui::Order::Middle)
            .fixed_pos(Pos2::ZERO)
            .show(ctx, |ui| {
                ui.painter().rect_filled(
                    screen,
                    CornerRadius::ZERO,
                    Color32::from_black_alpha(130),
                );
                if ui.allocate_rect(screen, Sense::click()).clicked() {
                    close = true;
                }
            });

        egui::Window::new("verse_menu")
            .id(egui::Id::new("verse_menu_window"))
            .title_bar(false)
            .order(egui::Order::Foreground)
            .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
            .resizable(true)
            .collapsible(false)
            .default_width((screen.width() * 0.84).min(440.0))
            .frame(theme::window_frame(th))
            .show(ctx, |ui| {
                ui.label(RichText::new(st.title.as_str()).strong());
                arabic_block(ui, &st.shaped_ar, &ar_font, text_color);
                ui.separator();

                egui::ScrollArea::vertical()
                    .id_salt("verse_menu_scroll")
                    .max_height(screen.height() * 0.35)
                    .show(ui, |ui| {
                        if tr.is_empty() {
                            ui.weak("No translation loaded for this verse.");
                        } else {
                            ui.label(RichText::new(tr_name).small().weak());
                            ui.label(RichText::new(tr).color(trans_color));
                        }
                        if st.show_tafsir {
                            ui.separator();
                            if tafsir_have {
                                if tafsir_text.is_empty() {
                                    ui.weak("No tafsir for this verse.");
                                } else {
                                    ui.label(tafsir_text);
                                }
                            } else if tafsir_busy == Some(tr_idx) {
                                ui.horizontal(|ui| {
                                    ui.add(egui::Spinner::new());
                                    ui.label(format!("Downloading the {tr_name} tafsir..."));
                                });
                            } else {
                                ui.weak(format!("The {tr_name} tafsir is not on the phone yet."));
                                if ui
                                    .add_enabled(tafsir_busy.is_none(), egui::Button::new("Download tafsir"))
                                    .clicked()
                                {
                                    start_tafsir = Some(tr_idx);
                                }
                                if let Some(m) = &self.tafsir_msg {
                                    ui.colored_label(Color32::from_rgb(220, 120, 40), m.as_str());
                                }
                            }
                        }
                    });

                if self.trans_ready.iter().filter(|r| **r).count() > 1 {
                    ui.separator();
                    ui.horizontal(|ui| {
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            ui.spacing_mut().item_spacing.x = 4.0;
                            let names: Vec<&str> = TRANSLATIONS
                                .iter()
                                .enumerate()
                                .filter(|(i, _)| self.trans_ready[*i])
                                .map(|(_, t)| t.name)
                                .collect();
                            let (fs, pad) = fit_button_row(ui, &names, 4.0);
                            for (i, t) in TRANSLATIONS.iter().enumerate().rev() {
                                if !self.trans_ready[i] {
                                    continue;
                                }
                                if pill_button(ui, t.name, i == tr_idx, fs, pad).clicked() {
                                    pick_tr = Some(i);
                                }
                            }
                        });
                    });
                }

                ui.separator();
                // One single row of compact buttons, aligned to the right.
                ui.horizontal(|ui| {
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.spacing_mut().item_spacing.x = 4.0;
                        // Opens Android's share sheet (rename the label here if you like).
                        let share_label = "Share";
                        let mark_label = if is_marked { "Remove" } else { "Giteleke" };
                        let close_label = "\u{3a7}";
                        let labels = [share_label, "Tafsir", mark_label, "Top", close_label];
                        let (fs, pad) = fit_button_row(ui, &labels, 4.0);
                        // Added right-to-left, so this reads Share, Tafsir, Giteleke, Top, X.
                        if pill_button(ui, close_label, false, fs, pad).clicked() {
                            close = true;
                        }
                        if pill_button(ui, "Top", false, fs, pad).clicked() {
                            do_surah_top = true;
                        }
                        if pill_button(ui, mark_label, false, fs, pad).clicked() {
                            do_bookmark = true;
                        }
                        if pill_button(ui, "Tafsir", st.show_tafsir, fs, pad).clicked() {
                            st.show_tafsir = !st.show_tafsir;
                        }
                        if pill_button(ui, share_label, false, fs, pad).clicked() {
                            do_share = true;
                        }
                    });
                });
            });

        if do_share {
            // Shares everything shown: Arabic, translation, tafsir (if open) and reference.
            let mut text = String::from(v.arabic);
            if !tr.is_empty() {
                text.push('\n');
                text.push_str(tr);
            }
            if st.show_tafsir && tafsir_have && !tafsir_text.is_empty() {
                text.push_str("\n\n");
                text.push_str(tafsir_text);
            }
            if tr.is_empty() {
                text.push_str(&format!("\n\n[{}]", st.title));
            } else {
                text.push_str(&format!("\n\n[{} | {}]", st.title, tr_name));
            }
            if let Err(e) = audio::share_text(&text) {
                self.toast(ctx, e);
            }
        }
        if let Some(i) = pick_tr {
            self.set_translation(i);
            ctx.request_repaint();
        }
        if let Some(i) = start_tafsir {
            self.start_tafsir_download(i);
        }
        if do_bookmark {
            self.toggle_bookmark(st.global);
        }
        if do_surah_top {
            self.select_surah(s);
            close = true;
        }
        if !close {
            self.long_press_state = Some(st);
        }
    }
}

fn break_lines(
    items: &[FlowItem],
    offset: usize,
    gap: f32,
    max_w: f32,
    mut emit: impl FnMut(usize, usize, f32),
) {
    let mut start = 0usize;
    let mut w = 0.0f32;
    for (i, it) in items.iter().enumerate() {
        if i == start {
            w = it.width;
        } else if w + gap + it.width > max_w {
            let is_marker = it.start == it.end;
            let split = if is_marker && i - start >= 2 {
                i - 1
        } else {
                i
            };
            let first_w = if split == i {
                w
            } else {
                w - gap - items[split].width
            };
            emit(offset + start, offset + split, first_w);
            start = split;
            w = items[start..=i].iter().map(|x| x.width).sum::<f32>() + gap * (i - start) as f32;
        } else {
            w += gap + it.width;
        }
    }
    if start < items.len() {
        emit(offset + start, offset + items.len(), w);
    }
}

#[allow(clippy::too_many_arguments)]
fn draw_header(
    ui: &mut egui::Ui,
    m: &Metrics,
    h: f32,
    name_ar: &str,
    translit: &str,
    meta: &str,
    meta_suffix: &str,
    basmala: &str,
    page_color: Color32,
    text_color: Color32,
    strong: Color32,
    weak: Color32,
    sep: Color32,
    accent: Color32,
) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), h), Sense::hover());
    if !ui.is_rect_visible(rect) {
        return;
    }
    let painter = ui.painter();
    let cx = rect.center().x;
    let centered = egui::Align2::CENTER_CENTER;
    let mut y = rect.top() + 10.0;
    painter.text(
        egui::pos2(cx, y + m.title_h * 0.5),
        centered,
        name_ar,
        m.title_font.clone(),
        text_color,
    );
    y += m.title_h + 2.0;
    painter.text(
        egui::pos2(cx, y + m.head_h * 0.5),
        centered,
        translit,
        m.head_font.clone(),
        strong,
    );
    y += m.head_h + 2.0;
    let meta_text: String = if meta_suffix.is_empty() {
        meta.to_owned()
    } else {
        format!("{meta}{meta_suffix}")
    };
    painter.text(
        egui::pos2(cx, y + m.meta_h * 0.5),
        centered,
        meta_text.as_str(),
        m.meta_font.clone(),
        weak,
    );
    y += m.meta_h + 6.0;
    if !basmala.is_empty() {
        y += 4.0;
        painter.text(
            egui::pos2(cx, y + m.bas_h * 0.5),
            centered,
            basmala,
            m.bas_font.clone(),
            text_color,
        );
        y += m.bas_h + 6.0;
    }
    let line_y = y + 4.0;
    painter.line_segment(
        [
            egui::pos2(rect.left(), line_y),
            egui::pos2(rect.right(), line_y),
        ],
        Stroke::new(1.0, sep),
    );
    let r = m.meta_h * 1.1;
    painter.circle_filled(egui::pos2(cx, line_y), r, page_color);
    painter.text(
        egui::pos2(cx, line_y),
        centered,
        "\u{06DE}",
        m.head_font.clone(),
        accent,
    );
}

#[allow(clippy::too_many_arguments)]
fn draw_line(
    ui: &mut egui::Ui,
    line: &Line,
    lay: &SurahLayout,
    m: &Metrics,
    base: usize,
    color_of: &dyn Fn(usize) -> Color32,
    accent: Color32,
    pointer: Option<Pos2>,
    id: egui::Id,
    interactive: bool,
) -> (egui::Response, Option<usize>) {
    let (rect, _) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), m.line_h), Sense::hover());
    let resp = ui.interact(
        rect,
        id,
        if interactive {
            Sense::click()
        } else {
            Sense::hover()
        },
    );

    let painter = ui.painter();
    let cy = rect.center().y;
    let mut x = rect.center().x + line.width * 0.5;
    let mut hit: Option<usize> = None;
    for it in &lay.items[line.first as usize..line.end as usize] {
        let left = x - it.width;
        let v = it.verse as usize;
        if let Some(p) = pointer {
            if p.x >= left - m.gap * 0.5 && p.x <= x + m.gap * 0.5 {
                hit = Some(base + v);
            }
        }
        if it.start == it.end {
            let center = egui::pos2(left + it.width * 0.5, cy);
            painter.circle_stroke(center, it.width * 0.5 - 1.0, Stroke::new(1.5, accent));
            let (label, family, scale) = if m.numeric_verses {
                let n = lay.ref_labels[v].rsplit(':').next().unwrap_or("");
                (n, FontFamily::Proportional, if n.len() >= 3 { 0.38 } else { 0.46 })
            } else {
                (lay.markers[v].as_str(), m.font.family.clone(), 0.5)
            };
            painter.text(
                center,
                egui::Align2::CENTER_CENTER,
                label,
                FontId::new(m.size * scale, family),
                accent,
            );
        } else {
            let word = &lay.shaped[v][it.start as usize..it.end as usize];
            draw_marked_word(
                painter,
                egui::pos2(left, cy),
                word,
                &m.font,
                color_of(base + v),
                ui.visuals().dark_mode,
            );
        }
        x = left - m.gap;
    }
    (resp, hit)
}

// ---- Arabic marks (harakat) --------------------------------------------------
//
// egui cannot attach a mark to its letter: it just draws the mark after the
// letter, so marks drift sideways and collide with tall letters. Instead every
// word is drawn WITHOUT its marks, then each mark is placed by hand: centred
// over the ink of its own letter and stacked above (or below) it.

/// How close a mark sits to its letter, as a fraction of the font size.
const MARK_GAP: f32 = 0.09;

thread_local! {
    static MARK_CACHE: std::cell::RefCell<HashMap<(String, u32, FontFamily), Vec<(char, egui::Vec2, usize)>>> =
        std::cell::RefCell::new(HashMap::new());
}

/// Marks that sit under the letter (kasra, kasratan, subscript alef ...).
fn is_below_mark(c: char) -> bool {
    matches!(
        c,
        '\u{0650}'
            | '\u{064D}'
            | '\u{0655}'
            | '\u{0656}'
            | '\u{061A}'
            | '\u{065C}'
            | '\u{065F}'
            | '\u{06E3}'
            | '\u{06EA}'
            | '\u{06ED}'
    )
}

/// Where along a letter (0 = left edge, 1 = right edge) its marks are centred.
fn mark_anchor(letter: char) -> f32 {
    // Lam-alef ligature: the lam's stem crosses the alef a bit right of centre.
    if arabic::is_lam_alef_ligature(letter) {
        0.58
    // Lam: its tall stem is at the right end of the letter, not in the middle.
    } else if ('\u{FEDD}'..='\u{FEE0}').contains(&letter) {
        0.72
    } else {
        0.5
    }
}

/// Splits a shaped word into its letters, the marks that follow each letter and
/// the tajweed `(letter_class, mark_class)` of each letter.
fn split_marks(word: &str) -> (String, Vec<Vec<char>>, Vec<(u8, u8)>, Vec<usize>) {
    let mut bases = String::new();
    let mut marks: Vec<Vec<char>> = Vec::new();
    let mut classes: Vec<(u8, u8)> = Vec::new();
    // Per letter: how many of its marks belong to the lam of a lam-alef ligature.
    let mut splits: Vec<usize> = Vec::new();
    for c in word.chars() {
        if c == arabic::ALEF_MARKS_SEP {
            if let (Some(m), Some(sp)) = (marks.last(), splits.last_mut()) {
                *sp = m.len();
            }
        } else if let Some((class, marks_only)) = arabic::decode_tag(c) {
            if let Some(k) = classes.last_mut() {
                if marks_only {
                    k.1 = class;
                } else {
                    k.0 = class;
                }
            }
        } else if arabic::is_mark(c) {
            if let Some(last) = marks.last_mut() {
                last.push(c);
            }
        } else {
            bases.push(c);
            marks.push(Vec::new());
            classes.push((0, 0));
            splits.push(usize::MAX);
        }
    }
    (bases, marks, classes, splits)
}

/// Text colour of a tajweed class (`None` = normal text colour).
fn tajweed_color(class: u8, dark: bool) -> Option<Color32> {
    use arabic::tj;
    // (light theme, dark theme)
    let (l, d) = match class {
        tj::SILENT => ((0x8C, 0x8C, 0x92), (0x9A, 0x9A, 0xA4)),
        tj::GHUNNA => ((0x1E, 0x92, 0x52), (0x4C, 0xD9, 0x8B)),
        tj::IKHFA => ((0xC2, 0x1F, 0x6B), (0xFF, 0x7A, 0xB6)),
        tj::IQLAB => ((0x00, 0x8A, 0xA6), (0x4D, 0xD0, 0xE1)),
        tj::IDGHAM => ((0x5E, 0x6B, 0x8C), (0xA9, 0xB6, 0xD6)),
        tj::QALQALAH => ((0x1F, 0x5F, 0xD0), (0x6C, 0xA8, 0xFF)),
        tj::MADD_NATURAL => ((0xD9, 0x7A, 0x00), (0xFF, 0xB1, 0x40)),
        tj::MADD_PERMISSIBLE => ((0xE0, 0x4F, 0x1A), (0xFF, 0x8A, 0x5C)),
        tj::MADD_OBLIGATORY => ((0xC4, 0x1A, 0x1A), (0xFF, 0x6B, 0x6B)),
        tj::MADD_NECESSARY => ((0x8E, 0x0F, 0x5A), (0xE0, 0x4D, 0xA0)),
        _ => return None,
    };
    let (r, g, b) = if dark { d } else { l };
    Some(Color32::from_rgb(r, g, b))
}

/// Size of a word drawn without its marks.
fn marked_word_size(painter: &egui::Painter, word: &str, font: &FontId) -> egui::Vec2 {
    let (bases, _, _, _) = split_marks(word);
    painter
        .layout_no_wrap(bases, font.clone(), Color32::WHITE)
        .size()
}

/// Offset of every mark of `word` from the word's top-left corner (cached).
fn mark_offsets(
    painter: &egui::Painter,
    word: &str,
    bases: &str,
    marks: &[Vec<char>],
    splits: &[usize],
    font: &FontId,
) -> Vec<(char, egui::Vec2, usize)> {
    let key = (word.to_owned(), font.size.to_bits(), font.family.clone());
    if let Some(hit) = MARK_CACHE.with(|c| c.borrow().get(&key).cloned()) {
        return hit;
    }

    // Tight ink box of some text, in its own coordinates.
    let ink = |text: &str| -> Option<egui::Rect> {
        let r = painter
            .layout_no_wrap(text.to_owned(), font.clone(), Color32::WHITE)
            .mesh_bounds;
        if r.is_positive() { Some(r) } else { None }
    };

    let gap = font.size * MARK_GAP;
    let mut out: Vec<(char, egui::Vec2, usize)> = Vec::new();
    let mut prefix = String::new();
    for (i, letter) in bases.chars().enumerate() {
        prefix.push(letter);
        if marks[i].is_empty() {
            continue;
        }
        let (Some(single), Some(upto)) = (ink(&letter.to_string()), ink(&prefix)) else {
            continue;
        };
        // Ink box of this letter inside the word: its right edge is the right
        // edge of everything laid out so far.
        let right = upto.max.x;
        let base = egui::Rect::from_min_max(
            egui::pos2(right - single.width(), single.min.y),
            egui::pos2(right, single.max.y),
        );
        // A lam-alef ligature carries the marks of two letters: the lam's go over its
        // stem (right), the alef's over the alef (left), each stacked on its own.
        let split = if arabic::is_lam_alef_ligature(letter) {
            splits.get(i).copied().unwrap_or(usize::MAX)
        } else {
            usize::MAX
        };
        let anchors = if split != usize::MAX {
            [0.70, 0.30]
        } else {
            [mark_anchor(letter); 2]
        };
        let mut above_top = [base.top(); 2];
        let mut below_bottom = [base.bottom(); 2];
        for (j, &mark) in marks[i].iter().enumerate() {
            let Some(n) = ink(&mark.to_string()) else {
                continue;
            };
            let g = if split != usize::MAX && j >= split { 1 } else { 0 };
            let anchor_x = base.left() + base.width() * anchors[g];
            let dx = anchor_x - n.center().x;
            let dy = if is_below_mark(mark) {
                let dy = below_bottom[g] + gap - n.top();
                below_bottom[g] = n.bottom() + dy;
                dy
            } else {
                let dy = above_top[g] - gap - n.bottom();
                above_top[g] = n.top() + dy;
                dy
            };
            out.push((mark, egui::vec2(dx, dy), i));
        }
    }

    MARK_CACHE.with(|c| {
        let mut c = c.borrow_mut();
        if c.len() > 30_000 {
            c.clear();
        }
        c.insert(key, out.clone());
    });
    out
}

/// Draws one shaped Arabic word; `left_center` is the middle of its left edge.
fn draw_marked_word(
    painter: &egui::Painter,
    left_center: Pos2,
    word: &str,
    font: &FontId,
    color: Color32,
    dark: bool,
) {
    let (bases, marks, classes, splits) = split_marks(word);
    let colored = classes.iter().any(|&(l, m)| l != 0 || m != 0);
    let galley = if colored {
        // One section per letter so each letter can have its own tajweed colour.
        let mut job = egui::text::LayoutJob::default();
        for (ch, &(lc, _)) in bases.chars().zip(classes.iter()) {
            let col = tajweed_color(lc, dark).unwrap_or(color);
            job.append(
                &ch.to_string(),
                0.0,
                egui::TextFormat::simple(font.clone(), col),
            );
        }
        painter.layout_job(job)
    } else {
        painter.layout_no_wrap(bases.clone(), font.clone(), color)
    };
    let origin = egui::Align2::LEFT_CENTER
        .anchor_size(left_center, galley.size())
        .min;
    painter.galley(origin, galley, color);
    if marks.iter().all(|m| m.is_empty()) {
        return;
    }
    for (mark, off, idx) in mark_offsets(painter, word, &bases, &marks, &splits, font) {
        let (lc, mc) = classes.get(idx).copied().unwrap_or((0, 0));
        // Tanween, dagger alef, small waw/yeh and the madda sign can have their own colour.
        let class = if arabic::is_tajweed_mark(mark) && mc != 0 { mc } else { lc };
        let col = tajweed_color(class, dark).unwrap_or(color);
        let g = painter.layout_no_wrap(mark.to_string(), font.clone(), col);
        painter.galley(origin + off, g, col);
    }
}

fn virtual_list(
    ui: &mut egui::Ui,
    heights: &mut [f32],
    mut draw: impl FnMut(&mut egui::Ui, usize) -> f32,
) {
    for i in 0..heights.len() {
        let estimate = heights[i];
        let top = ui.cursor().top();
        let clip = ui.clip_rect();
        if top + estimate >= clip.top() - OVERSCAN && top <= clip.bottom() + OVERSCAN {
            heights[i] = draw(ui, i);
        } else {
            ui.allocate_exact_size(egui::vec2(ui.available_width(), estimate), Sense::hover());
        }
    }
}

fn arabic_block(ui: &mut egui::Ui, shaped: &str, font: &FontId, color: Color32) {
    arabic_block_hl(ui, shaped, font, color, &[], Color32::TRANSPARENT);
}

/// Like `arabic_block`, but words flagged in `hl` (same order as the words) get a
/// coloured background. Used by the search results.
fn arabic_block_hl(
    ui: &mut egui::Ui,
    shaped: &str,
    font: &FontId,
    color: Color32,
    hl: &[bool],
    hl_color: Color32,
) {
    ui.with_layout(
        Layout::right_to_left(Align::Min).with_main_wrap(true),
        |ui| {
            ui.spacing_mut().item_spacing.x = font.size * 0.28;
            ui.spacing_mut().item_spacing.y = font.size * 0.25;
            for (j, word) in shaped.split(' ').enumerate() {
                let size = marked_word_size(ui.painter(), word, font);
                let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
                if ui.is_rect_visible(rect) {
                    if hl.get(j).copied().unwrap_or(false) {
                        let pad = font.size * 0.08;
                        let area = egui::Rect::from_min_max(
                            rect.min - egui::vec2(pad, font.size * 0.3),
                            rect.max + egui::vec2(pad, pad),
                        );
                        ui.painter()
                            .rect_filled(area, CornerRadius::same(4), hl_color);
                    }
                    draw_marked_word(
                        ui.painter(),
                        rect.left_center(),
                        word,
                        font,
                        color,
                        ui.visuals().dark_mode,
                    );
                }
            }
        },
    );
}

/// Byte ranges of every case-insensitive occurrence of `needle` in `text`.
fn find_matches_ci(text: &str, needle: &str) -> Vec<(usize, usize)> {
    if !needle.is_ascii() {
        // Letters such as the Luganda eng (lower and upper case): compare in lower
        // case. Byte positions only carry over when lower-casing keeps the length.
        let (h, n) = (text.to_lowercase(), needle.to_lowercase());
        if n.is_empty() || h.len() != text.len() {
            return Vec::new();
        }
        return h
            .match_indices(&n)
            .map(|(i, m)| (i, i + m.len()))
            .filter(|&(s, e)| text.is_char_boundary(s) && text.is_char_boundary(e))
            .collect();
    }
    let (h, n) = (text.as_bytes(), needle.as_bytes());
    let mut out = Vec::new();
    if n.is_empty() || n.len() > h.len() {
        return out;
    }
    let mut i = 0;
    while i + n.len() <= h.len() {
        if h[i..i + n.len()].eq_ignore_ascii_case(n) {
            out.push((i, i + n.len()));
            i += n.len();
        } else {
            i += 1;
        }
    }
    out
}

/// `text` as a label with every match of `needle` highlighted.
fn highlight_job(
    ui: &egui::Ui,
    text: &str,
    needle: &str,
    color: Color32,
    hl: Color32,
) -> egui::text::LayoutJob {
    let font = egui::TextStyle::Body.resolve(ui.style());
    let plain = egui::TextFormat {
        font_id: font.clone(),
        color,
        ..Default::default()
    };
    let marked = egui::TextFormat {
        font_id: font,
        color,
        background: hl,
        ..Default::default()
    };
    let mut job = egui::text::LayoutJob::default();
    let mut last = 0;
    for (a, b) in find_matches_ci(text, needle) {
        if a > last {
            job.append(&text[last..a], 0.0, plain.clone());
        }
        job.append(&text[a..b], 0.0, marked.clone());
        last = b;
    }
    if last < text.len() {
        job.append(&text[last..], 0.0, plain);
    }
    job
}

/// For each word of `arabic` that is drawn (same order as `arabic::shape_words`),
/// whether it is part of a match of `needle` (already normalised for searching).
fn arabic_match_words(arabic: &str, needle: &str) -> Vec<bool> {
    if needle.is_empty() {
        return Vec::new();
    }
    let mut joined = String::new();
    // (start, end, is drawn) of every word inside `joined`.
    let mut spans: Vec<(usize, usize, bool)> = Vec::new();
    let mut tmp = String::new();
    for word in arabic.split_whitespace() {
        if !spans.is_empty() {
            joined.push(' ');
        }
        let start = joined.len();
        arabic::normalize_into(word, &mut joined);
        tmp.clear();
        arabic::shape_word(word, true, &mut tmp);
        spans.push((start, joined.len(), !tmp.is_empty()));
    }
    let mut hit = vec![false; spans.len()];
    for (pos, m) in joined.match_indices(needle) {
        let end = pos + m.len();
        for (k, &(a, b, _)) in spans.iter().enumerate() {
            if a < end && b > pos {
                hit[k] = true;
            }
        }
    }
    spans
        .iter()
        .zip(hit)
        .filter(|(sp, _)| sp.2)
        .map(|(_, h)| h)
        .collect()
}

fn toggle_switch(ui: &mut egui::Ui, on: &mut bool) -> egui::Response {
	//translation button size 
    let h = ui.spacing().interact_size.y * 0.55;
    let (rect, mut response) = ui.allocate_exact_size(egui::vec2(h * 1.9, h), Sense::click());
    if response.clicked() {
        *on = !*on;
        response.mark_changed();
    }
    if ui.is_rect_visible(rect) {
        let t = ui.ctx().animate_bool(response.id, *on);
        let dark = ui.visuals().dark_mode;
        let off_color = if dark {
            theme::glass(Theme::Dark, 70)
        } else {
            Color32::from_black_alpha(60)
        };
        let color = off_color.lerp_to_gamma(
            theme::accent(if dark { Theme::Dark } else { Theme::Light }),
            t,
        );
        let radius = h * 0.5;
        ui.painter()
            .rect_filled(rect, CornerRadius::same(radius as u8), color);
        let x = egui::lerp((rect.left() + radius)..=(rect.right() - radius), t);
        ui.painter().circle_filled(
            egui::pos2(x, rect.center().y),
            radius * 0.78,
            Color32::WHITE,
        );
    }
    response
}

fn surah_row(
    ui: &mut egui::Ui,
    info: &SurahInfo,
    label: &SurahLabel,
    ar_font: &FontId,
    selected: bool,
) -> egui::Response {
    let dark = ui.visuals().dark_mode;
    let th = if dark { Theme::Dark } else { Theme::Light };
    let fill = if selected {
        theme::glass(th, 44)
    } else {
        theme::glass(th, 12)
    };
    let stroke = Stroke::new(1.0, theme::border(th, if selected { 110 } else { 34 }));
    let out = egui::Frame::new()
        .fill(fill)
        .stroke(stroke)
        .corner_radius(CornerRadius::same(16))
        .inner_margin(egui::Margin::symmetric(12, 10))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.label(RichText::new(label.number.as_str()).monospace().weak());
                ui.label(RichText::new(info.translit).strong());
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.label(RichText::new(label.arabic.as_str()).font(ar_font.clone()));
                });
            });
            ui.label(RichText::new(label.meta.as_str()).small().weak());
        });
    ui.interact(
        out.response.rect,
        ui.id().with(("surah_row", info.number)),
        Sense::click(),
    )
}

fn find_arabic_font() -> Option<Vec<u8>> {
    let font_bytes = include_bytes!("../assets/quran_font.ttf");
    Some(font_bytes.to_vec())
}

/// True if the font file has a glyph for every char in `chars`.
fn font_has_glyphs(bytes: &[u8], chars: &[char]) -> bool {
    use ab_glyph::Font as _;
    match ab_glyph::FontRef::try_from_slice(bytes) {
        Ok(f) => chars.iter().all(|&c| f.glyph_id(c).0 != 0),
        Err(_) => false,
    }
}

/// Looks for the phone's own Latin font (Android system fonts). Returns None on
/// other platforms or when none is found, so the built-in egui font is kept.
fn find_system_font() -> Option<Vec<u8>> {
    // Light weights come first: egui's own font is a light one, and a Regular
    // weight looks noticeably bolder next to it.
    const CANDIDATES: &[&str] = &[
        "/system/fonts/Roboto-Light.ttf",
        "/system/fonts/RobotoStatic-Light.ttf",
        "/system/fonts/NotoSans-Light.ttf",
        "/system/fonts/GoogleSans-Light.ttf",
        "/system/fonts/Roboto-Regular.ttf",
        "/system/fonts/RobotoStatic-Regular.ttf",
        "/system/fonts/GoogleSans-Regular.ttf",
        "/system/fonts/NotoSans-Regular.ttf",
        "/system/fonts/DroidSans.ttf",
    ];
    for path in CANDIDATES {
        if let Ok(meta) = std::fs::metadata(path) {
            if meta.len() == 0 || meta.len() > 8 * 1024 * 1024 {
                continue;
            }
            if let Ok(bytes) = std::fs::read(path) {
                log::info!("Latin text font: {path}");
                return Some(bytes);
            }
        }
    }
    None
}

fn setup_fonts(ctx: &egui::Context) -> (FontFamily, bool) {
    let mut fonts = egui::FontDefinitions::default();
    let family = FontFamily::Name("arabic".into());
    let mut chain: Vec<String> = Vec::new();
    let mut found = false;

    if let Some(bytes) = find_arabic_font() {
        fonts.font_data.insert(
            "arabic".to_owned(),
            egui::FontData::from_owned(bytes).into(),
        );
        chain.push("arabic".to_owned());
        found = true;
        arabic::set_wasla_forms(true);
        // Use the lam-alef ligature glyphs only if the font really has them;
        // otherwise keep lam and alef as two separate (overlapping) glyphs.
        let bytes = fonts.font_data["arabic"].font.as_ref();
        let has_lig = font_has_glyphs(bytes, &['\u{FEFB}', '\u{FEFC}']);
        let has_hamza_lig = font_has_glyphs(
            bytes,
            &['\u{FEF5}', '\u{FEF6}', '\u{FEF7}', '\u{FEF8}', '\u{FEF9}', '\u{FEFA}'],
        );
        log::info!("lam-alef ligature glyphs in font: {has_lig} (with hamza: {has_hamza_lig})");
        arabic::set_lam_alef_ligatures(has_lig);
        arabic::set_lam_alef_hamza_ligatures(has_hamza_lig);
    }
    if let Some(base) = fonts.families.get(&FontFamily::Proportional) {
        chain.extend(base.iter().cloned());
    }
    fonts.families.insert(family.clone(), chain);
    // Small text keeps egui's built-in font (the family must always exist, because
    // the theme's Small text style points at it).
    let mut small_chain: Vec<String> = fonts
        .families
        .get(&FontFamily::Proportional)
        .cloned()
        .unwrap_or_default();
    if found {
        small_chain.push("arabic".to_owned());
    }
    fonts
        .families
        .insert(FontFamily::Name("small_text".into()), small_chain);
    if found {
        fonts
            .families
            .entry(FontFamily::Proportional)
            .or_default()
            .push("arabic".to_owned());
    }
    // Every other Latin (alphabetic) text uses the phone's system font when found.
    if let Some(bytes) = find_system_font() {
        fonts.font_data.insert(
            "system_latin".to_owned(),
            egui::FontData::from_owned(bytes).into(),
        );
        fonts
            .families
            .entry(FontFamily::Proportional)
            .or_default()
            .insert(0, "system_latin".to_owned());
    }
    ctx.set_fonts(fonts);
    (family, found)
}
