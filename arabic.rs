//! Arabic text helpers for egui.
//!
//! egui 0.31 (ab_glyph based) has NO text shaper and NO bidi support: it draws
//! one codepoint after another, left to right. Raw Arabic therefore shows up as
//! disconnected, reversed letters. We work around it in two steps:
//!
//! 1. every letter is replaced by its contextual (isolated / final / initial /
//!    medial) glyph from the "Arabic Presentation Forms-B" block, and
//! 2. each word is emitted in *visual* order (reversed).
//!
//! Words are kept in logical order. Marks (harakat) stay right after their base
//! letter in the string; `app.rs` positions them itself (egui cannot).

use std::sync::atomic::{AtomicBool, Ordering};

/// When a real Quran font is bundled, alef wasla (ٱ) can use its own forms
/// (U+FB50 / U+FB51); otherwise it is drawn as a plain alef.
static WASLA_FORMS: AtomicBool = AtomicBool::new(false);

pub fn set_wasla_forms(on: bool) {
    WASLA_FORMS.store(on, Ordering::Relaxed);
}

/// Lam + alef must be drawn as ONE ligature glyph (U+FEF5..U+FEFC). Without it
/// the lam's stem and the alef overlap and look broken. Turned on only when the
/// bundled font really contains those glyphs (see `setup_fonts`).
static LAM_ALEF_LIGATURES: AtomicBool = AtomicBool::new(true);

pub fn set_lam_alef_ligatures(on: bool) {
    LAM_ALEF_LIGATURES.store(on, Ordering::Relaxed);
}

/// The hamza variants (لأ لإ لآ) need their own ligature glyphs. Some fonts only
/// have the plain لا one; then the others stay two separate letters.
static LAM_ALEF_HAMZA: AtomicBool = AtomicBool::new(true);

pub fn set_lam_alef_hamza_ligatures(on: bool) {
    LAM_ALEF_HAMZA.store(on, Ordering::Relaxed);
}

/// Isolated presentation form of the lam-alef ligature for a given alef
/// (final form = isolated + 1). `None` if `alef` does not form one.
fn lam_alef_ligature(alef: char) -> Option<u32> {
    if alef != '\u{0627}' && !LAM_ALEF_HAMZA.load(Ordering::Relaxed) {
        return None;
    }
    match alef {
        '\u{0622}' => Some(0xFEF5), // لآ
        '\u{0623}' => Some(0xFEF7), // لأ
        '\u{0625}' => Some(0xFEF9), // لإ
        '\u{0627}' => Some(0xFEFB), // لا
        _ => None,
    }
}

/// True if `c` is a shaped lam-alef ligature glyph.
pub fn is_lam_alef_ligature(c: char) -> bool {
    ('\u{FEF5}'..='\u{FEFC}').contains(&c)
}

/// One letter (or lam+alef pair) with the marks that belong to it.
struct Cluster {
    base: char,
    /// Second letter when `base` is a lam fused with an alef.
    alef: Option<char>,
    marks: String,
    /// Tajweed class of the letter (and its marks) / of its special marks only.
    letter_class: u8,
    mark_class: u8,
    /// For a lam-alef ligature: how many bytes of `marks` belong to the lam (the
    /// rest belong to the alef). `None` for every other cluster.
    lam_marks_len: Option<usize>,
}

impl Cluster {
    /// Letter that connects to the PREVIOUS cluster.
    fn first(&self) -> char {
        self.base
    }
    /// Letter that connects to the NEXT cluster (alef never does).
    fn last(&self) -> char {
        self.alef.unwrap_or(self.base)
    }
}

/// True for combining marks: harakat, Quranic annotation signs, dagger alef.
pub fn is_mark(c: char) -> bool {
    matches!(
        c,
        '\u{0610}'..='\u{061A}'
            | '\u{064B}'..='\u{065F}'
            | '\u{0670}'
            | '\u{06D6}'..='\u{06DC}'
            | '\u{06DF}'..='\u{06E8}'
            | '\u{06EA}'..='\u{06ED}'
            | '\u{08D3}'..='\u{08FF}'
    )
}

/// True for characters in the basic Arabic block.
pub fn is_arabic_char(c: char) -> bool {
    ('\u{0600}'..='\u{06FF}').contains(&c)
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Joining {
    /// Never connects (e.g. hamza).
    Non,
    /// Connects to the previous letter only (alef, dal, ra, waw ...).
    Right,
    /// Connects on both sides.
    Dual,
}

/// Joining class and the isolated presentation form (Arabic Presentation
/// Forms-B, U+FE70..). Dual letters have 4 consecutive forms:
/// isolated, final, initial, medial. Right-joining letters have 2.
fn joining(c: char) -> Option<(Joining, u32)> {
    use Joining::{Dual, Non, Right};
    Some(match c {
        '\u{0621}' => (Non, 0xFE80),   // ء
        '\u{0622}' => (Right, 0xFE81), // آ
        '\u{0623}' => (Right, 0xFE83), // أ
        '\u{0624}' => (Right, 0xFE85), // ؤ
        '\u{0625}' => (Right, 0xFE87), // إ
        '\u{0626}' => (Dual, 0xFE89),  // ئ
        '\u{0627}' => (Right, 0xFE8D), // ا
        // ٱ alef wasla: own forms with a bundled Quran font, plain alef otherwise.
        '\u{0671}' => (
            Right,
            if WASLA_FORMS.load(Ordering::Relaxed) {
                0xFB50
            } else {
                0xFE8D
            },
        ),
        '\u{0628}' => (Dual, 0xFE8F),  // ب
        '\u{0629}' => (Right, 0xFE93), // ة
        '\u{062A}' => (Dual, 0xFE95),  // ت
        '\u{062B}' => (Dual, 0xFE99),  // ث
        '\u{062C}' => (Dual, 0xFE9D),  // ج
        '\u{062D}' => (Dual, 0xFEA1),  // ح
        '\u{062E}' => (Dual, 0xFEA5),  // خ
        '\u{062F}' => (Right, 0xFEA9), // د
        '\u{0630}' => (Right, 0xFEAB), // ذ
        '\u{0631}' => (Right, 0xFEAD), // ر
        '\u{0632}' => (Right, 0xFEAF), // ز
        '\u{0633}' => (Dual, 0xFEB1),  // س
        '\u{0634}' => (Dual, 0xFEB5),  // ش
        '\u{0635}' => (Dual, 0xFEB9),  // ص
        '\u{0636}' => (Dual, 0xFEBD),  // ض
        '\u{0637}' => (Dual, 0xFEC1),  // ط
        '\u{0638}' => (Dual, 0xFEC5),  // ظ
        '\u{0639}' => (Dual, 0xFEC9),  // ع
        '\u{063A}' => (Dual, 0xFECD),  // غ
        '\u{0640}' => (Dual, 0x0640),  // tatweel (kept as is)
        '\u{0641}' => (Dual, 0xFED1),  // ف
        '\u{0642}' => (Dual, 0xFED5),  // ق
        '\u{0643}' => (Dual, 0xFED9),  // ك
        '\u{0644}' => (Dual, 0xFEDD),  // ل
        '\u{0645}' => (Dual, 0xFEE1),  // م
        '\u{0646}' => (Dual, 0xFEE5),  // ن
        '\u{0647}' => (Dual, 0xFEE9),  // ه
        '\u{0648}' => (Right, 0xFEED), // و
        '\u{0649}' => (Right, 0xFEEF), // ى
        '\u{064A}' => (Dual, 0xFEF1),  // ي
        _ => return None,
    })
}

fn connects_forward(c: char) -> bool {
    matches!(joining(c), Some((Joining::Dual, _)))
}

fn connects_backward(c: char) -> bool {
    matches!(joining(c), Some((Joining::Right | Joining::Dual, _)))
}

fn shaped_char(c: char, joins_prev: bool, joins_next: bool) -> char {
    let Some((kind, iso)) = joining(c) else {
        return c;
    };
    if c == '\u{0640}' {
        return c;
    }
    let offset = match (kind, joins_prev, joins_next) {
        (Joining::Non, _, _) => 0,
        (Joining::Right, false, _) => 0,
        (Joining::Right, true, _) => 1,
        (Joining::Dual, false, false) => 0,
        (Joining::Dual, true, false) => 1,
        (Joining::Dual, false, true) => 2,
        (Joining::Dual, true, true) => 3,
    };
    char::from_u32(iso + offset).unwrap_or(c)
}

/// Shapes ONE word and appends it to `out` in visual (left-to-right) order.
///
/// With `keep_marks == false` all combining marks are dropped (readable
/// skeleton text). With `true` they are kept right after their base letter.
/// Stand-alone annotation signs (stop signs written as separate tokens) have no
/// base letter and are dropped in both modes.
pub fn shape_word(word: &str, keep_marks: bool, out: &mut String) {
    shape_word_ex(word, keep_marks, None, out);
}

/// Like `shape_word`; `classes` (from `analyze_tajweed`, one `(letter, mark)`
/// pair per letter of the word) is written into the output as invisible tag
/// characters that `app.rs` turns into colours.
pub fn shape_word_ex(word: &str, keep_marks: bool, classes: Option<&[(u8, u8)]>, out: &mut String) {
    let ligatures = LAM_ALEF_LIGATURES.load(Ordering::Relaxed);
    let mut clusters: Vec<Cluster> = Vec::with_capacity(word.len() / 2 + 1);
    // Index of the current letter, counted like `analyze_tajweed` counts them.
    let mut letter_idx = 0usize;
    for c in word.chars() {
        if is_mark(c) {
            if keep_marks {
                if let Some(last) = clusters.last_mut() {
                    last.marks.push(c);
                }
            }
            continue;
        }
        let (lc, mc) = classes
            .and_then(|k| k.get(letter_idx))
            .copied()
            .unwrap_or((0, 0));
        letter_idx += 1;
        // Fuse lam + alef into one cluster (marks in between are kept).
        if ligatures && lam_alef_ligature(c).is_some() {
            if let Some(prev) = clusters.last_mut() {
                if prev.base == '\u{0644}' && prev.alef.is_none() {
                    prev.alef = Some(c);
                    prev.lam_marks_len = Some(prev.marks.len());
                    // The alef decides the colour of the single ligature glyph.
                    if lc != 0 {
                        prev.letter_class = lc;
                    }
                    if mc != 0 {
                        prev.mark_class = mc;
                    }
                    continue;
                }
            }
        }
        clusters.push(Cluster {
            base: c,
            alef: None,
            marks: String::new(),
            letter_class: lc,
            mark_class: mc,
            lam_marks_len: None,
        });
    }

    let n = clusters.len();
    // Iterate backwards to reverse the word for LTR rendering
    for i in (0..n).rev() {
        let cl = &clusters[i];
        let joins_prev =
            i > 0 && connects_forward(clusters[i - 1].last()) && connects_backward(cl.first());
        let joins_next =
            i + 1 < n && connects_forward(cl.last()) && connects_backward(clusters[i + 1].first());

        match cl.alef.and_then(lam_alef_ligature) {
            Some(iso) => {
                // Lam-alef never connects forward; the final form joins the previous letter.
                let g = char::from_u32(iso + joins_prev as u32).unwrap_or(cl.base);
                out.push(g);
            }
            None => out.push(shaped_char(cl.base, joins_prev, joins_next)),
        }
        // Tajweed tags sit right after the glyph; they are not drawn.
        if cl.letter_class != 0 {
            out.push(tag_char(cl.letter_class, false));
        }
        if cl.mark_class != 0 && keep_marks {
            out.push(tag_char(cl.mark_class, true));
        }
        // Marks go immediately AFTER the (single) glyph; app.rs positions them.
        match cl.lam_marks_len {
            // Ligature: the lam's marks, a separator, then the alef's marks, so the
            // two sets can be placed over their own half of the glyph.
            Some(n) if n < cl.marks.len() => {
                out.push_str(&cl.marks[..n]);
                out.push(ALEF_MARKS_SEP);
                out.push_str(&cl.marks[n..]);
            }
            _ => out.push_str(&cl.marks),
        }
    }
}

// ---- Tajweed -----------------------------------------------------------------
//
// The rules are worked out on the LOGICAL text (before shaping), one verse at a
// time, because most of them look at the next word (noon sakinah, meem sakinah,
// madd before a hamza ...). The result is one `(letter_class, mark_class)` pair
// per letter. `letter_class` colours the whole letter with its marks;
// `mark_class` colours only its special marks (tanween, dagger alef, small
// waw/yeh, madda sign), so e.g. only the tanween of an ikhfa is coloured.

/// Tajweed class ids (0 = nothing). Colours live in `app.rs`.
pub mod tj {
    pub const NONE: u8 = 0;
    /// Silent: hamzat wasl, lam shamsiyyah, letters with a small zero.
    pub const SILENT: u8 = 1;
    /// Ghunnah: noon/meem with shadda, idgham with ghunnah, idgham shafawi.
    pub const GHUNNA: u8 = 2;
    /// Ikhfa (and ikhfa shafawi).
    pub const IKHFA: u8 = 3;
    /// Iqlab.
    pub const IQLAB: u8 = 4;
    /// Idgham without ghunnah (noon before lam / ra).
    pub const IDGHAM: u8 = 5;
    pub const QALQALAH: u8 = 6;
    /// Natural madd, 2 counts.
    pub const MADD_NATURAL: u8 = 7;
    /// Permissible madd (munfasil / silah kubra), 2 / 4 / 5 counts.
    pub const MADD_PERMISSIBLE: u8 = 8;
    /// Connected madd (muttasil), 4 / 5 counts.
    pub const MADD_OBLIGATORY: u8 = 9;
    /// Necessary madd (lazim), 6 counts.
    pub const MADD_NECESSARY: u8 = 10;
}

/// (class, short name, what it is) for the legend in the settings.
pub const TAJWEED_LEGEND: &[(u8, &str)] = &[
    (tj::MADD_NATURAL, "Madd – natural (2)"),
    (tj::MADD_PERMISSIBLE, "Madd – permissible (2/4/5)"),
    (tj::MADD_OBLIGATORY, "Madd – connected (4/5)"),
    (tj::MADD_NECESSARY, "Madd – necessary (6)"),
    (tj::GHUNNA, "Ghunnah / idgham with ghunnah"),
    (tj::IKHFA, "Ikhfa"),
    (tj::IQLAB, "Iqlab"),
    (tj::IDGHAM, "Idgham without ghunnah"),
    (tj::QALQALAH, "Qalqalah"),
    (tj::SILENT, "Silent letter"),
];

const TAG_LETTER: u32 = 0xE000;
const TAG_MARKS: u32 = 0xE100;

/// Invisible private-use character that carries a tajweed class in a shaped word.
fn tag_char(class: u8, marks_only: bool) -> char {
    let base = if marks_only { TAG_MARKS } else { TAG_LETTER };
    char::from_u32(base + class as u32).unwrap_or('\u{E000}')
}

/// Invisible separator inside a lam-alef cluster: marks before it belong to the
/// lam, marks after it to the alef.
pub const ALEF_MARKS_SEP: char = '\u{E200}';

/// True for the invisible tag characters (tajweed classes and the separator).
pub fn is_tag(c: char) -> bool {
    ('\u{E000}'..='\u{E2FF}').contains(&c)
}

/// `(class, marks_only)` of a tag character.
pub fn decode_tag(c: char) -> Option<(u8, bool)> {
    let u = c as u32;
    if (TAG_LETTER..TAG_LETTER + 0x100).contains(&u) {
        Some(((u - TAG_LETTER) as u8, false))
    } else if (TAG_MARKS..TAG_MARKS + 0x100).contains(&u) {
        Some(((u - TAG_MARKS) as u8, true))
    } else {
        None
    }
}

/// Tanween in all the spellings the Uthmani texts use (closed and open).
fn is_tanween(c: char) -> bool {
    matches!(
        c,
        '\u{064B}'
            | '\u{064C}'
            | '\u{064D}'
            | '\u{08F0}'
            | '\u{08F1}'
            | '\u{08F2}'
            | '\u{0657}'
            | '\u{065E}'
    )
}

/// Marks that can be coloured on their own (`mark_class`).
pub fn is_tajweed_mark(c: char) -> bool {
    is_tanween(c)
        || matches!(
            c,
            '\u{0670}' | '\u{0653}' | '\u{06E5}' | '\u{06E6}' | '\u{06E7}'
        )
}

/// A letter of the logical text with its marks.
struct Letter {
    ch: char,
    marks: Vec<char>,
}

impl Letter {
    fn has(&self, c: char) -> bool {
        self.marks.contains(&c)
    }
    fn tanween(&self) -> bool {
        self.marks.iter().any(|&m| is_tanween(m))
    }
    fn vowel(&self) -> bool {
        self.tanween()
            || self
                .marks
                .iter()
                .any(|&m| matches!(m, '\u{064E}' | '\u{064F}' | '\u{0650}' | '\u{0656}'))
    }
    fn sukun(&self) -> bool {
        self.has('\u{0652}') || self.has('\u{06E1}')
    }
    fn shadda(&self) -> bool {
        self.has('\u{0651}')
    }
    /// No vowel and no shadda (a sukun is allowed).
    fn plain(&self) -> bool {
        !self.vowel() && !self.shadda()
    }
    /// Nothing that says how the letter is pronounced: in the Uthmani script this
    /// is how a noon/meem that is merged (idgham) or hidden (ikhfa) is written.
    fn bare(&self) -> bool {
        self.plain() && !self.sukun()
    }
    fn is_hamza(&self) -> bool {
        is_hamza(self.ch)
    }
}

fn is_hamza(c: char) -> bool {
    matches!(
        c,
        '\u{0621}' | '\u{0623}' | '\u{0624}' | '\u{0625}' | '\u{0626}'
    )
}

fn letters_of(word: &str) -> Vec<Letter> {
    let mut v: Vec<Letter> = Vec::new();
    for c in word.chars() {
        if is_mark(c) {
            if let Some(l) = v.last_mut() {
                l.marks.push(c);
            }
        } else {
            v.push(Letter {
                ch: c,
                marks: Vec::new(),
            });
        }
    }
    v
}

/// What follows a noon sakinah / tanween: the class for the noon (or tanween)
/// and whether the next letter should get the ghunnah colour too.
fn noon_rule(next: Option<&Letter>, same_word: bool) -> (u8, bool) {
    let Some(n) = next else {
        return (tj::NONE, false);
    };
    match n.ch {
        '\u{0628}' => (tj::IQLAB, false),
        // Idgham with ghunnah only between two words (yanmu): inside a word it is izhar.
        '\u{064A}' | '\u{0648}' | '\u{0646}' | '\u{0645}' => {
            if same_word {
                (tj::NONE, false)
            } else {
                (tj::GHUNNA, true)
            }
        }
        '\u{0644}' | '\u{0631}' => (tj::IDGHAM, false),
        '\u{062A}' | '\u{062B}' | '\u{062C}' | '\u{062F}' | '\u{0630}' | '\u{0632}'
        | '\u{0633}' | '\u{0634}' | '\u{0635}' | '\u{0636}' | '\u{0637}' | '\u{0638}'
        | '\u{0641}' | '\u{0642}' | '\u{0643}' => (tj::IKHFA, false),
        _ => (tj::NONE, false), // izhar (throat letters and the rest)
    }
}

/// Madd class for a madd letter / mark sitting at `w[i]`.
/// `explicit` = the letter carries the madda sign (U+0653).
fn madd_class(w: &[Letter], i: usize, next_word: Option<&[Letter]>, explicit: bool) -> u8 {
    // A silent alef after waw (qaluu-) is only spelling: the waw ends the word.
    let following = match w.get(i + 1) {
        Some(n) if (n.has('\u{06DF}') || n.has('\u{06E0}')) && i + 2 == w.len() => None,
        other => other,
    };
    if let Some(n) = following {
        if n.is_hamza() {
            return tj::MADD_OBLIGATORY;
        }
        if explicit && (n.shadda() || n.sukun()) {
            return tj::MADD_NECESSARY;
        }
        return if explicit {
            tj::MADD_PERMISSIBLE
        } else {
            tj::MADD_NATURAL
        };
    }
    // Last letter of the word: look at the first letter of the next word.
    match next_word.and_then(|nw| nw.first()) {
        Some(n) if n.is_hamza() => tj::MADD_PERMISSIBLE,
        Some(n) if n.ch == '\u{0671}' => tj::NONE, // shortened before hamzat wasl
        _ if explicit => tj::MADD_PERMISSIBLE,
        _ => tj::MADD_NATURAL,
    }
}

/// First letter after letter `i` of word `wi`: the letter, whether it is in the
/// same word, and its (word, letter) position.
fn after_letter<'a>(
    words: &'a [Vec<Letter>],
    wi: usize,
    i: usize,
) -> (Option<&'a Letter>, bool, Option<(usize, usize)>) {
    if i + 1 < words[wi].len() {
        (Some(&words[wi][i + 1]), true, Some((wi, i + 1)))
    } else if let Some(nj) = (wi + 1..words.len()).find(|&j| !words[j].is_empty()) {
        (Some(&words[nj][0]), false, Some((nj, 0)))
    } else {
        (None, false, None)
    }
}

/// Works out the tajweed class of every letter of `text` (one verse).
/// The result has one entry per whitespace-separated word (also for words that
/// have no letters) and one `(letter_class, mark_class)` per letter of that word.
pub fn analyze_tajweed(text: &str) -> Vec<Vec<(u8, u8)>> {
    let words: Vec<Vec<Letter>> = text.split_whitespace().map(letters_of).collect();
    let mut out: Vec<Vec<(u8, u8)>> = words.iter().map(|w| vec![(0, 0); w.len()]).collect();
    let first_word = words.iter().position(|w| !w.is_empty());
    let last_word = words.iter().rposition(|w| !w.is_empty());
    // Letters that must also get the ghunnah colour (idgham): (word, letter).
    let mut also_ghunna: Vec<(usize, usize)> = Vec::new();

    for wi in 0..words.len() {
        let w = &words[wi];
        if w.is_empty() {
            continue;
        }
        let next_word: Option<&[Letter]> = words[wi + 1..]
            .iter()
            .find(|x| !x.is_empty())
            .map(|v| v.as_slice());
        let after = |i: usize| after_letter(&words, wi, i);

        for i in 0..w.len() {
            let c = &w[i];
            let (mut lc, mut mc) = (tj::NONE, tj::NONE);

            // --- silent letters ---------------------------------------------
            if c.has('\u{06DF}') || c.has('\u{06E0}') {
                lc = tj::SILENT;
            } else if c.ch == '\u{0671}' {
                // Hamzat wasl is silent when the reading continues from the word before.
                if i > 0 || Some(wi) != first_word {
                    lc = tj::SILENT;
                }
            } else if c.ch == '\u{0644}'
                && i > 0
                && w[i - 1].ch == '\u{0671}'
                && c.bare()
                && w.get(i + 1).map_or(false, |n| n.shadda())
            {
                lc = tj::SILENT; // lam shamsiyyah
            }

            // --- ghunnah, noon / meem sakinah, tanween ----------------------
            if lc == tj::NONE {
                if (c.ch == '\u{0646}' || c.ch == '\u{0645}') && c.shadda() {
                    lc = tj::GHUNNA;
                } else if c.ch == '\u{0646}' && c.plain() && !c.tanween() {
                    // Noon sakinah (explicit sukun, or bare = idgham / ikhfa / iqlab).
                    let (nx, same, pos) = after(i);
                    let (cl, ghunna) = noon_rule(nx, same);
                    if cl != tj::NONE && (c.sukun() || c.bare()) {
                        lc = cl;
                        if ghunna {
                            also_ghunna.extend(pos);
                        }
                    }
                } else if c.ch == '\u{0645}' && c.plain() {
                    // Meem sakinah: ikhfa shafawi before ba, idgham shafawi before meem.
                    let (nx, _, pos) = after(i);
                    match nx.map(|n| n.ch) {
                        Some('\u{0628}') => lc = tj::IKHFA,
                        Some('\u{0645}') => {
                            lc = tj::GHUNNA;
                            also_ghunna.extend(pos);
                        }
                        _ => {}
                    }
                }
                if c.tanween() {
                    // Tanween belongs to the end of the word: skip a silent alef/ya after it.
                    let trailing = w[i + 1..].iter().all(|l| {
                        matches!(l.ch, '\u{0627}' | '\u{0649}') && !l.vowel() && !l.shadda()
                    });
                    if trailing {
                        let (nx, _, pos) = match next_word {
                            Some(nw) => (
                                nw.first(),
                                false,
                                (wi + 1..words.len())
                                    .find(|&j| !words[j].is_empty())
                                    .map(|j| (j, 0usize)),
                            ),
                            None => (None, false, None),
                        };
                        let (cl, ghunna) = noon_rule(nx, false);
                        if cl != tj::NONE {
                            mc = cl;
                            if ghunna {
                                also_ghunna.extend(pos);
                            }
                        }
                    }
                }
            }

            // --- qalqalah ---------------------------------------------------
            if lc == tj::NONE
                && matches!(
                    c.ch,
                    '\u{0642}' | '\u{0637}' | '\u{0628}' | '\u{062C}' | '\u{062F}'
                )
            {
                let verse_end = Some(wi) == last_word && i + 1 == w.len();
                if c.sukun() || verse_end {
                    lc = tj::QALQALAH;
                }
            }

            // --- madd -------------------------------------------------------
            if lc == tj::NONE {
                let prev = if i > 0 { Some(&w[i - 1]) } else { None };
                let letter_madd = match c.ch {
                    '\u{0627}' => prev.map_or(false, |p| p.has('\u{064E}')) && c.plain(),
                    '\u{0648}' => prev.map_or(false, |p| p.has('\u{064F}')) && c.plain(),
                    '\u{064A}' | '\u{0649}' => {
                        prev.map_or(false, |p| p.has('\u{0650}')) && c.plain()
                    }
                    _ => false,
                };
                let explicit = c.has('\u{0653}');
                let small = c.has('\u{0670}')
                    || c.has('\u{06E5}')
                    || c.has('\u{06E6}')
                    || c.has('\u{06E7}');
                if letter_madd {
                    lc = madd_class(w, i, next_word, explicit);
                } else if small || explicit {
                    mc = madd_class(w, i, next_word, explicit);
                }
            }

            out[wi][i] = (lc, mc);
        }
    }

    // The letter after an idgham with ghunnah carries the ghunnah colour as well.
    for (wi, li) in also_ghunna {
        if out[wi][li].0 == tj::NONE {
            out[wi][li].0 = tj::GHUNNA;
        }
    }
    out
}

/// Shapes a whole text. Words stay in LOGICAL order, separated by one space;
/// every word is individually in visual order.
pub fn shape_words(text: &str, keep_marks: bool) -> String {
    shape_words_ex(text, keep_marks, false)
}

/// Like `shape_words`; with `tajweed` every letter also carries its tajweed class.
pub fn shape_words_ex(text: &str, keep_marks: bool, tajweed: bool) -> String {
    let classes = if tajweed {
        Some(analyze_tajweed(text))
    } else {
        None
    };
    let mut out = String::with_capacity(text.len() * 2);
    for (wi, word) in text.split_whitespace().enumerate() {
        let before = out.len();
        if before > 0 {
            out.push(' ');
        }
        let start = out.len();
        let k = classes
            .as_ref()
            .and_then(|c| c.get(wi))
            .map(|v| v.as_slice());
        shape_word_ex(word, keep_marks, k, &mut out);
        if out.len() == start {
            out.truncate(before); // word shaped to nothing (annotation only)
        }
    }
    out
}

/// Shapes a short single-line text (a surah name) into one string that can be
/// drawn with a normal left-to-right label: words AND letters are reversed.
pub fn shape_line(text: &str, keep_marks: bool) -> String {
    let mut words: Vec<String> = text
        .split_whitespace()
        .map(|w| {
            let mut s = String::new();
            shape_word(w, keep_marks, &mut s);
            s
        })
        .collect();
    words.retain(|w| !w.is_empty());
    words.reverse();
    words.join(" ")
}

/// Verse number written with Arabic-Indic digits (e.g. 12 -> ١٢). Digits keep
/// their left-to-right order inside a number, so no reversal is needed.
pub fn to_arabic_indic(n: u16) -> String {
    n.to_string()
        .chars()
        .map(|c| match c.to_digit(10) {
            Some(d) => char::from_u32(0x0660 + d).unwrap_or(c),
            None => c,
        })
        .collect()
}

/// Search normalisation: drops marks/tatweel and unifies alef, ya and ta-marbuta
/// variants. Appends to `out` (no allocation if `out` has capacity).
pub fn normalize_into(text: &str, out: &mut String) {
    for c in text.chars() {
        if is_mark(c) || c == '\u{0640}' {
            continue;
        }
        out.push(match c {
            '\u{0671}' | '\u{0622}' | '\u{0623}' | '\u{0625}' => '\u{0627}',
            '\u{0649}' => '\u{064A}',
            '\u{0629}' => '\u{0647}',
            other => other,
        });
    }
}

/// ASCII case-insensitive substring test without allocating.
pub fn contains_ci(hay: &str, needle: &str) -> bool {
    if needle.is_ascii() {
        // Fast path for plain English letters and numbers.
        let (h, n) = (hay.as_bytes(), needle.as_bytes());
        return n.is_empty()
            || (n.len() <= h.len() && h.windows(n.len()).any(|w| w.eq_ignore_ascii_case(n)));
    }
    // Letters such as the Luganda eng (lower and upper case): compare in lower case.
    hay.to_lowercase().contains(&needle.to_lowercase())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn joins_letters_and_reverses() {
        // ب (initial) س (medial) م (final), emitted right-to-left.
        let mut s = String::new();
        shape_word("\u{0628}\u{0633}\u{0645}", false, &mut s);
        assert_eq!(s, "\u{FEE2}\u{FEB4}\u{FE91}");
    }

    #[test]
    fn marks_are_dropped_or_kept() {
        let word = "\u{0628}\u{0650}"; // ب + kasra
        let mut plain = String::new();
        shape_word(word, false, &mut plain);
        assert_eq!(plain, "\u{FE8F}");
        let mut marked = String::new();
        shape_word(word, true, &mut marked);
        assert_eq!(marked, "\u{FE8F}\u{0650}");
    }

    #[test]
    fn lam_alef_becomes_one_ligature() {
        // لا alone -> isolated ligature
        let mut s = String::new();
        shape_word("\u{0644}\u{0627}", false, &mut s);
        assert_eq!(s, "\u{FEFB}");
        // وَلَا : waw + fatha, lam + fatha?, alef -> waw is right-joining so lam is initial
        // -> isolated-form ligature; the fatha follows its glyph.
        let mut s = String::new();
        shape_word("\u{0648}\u{064E}\u{0644}\u{064E}\u{0627}", true, &mut s);
        assert_eq!(s, "\u{FEFB}\u{064E}\u{FEED}\u{064E}");
        // إِلَّا : hamza-alef, lam+shadda+fatha, alef -> final ligature after a non-joiner is isolated
        let mut s = String::new();
        shape_word(
            "\u{0625}\u{0650}\u{0644}\u{0651}\u{064E}\u{0627}",
            true,
            &mut s,
        );
        assert_eq!(s, "\u{FEFB}\u{0651}\u{064E}\u{FE87}\u{0650}");
        // بلا : ba joins the lam -> FINAL ligature (FEFC)
        let mut s = String::new();
        shape_word("\u{0628}\u{0644}\u{0627}", false, &mut s);
        assert_eq!(s, "\u{FEFC}\u{FE91}");
        // لأ / لإ / لآ pick the right variant
        for (alef, iso) in [
            ('\u{0623}', 0xFEF7u32),
            ('\u{0625}', 0xFEF9),
            ('\u{0622}', 0xFEF5),
        ] {
            let mut s = String::new();
            let mut w = String::from("\u{0644}");
            w.push(alef);
            shape_word(&w, false, &mut s);
            assert_eq!(s.chars().next().unwrap() as u32, iso);
        }
        // alef wasla after lam is NOT fused
        let mut s = String::new();
        shape_word("\u{0644}\u{0671}", false, &mut s);
        assert_eq!(s.chars().count(), 2);
    }

    #[test]
    fn tajweed_rules() {
        use super::tj;
        // أَنتُمْ : noon before ta = ikhfa (bare noon)
        let k = analyze_tajweed("\u{0623}\u{064E}\u{0646}\u{062A}\u{064F}\u{0645}\u{0652}");
        assert_eq!(k[0][1].0, tj::IKHFA);
        // يَخْلُقْ : final qaf with sukun = qalqalah
        let k = analyze_tajweed("\u{064A}\u{064E}\u{062E}\u{0652}\u{0644}\u{064F}\u{0642}\u{0652}");
        assert_eq!(k[0][3].0, tj::QALQALAH);
        // إِنَّ : noon with shadda = ghunnah
        let k = analyze_tajweed("\u{0625}\u{0650}\u{0646}\u{0651}\u{064E}");
        assert_eq!(k[0][1].0, tj::GHUNNA);
        // ٱلنَّاسِ : silent lam (shamsiyyah), ghunnah noon, natural madd alef
        let k = analyze_tajweed("\u{0671}\u{0644}\u{0646}\u{0651}\u{064E}\u{0627}\u{0633}\u{0650}");
        assert_eq!(k[0][1].0, tj::SILENT);
        assert_eq!(k[0][2].0, tj::GHUNNA);
        assert_eq!(k[0][3].0, tj::MADD_NATURAL);
        // جَآءَ : connected madd (alef with madda sign before hamza)
        let k = analyze_tajweed("\u{062C}\u{064E}\u{0627}\u{0653}\u{0621}\u{064E}");
        assert_eq!(k[0][1].0, tj::MADD_OBLIGATORY);
        // مِنۢ بَعْدِ : iqlab across two words
        let k = analyze_tajweed(
            "\u{0645}\u{0650}\u{0646}\u{06E2} \u{0628}\u{064E}\u{0639}\u{0652}\u{062F}\u{0650}",
        );
        assert_eq!(k[0][1].0, tj::IQLAB);
        // كَبِيرٌ كَثِيرٌ : tanween before kaf = ikhfa, only the tanween mark is coloured
        let k = analyze_tajweed(
            "\u{0643}\u{064E}\u{0628}\u{0650}\u{064A}\u{0631}\u{064C} \u{0643}\u{064E}",
        );
        assert_eq!(k[0][3], (tj::NONE, tj::IKHFA));
        // مَن يَقُولُ : idgham with ghunnah, the ya is coloured too
        let k = analyze_tajweed("\u{0645}\u{064E}\u{0646} \u{064A}\u{0651}\u{064E}\u{0642}");
        assert_eq!(k[0][1].0, tj::GHUNNA);
        assert_eq!(k[1][0].0, tj::GHUNNA);
    }

    #[test]
    fn tajweed_tags_survive_shaping() {
        let plain = shape_words("\u{0625}\u{0650}\u{0646}\u{0651}\u{064E}", true);
        let tagged = shape_words_ex("\u{0625}\u{0650}\u{0646}\u{0651}\u{064E}", true, true);
        assert!(!plain.chars().any(is_tag));
        assert_eq!(
            tagged.chars().filter(|&c| !is_tag(c)).collect::<String>(),
            plain
        );
        assert!(
            tagged
                .chars()
                .any(|c| decode_tag(c) == Some((super::tj::GHUNNA, false)))
        );
    }

    #[test]
    fn arabic_indic_digits() {
        assert_eq!(to_arabic_indic(7), "\u{0667}");
        assert_eq!(to_arabic_indic(286), "\u{0662}\u{0668}\u{0666}");
    }

    #[test]
    fn case_insensitive_contains() {
        assert!(contains_ci("The Opening", "open"));
        assert!(!contains_ci("The Opening", "cow"));
    }
}
