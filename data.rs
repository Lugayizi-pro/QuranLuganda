use std::collections::HashMap;
use std::sync::OnceLock;

#[derive(Clone, Copy)]
pub struct SurahInfo {
    pub number: u16,
    pub name: &'static str,
    pub translit: &'static str,
    pub english: &'static str,
    pub verses: u16,
    pub arabic: &'static str,
}

#[derive(Clone, Copy)]
pub struct Verse {
    pub surah: u16,
    /// Original ayah number from the data files (0 for the inserted Bismillah entry).
    /// Used for data lookups (translation, tafsir, ruku, juz).
    pub ayah: u16,
    /// Number shown to the reader. In surahs other than Al-Fatihah and At-Tawbah the
    /// Bismillah is verse 1 and every real verse is shifted by one.
    pub num: u16,
    pub arabic: &'static str,
}

/// Where the downloadable translations live: the assets of a GitHub release.
/// The repository is `QuranLuganda`. The release tag is
/// `Translations`; each file below is downloaded from `<this address><file name>`.
pub const TRANSLATIONS_URL: &str =
    "https://github.com/Lugayizi-pro/QuranLuganda/releases/download/Translations/";

/// One translation. Each translation is a plain text file with one
/// `surah|ayah|text` line per verse (same format as `assets/translation.txt`).
///
/// A translation is either bundled inside the app (`raw` is set) or downloaded
/// once from `TRANSLATIONS_URL` (`file` is its file name there).
pub struct TranslationSource {
    /// Stable id saved in the user's settings. Never change it once released.
    pub id: &'static str,
    /// Name shown in the translation picker.
    pub name: &'static str,
    /// Language shown next to the name (may be empty).
    pub language: &'static str,
    /// File name in the GitHub release ("" for a bundled translation).
    pub file: &'static str,
    /// Download size shown on the Download button.
    pub size: &'static str,
    /// Tafsir that goes with this translation: file name in the GitHub release.
    pub tafsir_file: &'static str,
    /// Download size of the tafsir ("" = not shown).
    pub tafsir_size: &'static str,
    raw: Option<&'static str>,
}

impl TranslationSource {
    /// True when the text is inside the app and needs no download.
    pub fn is_bundled(&self) -> bool {
        self.raw.is_some()
    }

    /// Download address of this translation's tafsir.
    pub fn tafsir_url(&self) -> String {
        format!("{}{}", TRANSLATIONS_URL, self.tafsir_file)
    }

    /// Key of this translation's tafsir in `translations_version.txt`.
    pub fn tafsir_key(&self) -> String {
        format!("tafsir_{}", self.id)
    }

    /// Full download address of a downloadable translation.
    pub fn url(&self) -> String {
        format!("{}{}", TRANSLATIONS_URL, self.file)
    }
}

/// All translations the reader can show. The FIRST entry is the default one.
///
/// To add another translation:
///   * bundled: copy its file into `assets/` and use `raw: Some(include_str!(..))`
///   * downloadable: upload the file to the GitHub release and set `file`/`size`
///     with `raw: None`.
pub const TRANSLATIONS: &[TranslationSource] = &[
    TranslationSource {
        id: "main",
        name: "Luganda",
        language: "",
        file: "translation.txt",
        size: "874 KB",
        tafsir_file: "tafsir_main.txt",
        tafsir_size: "",
        raw: None,
    },
    TranslationSource {
        id: "en_sahih",
        name: "English",
        language: "",
        file: "translation_en.txt",
        size: "882 KB",
        tafsir_file: "tafsir_en.txt",
        tafsir_size: "",
        raw: None,
    },
    TranslationSource {
        id: "nyn",
        name: "Runyankole",
        language: "",
        file: "translation_nyn.txt",
        size: "1.4 MB",
        tafsir_file: "tafsir_nyn.txt",
        tafsir_size: "",
        raw: None,
    },
    TranslationSource {
        id: "sw",
        name: "Kiswahili",
        language: "",
        file: "translation_sw.txt",
        size: "782 KB",
        tafsir_file: "tafsir_sw.txt",
        tafsir_size: "",
        raw: None,
    },
];

pub struct QuranDataset {
    pub verses: Vec<Verse>,
    /// `translations[t][global_verse_index]`, same order as `TRANSLATIONS`.
    /// Every list has exactly `verses.len()` entries ("" when a verse is missing).
    pub translations: Vec<Vec<&'static str>>,
    /// `tafsirs[t][global_verse_index]`: the tafsir that goes with translation `t`
    /// (empty until that tafsir has been downloaded).
    pub tafsirs: Vec<Vec<&'static str>>,
    pub surah_offsets: Vec<(u32, u32)>,
}

/// Helper function to cleanly remove leading Basmala from Verse 1 text
fn strip_leading_basmala(text: &str) -> &str {
    let mut letter_byte_indices = Vec::new();
    let mut pure_letters = String::new();

    for (byte_idx, ch) in text.char_indices() {
        if matches!(ch,
            '\u{0621}'..='\u{063A}' | '\u{0641}'..='\u{064A}' | '\u{0671}'..='\u{0680}'
        ) {
            letter_byte_indices.push((byte_idx, ch));
            pure_letters.push(ch);
        }
    }

    let basmala_skeletal_1 = "بسماللهالرحمنالرحيم";
    let basmala_skeletal_2 = "بسمٱللهٱلرحمنٱلرحيم";

    let mut matched_len = 0;
    if pure_letters.starts_with(basmala_skeletal_1) {
        matched_len = basmala_skeletal_1.chars().count();
    } else if pure_letters.starts_with(basmala_skeletal_2) {
        matched_len = basmala_skeletal_2.chars().count();
    } else if pure_letters.starts_with("بسم") && pure_letters.contains("الرحيم") {
        if let Some(pos) = pure_letters.find("الرحيم") {
            matched_len = pure_letters[..pos + "الرحيم".len()].chars().count();
        }
    } else if pure_letters.starts_with("بسم") && pure_letters.contains("ٱلرحيم") {
        if let Some(pos) = pure_letters.find("ٱلرحيم") {
            matched_len = pure_letters[..pos + "ٱلرحيم".len()].chars().count();
        }
    }

    if matched_len > 0 && matched_len <= letter_byte_indices.len() {
        let last_letter_byte_idx = if matched_len < letter_byte_indices.len() {
            letter_byte_indices[matched_len].0
        } else {
            text.len()
        };
        return text[last_letter_byte_idx..].trim();
    }

    text
}

/// Robust `surah|ayah|text` line parser. Splits only on the first two `|`
/// so any `|` inside the body is preserved verbatim.
fn parse_sa_text(line: &str) -> Option<(u16, u16, &str)> {
    let mut it = line.splitn(3, '|');
    let s = it.next()?.trim().parse::<u16>().ok()?;
    let a = it.next()?.trim().parse::<u16>().ok()?;
    let t = it.next()?.trim();
    Some((s, a, t))
}

/// Builds the aligned list `[global verse index] -> text` of one translation file
/// (`""` when that translation lacks the verse).
fn build_translation(verses: &[Verse], raw: &'static str) -> Vec<&'static str> {
    let mut map: HashMap<(u16, u16), &'static str> = HashMap::new();
    for line in raw.lines() {
        if line.trim().is_empty() {
            continue;
        }
        if let Some((s, a, t)) = parse_sa_text(line) {
            map.insert((s, a), t);
        }
    }
    verses
        .iter()
        .map(|v| {
            // The inserted Bismillah entry uses the translation of 1:1.
            let key = if v.ayah == 0 { (1, 1) } else { (v.surah, v.ayah) };
            map.get(&key).copied().unwrap_or("")
        })
        .collect()
}

impl QuranDataset {
    pub fn load_embedded() -> Self {
        let mut verses = Vec::new();
        let mut surah_offsets = Vec::new();

        let ar_raw = include_str!("../assets/quran-uthmani.txt");

        let mut current_surah = 0u16;
        let mut start_idx = 0u32;
        // Text of 1:1 (the Bismillah), inserted as verse 1 at the top of every other surah.
        let mut bismillah_text: &'static str = "";

        for line in ar_raw.lines() {
            let Some((s, a, raw_text)) = parse_sa_text(line) else {
                continue;
            };

            let mut arabic_text = raw_text;

            if s != 1 && s != 9 && a == 1 {
                arabic_text = strip_leading_basmala(arabic_text);
            }

            if s != current_surah {
                if current_surah != 0 {
                    surah_offsets.push((start_idx, verses.len() as u32));
                }
                current_surah = s;
                start_idx = verses.len() as u32;
            }

            if s == 1 && a == 1 {
                bismillah_text = arabic_text;
            }
            let has_bismillah = s != 1 && s != 9;
            if has_bismillah && a == 1 {
                verses.push(Verse {
                    surah: s,
                    ayah: 0,
                    num: 1,
                    arabic: bismillah_text,
                });
            }

            verses.push(Verse {
                surah: s,
                ayah: a,
                num: if has_bismillah { a + 1 } else { a },
                arabic: arabic_text,
            });
        }
        if !verses.is_empty() {
            surah_offsets.push((start_idx, verses.len() as u32));
        }

        // One aligned list per translation: translations[t][global] is the text
        // of verse `global` ("" when that translation lacks the verse).
        let translations: Vec<Vec<&'static str>> = TRANSLATIONS
            .iter()
            .map(|src| match src.raw {
                Some(raw) => build_translation(&verses, raw),
                // Downloadable: empty until the file has been downloaded.
                None => vec![""; verses.len()],
            })
            .collect();

        let tafsirs: Vec<Vec<&'static str>> = TRANSLATIONS
            .iter()
            .map(|_| vec![""; verses.len()])
            .collect();

        Self {
            verses,
            translations,
            tafsirs,
            surah_offsets,
        }
    }

    /// Installs the text of a downloaded tafsir (same line format as a translation).
    /// Returns false (and changes nothing) when it does not look like a tafsir file.
    pub fn set_tafsir_text(&mut self, t: usize, text: String) -> bool {
        if t >= self.tafsirs.len() {
            return false;
        }
        let raw: &'static str = Box::leak(text.into_boxed_str());
        let list = build_translation(&self.verses, raw);
        if list.iter().filter(|s| !s.is_empty()).count() < 100 {
            return false;
        }
        self.tafsirs[t] = list;
        true
    }

    /// Tafsir of verse `global` for translation `t` ("" if unavailable).
    pub fn tafsir(&self, t: usize, global: usize) -> &'static str {
        self.tafsirs
            .get(t)
            .and_then(|list| list.get(global))
            .copied()
            .unwrap_or("")
    }

    /// Installs the text of a downloaded translation. Returns false (and changes
    /// nothing) when the text does not look like a translation file.
    pub fn set_translation_text(&mut self, t: usize, text: String) -> bool {
        if t >= self.translations.len() {
            return false;
        }
        // Lives until the app closes, like the bundled translations.
        let raw: &'static str = Box::leak(text.into_boxed_str());
        let list = build_translation(&self.verses, raw);
        if list.iter().filter(|s| !s.is_empty()).count() < 100 {
            return false;
        }
        self.translations[t] = list;
        true
    }

    /// Text of verse `global` in translation `t` ("" if unavailable).
    pub fn translation(&self, t: usize, global: usize) -> &'static str {
        self.translations
            .get(t)
            .and_then(|list| list.get(global))
            .copied()
            .unwrap_or("")
    }

    /// Translation texts of surah index `s` (0-based), aligned with `surah_verses(s)`.
    pub fn surah_translations(&self, t: usize, s: usize) -> &[&'static str] {
        let (a, b) = self.surah_range(s);
        let Some(list) = self.translations.get(t) else {
            return &[];
        };
        list.get(a as usize..b as usize).unwrap_or(&[])
    }

    pub fn surah_range(&self, s: usize) -> (u32, u32) {
        if s < self.surah_offsets.len() {
            self.surah_offsets[s]
        } else {
            (0, 0)
        }
    }

    pub fn surah_verses(&self, s: usize) -> &[Verse] {
        let (a, b) = self.surah_range(s);
        if (a as usize) <= self.verses.len() && (b as usize) <= self.verses.len() && a <= b {
            &self.verses[a as usize..b as usize]
        } else {
            &[]
        }
    }

    /// Finds a verse by its ORIGINAL ayah number (used for juz starts). Ayah 1 returns
    /// the Bismillah entry above it, so a jump lands at the top of the surah.
    pub fn find(&self, surah: u16, ayah: u16) -> Option<u32> {
        let i = self
            .verses
            .iter()
            .position(|v| v.surah == surah && v.ayah == ayah)?;
        if ayah == 1 && i > 0 && self.verses[i - 1].surah == surah && self.verses[i - 1].ayah == 0 {
            return Some(i as u32 - 1);
        }
        Some(i as u32)
    }

    /// Finds a verse by the number shown to the reader (what people type, like 2:255).
    pub fn find_num(&self, surah: u16, num: u16) -> Option<u32> {
        self.verses
            .iter()
            .position(|v| v.surah == surah && v.num == num)
            .map(|i| i as u32)
    }

    /// Converts a verse index saved before the Bismillah entries existed to the
    /// current index, so old bookmarks and the last read verse still point to the
    /// same verse.
    pub fn migrate_old_index(&self, old: u32) -> u32 {
        let mut shift = 0u32;
        for s in 0..self.surah_offsets.len() {
            let (a, b) = self.surah_offsets[s];
            let has = (a as usize) < self.verses.len() && self.verses[a as usize].ayah == 0;
            let extra = has as u32;
            let old_start = a - shift;
            let old_len = (b - a) - extra;
            if old < old_start + old_len {
                return a + extra + (old - old_start);
            }
            shift += extra;
        }
        old
    }
}

pub const SURAHS: &[SurahInfo] = &[
    SurahInfo {
	        number: 1,
	        name: "Al-Fatihah",
	        translit: "Al-Fatihah",
	        english: "The Opening",
	        verses: 7,
	        arabic: "الفاتحة",
	    },
    SurahInfo {
	        number: 2,
	        name: "Al-Baqarah",
	        translit: "Al-Baqarah",
	        english: "The Cow",
	        verses: 286,
	        arabic: "البقرة",
	    },
    SurahInfo {
	        number: 3,
	        name: "Ali 'Imran",
	        translit: "Ali 'Imran",
	        english: "Family of Imran",
	        verses: 200,
	        arabic: "آل عمران",
	    },
    SurahInfo {
	        number: 4,
	        name: "An-Nisa",
	        translit: "An-Nisa",
	        english: "The Women",
	        verses: 176,
	        arabic: "النساء",
	    },
    SurahInfo {
	        number: 5,
	        name: "Al-Ma'idah",
	        translit: "Al-Ma'idah",
	        english: "The Table Spread",
	        verses: 120,
	        arabic: "المائدة",
	    },
    SurahInfo {
	        number: 6,
	        name: "Al-An'am",
	        translit: "Al-An'am",
	        english: "The Cattle",
	        verses: 165,
	        arabic: "الأنعام",
	    },
    SurahInfo {
	        number: 7,
	        name: "Al-A'raf",
	        translit: "Al-A'raf",
	        english: "The Heights",
	        verses: 206,
	        arabic: "الأعراف",
	    },
    SurahInfo {
	        number: 8,
	        name: "Al-Anfal",
	        translit: "Al-Anfal",
	        english: "The Spoils of War",
	        verses: 75,
	        arabic: "الأنفال",
	    },
    SurahInfo {
	        number: 9,
	        name: "At-Tawbah",
	        translit: "At-Tawbah",
	        english: "The Repentance",
	        verses: 129,
	        arabic: "التوبة",
	    },
    SurahInfo {
	        number: 10,
	        name: "Yunus",
	        translit: "Yunus",
	        english: "Jonah",
	        verses: 109,
	        arabic: "يونس",
	    },
    SurahInfo {
	        number: 11,
	        name: "Hud",
	        translit: "Hud",
	        english: "Hud",
	        verses: 123,
	        arabic: "هود",
	    },
    SurahInfo {
	        number: 12,
	        name: "Yusuf",
	        translit: "Yusuf",
	        english: "Joseph",
	        verses: 111,
	        arabic: "يوسف",
	    },
    SurahInfo {
	        number: 13,
	        name: "Ar-Ra'd",
	        translit: "Ar-Ra'd",
	        english: "The Thunder",
	        verses: 43,
	        arabic: "الرعد",
	    },
    SurahInfo {
	        number: 14,
	        name: "Ibrahim",
	        translit: "Ibrahim",
	        english: "Abraham",
	        verses: 52,
	        arabic: "إبراهيم",
	    },
    SurahInfo {
	        number: 15,
	        name: "Al-Hijr",
	        translit: "Al-Hijr",
	        english: "The Rocky Tract",
	        verses: 99,
	        arabic: "الحجر",
	    },
    SurahInfo {
	        number: 16,
	        name: "An-Nahl",
	        translit: "An-Nahl",
	        english: "The Bee",
	        verses: 128,
	        arabic: "النحل",
	    },
    SurahInfo {
	        number: 17,
	        name: "Al-Isra",
	        translit: "Al-Isra",
	        english: "The Night Journey",
	        verses: 111,
	        arabic: "الإسراء",
	    },
    SurahInfo {
	        number: 18,
	        name: "Al-Kahf",
	        translit: "Al-Kahf",
	        english: "The Cave",
	        verses: 110,
	        arabic: "الكهف",
	    },
    SurahInfo {
	        number: 19,
	        name: "Maryam",
	        translit: "Maryam",
	        english: "Mary",
	        verses: 98,
	        arabic: "مريم",
	    },
    SurahInfo {
	        number: 20,
	        name: "Ta-Ha",
	        translit: "Ta-Ha",
	        english: "Ta-Ha",
	        verses: 135,
	        arabic: "طه",
	    },
    SurahInfo {
	        number: 21,
	        name: "Al-Anbiya",
	        translit: "Al-Anbiya",
	        english: "The Prophets",
	        verses: 112,
	        arabic: "الأنبياء",
	    },
    SurahInfo {
	        number: 22,
	        name: "Al-Hajj",
	        translit: "Al-Hajj",
	        english: "The Pilgrimage",
	        verses: 78,
	        arabic: "الحج",
	    },
    SurahInfo {
	        number: 23,
	        name: "Al-Mu'minun",
	        translit: "Al-Mu'minun",
	        english: "The Believers",
	        verses: 118,
	        arabic: "المؤمنون",
	    },
    SurahInfo {
	        number: 24,
	        name: "An-Nur",
	        translit: "An-Nur",
	        english: "The Light",
	        verses: 64,
	        arabic: "النور",
	    },
    SurahInfo {
	        number: 25,
	        name: "Al-Furqan",
	        translit: "Al-Furqan",
	        english: "The Criterion",
	        verses: 77,
	        arabic: "الفرقان",
	    },
    SurahInfo {
	        number: 26,
	        name: "Ash-Shu'ara",
	        translit: "Ash-Shu'ara",
	        english: "The Poets",
	        verses: 227,
	        arabic: "الشعراء",
	    },
    SurahInfo {
	        number: 27,
	        name: "An-Naml",
	        translit: "An-Naml",
	        english: "The Ant",
	        verses: 93,
	        arabic: "النمل",
	    },
    SurahInfo {
	        number: 28,
	        name: "Al-Qasas",
	        translit: "Al-Qasas",
	        english: "The Stories",
	        verses: 88,
	        arabic: "القصص",
	    },
    SurahInfo {
	        number: 29,
	        name: "Al-Ankabut",
	        translit: "Al-Ankabut",
	        english: "The Spider",
	        verses: 69,
	        arabic: "العنكبوت",
	    },
    SurahInfo {
	        number: 30,
	        name: "Ar-Rum",
	        translit: "Ar-Rum",
	        english: "The Romans",
	        verses: 60,
	        arabic: "الروم",
	    },
    SurahInfo {
	        number: 31,
	        name: "Luqman",
	        translit: "Luqman",
	        english: "Luqman",
	        verses: 34,
	        arabic: "لقمان",
	    },
    SurahInfo {
	        number: 32,
	        name: "As-Sajdah",
	        translit: "As-Sajdah",
	        english: "The Prostration",
	        verses: 30,
	        arabic: "السجدة",
	    },
    SurahInfo {
	        number: 33,
	        name: "Al-Ahzab",
	        translit: "Al-Ahzab",
	        english: "The Combined Forces",
	        verses: 73,
	        arabic: "الأحزاب",
	    },
    SurahInfo {
	        number: 34,
	        name: "Saba",
	        translit: "Saba",
	        english: "Sheba",
	        verses: 54,
	        arabic: "سبأ",
	    },
    SurahInfo {
	        number: 35,
	        name: "Fatir",
	        translit: "Fatir",
	        english: "Originator",
	        verses: 45,
	        arabic: "فاطر",
	    },
    SurahInfo {
	        number: 36,
	        name: "Ya-Sin",
	        translit: "Ya-Sin",
	        english: "Ya-Sin",
	        verses: 83,
	        arabic: "يس",
	    },
    SurahInfo {
	        number: 37,
	        name: "As-Saffat",
	        translit: "As-Saffat",
	        english: "Those Ranged in Ranks",
	        verses: 182,
	        arabic: "الصافات",
	    },
    SurahInfo {
	        number: 38,
	        name: "Sad",
	        translit: "Sad",
	        english: "The Letter Sad",
	        verses: 88,
	        arabic: "ص",
	    },
    SurahInfo {
	        number: 39,
	        name: "Az-Zumar",
	        translit: "Az-Zumar",
	        english: "The Troops",
	        verses: 75,
	        arabic: "الزمر",
	    },
    SurahInfo {
	        number: 40,
	        name: "Ghafir",
	        translit: "Ghafir",
	        english: "The Forgiver",
	        verses: 85,
	        arabic: "غافر",
	    },
    SurahInfo {
	        number: 41,
	        name: "Fussilat",
	        translit: "Fussilat",
	        english: "Explained in Detail",
	        verses: 54,
	        arabic: "فصلت",
	    },
    SurahInfo {
	        number: 42,
	        name: "Ash-Shura",
	        translit: "Ash-Shura",
	        english: "The Consultation",
	        verses: 53,
	        arabic: "الشورى",
	    },
    SurahInfo {
	        number: 43,
	        name: "Az-Zukhruf",
	        translit: "Az-Zukhruf",
	        english: "The Gold Ornaments",
	        verses: 89,
	        arabic: "الزخرف",
	    },
    SurahInfo {
	        number: 44,
	        name: "Ad-Dukhan",
	        translit: "Ad-Dukhan",
	        english: "The Smoke",
	        verses: 59,
	        arabic: "الدخان",
	    },
    SurahInfo {
	        number: 45,
	        name: "Al-Jathiyah",
	        translit: "Al-Jathiyah",
	        english: "The Crouching",
	        verses: 37,
	        arabic: "الجاثية",
	    },
    SurahInfo {
	        number: 46,
	        name: "Al-Ahqaf",
	        translit: "Al-Ahqaf",
	        english: "The Curved Sand-hills",
	        verses: 35,
	        arabic: "الأحقاف",
	    },
    SurahInfo {
	        number: 47,
	        name: "Muhammad",
	        translit: "Muhammad",
	        english: "Muhammad",
	        verses: 38,
	        arabic: "محمد",
	    },
    SurahInfo {
	        number: 48,
	        name: "Al-Fath",
	        translit: "Al-Fath",
	        english: "The Victory",
	        verses: 29,
	        arabic: "الفتح",
	    },
    SurahInfo {
	        number: 49,
	        name: "Al-Hujurat",
	        translit: "Al-Hujurat",
	        english: "The Dwellings",
	        verses: 18,
	        arabic: "الحجرات",
	    },
    SurahInfo {
	        number: 50,
	        name: "Qaf",
	        translit: "Qaf",
	        english: "The Letter Qaf",
	        verses: 45,
	        arabic: "ق",
	    },
    SurahInfo {
	        number: 51,
	        name: "Adh-Dhariyat",
	        translit: "Adh-Dhariyat",
	        english: "The Winnowing Winds",
	        verses: 60,
	        arabic: "الذاريات",
	    },
    SurahInfo {
	        number: 52,
	        name: "At-Tur",
	        translit: "At-Tur",
	        english: "The Mount",
	        verses: 49,
	        arabic: "الطور",
	    },
    SurahInfo {
	        number: 53,
	        name: "An-Najm",
	        translit: "An-Najm",
	        english: "The Star",
	        verses: 62,
	        arabic: "النجم",
	    },
    SurahInfo {
	        number: 54,
	        name: "Al-Qamar",
	        translit: "Al-Qamar",
	        english: "The Moon",
	        verses: 55,
	        arabic: "القمر",
	    },
    SurahInfo {
	        number: 55,
	        name: "Ar-Rahman",
	        translit: "Ar-Rahman",
	        english: "The Beneficent",
	        verses: 78,
	        arabic: "الرحمن",
	    },
    SurahInfo {
	        number: 56,
	        name: "Al-Waqi'ah",
	        translit: "Al-Waqi'ah",
	        english: "The Inevitable",
	        verses: 96,
	        arabic: "واقعة",
	    },
    SurahInfo {
	        number: 57,
	        name: "Al-Hadid",
	        translit: "Al-Hadid",
	        english: "The Iron",
	        verses: 29,
	        arabic: "الحديد",
	    },
    SurahInfo {
	        number: 58,
	        name: "Al-Mujadila",
	        translit: "Al-Mujadila",
	        english: "The Pleading Woman",
	        verses: 22,
	        arabic: "المجادلة",
	    },
    SurahInfo {
	        number: 59,
	        name: "Al-Hashr",
	        translit: "Al-Hashr",
	        english: "The Exile",
	        verses: 24,
	        arabic: "الحشر",
	    },
    SurahInfo {
	        number: 60,
	        name: "Al-Mumtahanah",
	        translit: "Al-Mumtahanah",
	        english: "She that is to be examined",
	        verses: 13,
	        arabic: "الممتحنة",
	    },
    SurahInfo {
	        number: 61,
	        name: "As-Saff",
	        translit: "As-Saff",
	        english: "The Ranks",
	        verses: 14,
	        arabic: "الصف",
	    },
    SurahInfo {
	        number: 62,
	        name: "Al-Jumu'ah",
	        translit: "Al-Jumu'ah",
	        english: "Friday",
	        verses: 11,
	        arabic: "الجمعة",
	    },
    SurahInfo {
	        number: 63,
	        name: "Al-Munafiqun",
	        translit: "Al-Munafiqun",
	        english: "The Hypocrites",
	        verses: 11,
	        arabic: "المنافقون",
	    },
    SurahInfo {
	        number: 64,
	        name: "At-Taghabun",
	        translit: "At-Taghabun",
	        english: "Mutual Disillusion",
	        verses: 18,
	        arabic: "التغابن",
	    },
    SurahInfo {
	        number: 65,
	        name: "At-Talaq",
	        translit: "At-Talaq",
	        english: "Divorce",
	        verses: 12,
	        arabic: "الطلاق",
	    },
    SurahInfo {
	        number: 66,
	        name: "At-Tahrim",
	        translit: "At-Tahrim",
	        english: "The Prohibition",
	        verses: 12,
	        arabic: "التحريم",
	    },
    SurahInfo {
	        number: 67,
	        name: "Al-Mulk",
	        translit: "Al-Mulk",
	        english: "The Sovereignty",
	        verses: 30,
	        arabic: "الملك",
	    },
    SurahInfo {
	        number: 68,
	        name: "Al-Qalam",
	        translit: "Al-Qalam",
	        english: "The Pen",
	        verses: 52,
	        arabic: "القلم",
	    },
    SurahInfo {
	        number: 69,
	        name: "Al-Haqqah",
	        translit: "Al-Haqqah",
	        english: "The Reality",
	        verses: 52,
	        arabic: "الحاقة",
	    },
    SurahInfo {
	        number: 70,
	        name: "Al-Ma'arij",
	        translit: "Al-Ma'arij",
	        english: "The Ascending Stairways",
	        verses: 44,
	        arabic: "المعارج",
	    },
    SurahInfo {
	        number: 71,
	        name: "Nuh",
	        translit: "Nuh",
	        english: "Noah",
	        verses: 28,
	        arabic: "نوح",
	    },
    SurahInfo {
	        number: 72,
	        name: "Al-Jinn",
	        translit: "Al-Jinn",
	        english: "The Jinn",
	        verses: 28,
	        arabic: "الجن",
	    },
    SurahInfo {
	        number: 73,
	        name: "Al-Muzzammil",
	        translit: "Al-Muzzammil",
	        english: "The Enshrouded One",
	        verses: 20,
	        arabic: "المزمل",
	    },
    SurahInfo {
	        number: 74,
	        name: "Al-Muddaththir",
	        translit: "Al-Muddaththir",
	        english: "The Cloaked One",
	        verses: 56,
	        arabic: "المدثر",
	    },
    SurahInfo {
	        number: 75,
	        name: "Al-Qiyamah",
	        translit: "Al-Qiyamah",
	        english: "The Resurrection",
	        verses: 40,
	        arabic: "القيامة",
	    },
    SurahInfo {
	        number: 76,
	        name: "Al-Insan",
	        translit: "Al-Insan",
	        english: "Man",
	        verses: 31,
	        arabic: "الانسان",
	    },
    SurahInfo {
	        number: 77,
	        name: "Al-Mursalat",
	        translit: "Al-Mursalat",
	        english: "The Emissaries",
	        verses: 50,
	        arabic: "المرسلات",
	    },
    SurahInfo {
	        number: 78,
	        name: "An-Naba",
	        translit: "An-Naba",
	        english: "The Announcement",
	        verses: 40,
	        arabic: "النبأ",
	    },
    SurahInfo {
	        number: 79,
	        name: "An-Nazi'at",
	        translit: "An-Nazi'at",
	        english: "Those who drag forth",
	        verses: 46,
	        arabic: "النازعات",
	    },
    SurahInfo {
	        number: 80,
	        name: "Abasa",
	        translit: "Abasa",
	        english: "He Frowned",
	        verses: 42,
	        arabic: "عبس",
	    },
    SurahInfo {
	        number: 81,
	        name: "At-Takwir",
	        translit: "At-Takwir",
	        english: "The Overthrowing",
	        verses: 29,
	        arabic: "التكوير",
	    },
    SurahInfo {
	        number: 82,
	        name: "Al-Infitar",
	        translit: "Al-Infitar",
	        english: "The Cleaving",
	        verses: 19,
	        arabic: "الانفطار",
	    },
    SurahInfo {
	        number: 83,
	        name: "Al-Mutaffifin",
	        translit: "Al-Mutaffifin",
	        english: "Defaulting in Duty",
	        verses: 36,
	        arabic: "المطففين",
	    },
    SurahInfo {
	        number: 84,
	        name: "Al-Inshiqaq",
	        translit: "Al-Inshiqaq",
	        english: "The Splitting Open",
	        verses: 25,
	        arabic: "الانشقاق",
	    },
    SurahInfo {
	        number: 85,
	        name: "Al-Burooj",
	        translit: "Al-Burooj",
	        english: "The Mansions of the Stars",
	        verses: 22,
	        arabic: "البروج",
	    },
    SurahInfo {
	        number: 86,
	        name: "At-Tariq",
	        translit: "At-Tariq",
	        english: "The Morning Star",
	        verses: 17,
	        arabic: "الطارق",
	    },
    SurahInfo {
	        number: 87,
	        name: "Al-A'la",
	        translit: "Al-A'la",
	        english: "The Most High",
	        verses: 19,
	        arabic: "الأعلى",
	    },
    SurahInfo {
	        number: 88,
	        name: "Al-Ghashiyah",
	        translit: "Al-Ghashiyah",
	        english: "The Overwhelming",
	        verses: 26,
	        arabic: "الغاشية",
	    },
    SurahInfo {
	        number: 89,
	        name: "Al-Fajr",
	        translit: "Al-Fajr",
	        english: "The Dawn",
	        verses: 30,
	        arabic: "الفجر",
	    },
    SurahInfo {
	        number: 90,
	        name: "Al-Balad",
	        translit: "Al-Balad",
	        english: "The City",
	        verses: 20,
	        arabic: "البلد",
	    },
    SurahInfo {
	        number: 91,
	        name: "Ash-Shams",
	        translit: "Ash-Shams",
	        english: "The Sun",
	        verses: 15,
	        arabic: "الشمس",
	    },
    SurahInfo {
	        number: 92,
	        name: "Al-Layl",
	        translit: "Al-Layl",
	        english: "The Night",
	        verses: 21,
	        arabic: "الليل",
	    },
    SurahInfo {
	        number: 93,
	        name: "Ad-Duha",
	        translit: "Ad-Duha",
	        english: "The Morning Hours",
	        verses: 11,
	        arabic: "الضحى",
	    },
    SurahInfo {
	        number: 94,
	        name: "Ash-Sharh",
	        translit: "Ash-Sharh",
	        english: "The Consolation",
	        verses: 8,
	        arabic: "الشرح",
	    },
    SurahInfo {
	        number: 95,
	        name: "At-Tin",
	        translit: "At-Tin",
	        english: "The Fig",
	        verses: 8,
	        arabic: "التين",
	    },
    SurahInfo {
	        number: 96,
	        name: "Al-'Alaq",
	        translit: "Al-'Alaq",
	        english: "The Clot",
	        verses: 19,
	        arabic: "العلق",
	    },
    SurahInfo {
	        number: 97,
	        name: "Al-Qadr",
	        translit: "Al-Qadr",
	        english: "The Power",
	        verses: 5,
	        arabic: "القدر",
	    },
    SurahInfo {
	        number: 98,
	        name: "Al-Bayyinah",
	        translit: "Al-Bayyinah",
	        english: "The Clear Proof",
	        verses: 8,
	        arabic: "البينة",
	    },
    SurahInfo {
	        number: 99,
	        name: "Az-Zalzalah",
	        translit: "Az-Zalzalah",
	        english: "The Earthquake",
	        verses: 8,
	        arabic: "الزلزلة",
	    },
    SurahInfo {
	        number: 100,
	        name: "Al-'Adiyat",
	        translit: "Al-'Adiyat",
	        english: "The Courser",
	        verses: 11,
	        arabic: "العاديات",
	    },
    SurahInfo {
	        number: 101,
	        name: "Al-Qari'ah",
	        translit: "Al-Qari'ah",
	        english: "The Striking Hour",
	        verses: 11,
	        arabic: "القارعة",
	    },
    SurahInfo {
	        number: 102,
	        name: "At-Takathur",
	        translit: "At-Takathur",
	        english: "Rivalry in World Increase",
	        verses: 8,
	        arabic: "التكاثر",
	    },
    SurahInfo {
	        number: 103,
	        name: "Al-Asr",
	        translit: "Al-Asr",
	        english: "The Declining Day",
	        verses: 3,
	        arabic: "العصر",
	    },
    SurahInfo {
	        number: 104,
	        name: "Al-Humazah",
	        translit: "Al-Humazah",
	        english: "The Traducer",
	        verses: 9,
	        arabic: "الهمزة",
	    },
    SurahInfo {
	        number: 105,
	        name: "Al-Fil",
	        translit: "Al-Fil",
	        english: "The Elephant",
	        verses: 5,
	        arabic: "الفيل",
	    },
    SurahInfo {
	        number: 106,
	        name: "Quraish",
	        translit: "Quraish",
	        english: "Quraish",
	        verses: 4,
	        arabic: "قريش",
	    },
    SurahInfo {
	        number: 107,
	        name: "Al-Ma'un",
	        translit: "Al-Ma'un",
	        english: "The Small Kindnesses",
	        verses: 7,
	        arabic: "الماعون",
	    },
    SurahInfo {
	        number: 108,
	        name: "Al-Kawthar",
	        translit: "Al-Kawthar",
	        english: "Abundance",
	        verses: 3,
	        arabic: "الكوثر",
	    },
    SurahInfo {
	        number: 109,
	        name: "Al-Kafirun",
	        translit: "Al-Kafirun",
	        english: "The Disbelievers",
	        verses: 6,
	        arabic: "الكافرون",
	    },
    SurahInfo {
	        number: 110,
	        name: "An-Nasr",
	        translit: "An-Nasr",
	        english: "The Divine Support",
	        verses: 3,
	        arabic: "النصر",
	    },
    SurahInfo {
	        number: 111,
	        name: "Al-Masad",
	        translit: "Al-Masad",
	        english: "The Palm Fibre",
	        verses: 5,
	        arabic: "المسد",
	    },
    SurahInfo {
	        number: 112,
	        name: "Al-Ikhlas",
	        translit: "Al-Ikhlas",
	        english: "The Sincerity",
	        verses: 4,
	        arabic: "الإخلاص",
	    },
    SurahInfo {
	        number: 113,
	        name: "Al-Falaq",
	        translit: "Al-Falaq",
	        english: "The Daybreak",
	        verses: 5,
	        arabic: "الفلق",
	    },
    SurahInfo {
	        number: 114,
	        name: "An-Nas",
	        translit: "An-Nas",
	        english: "Mankind",
	        verses: 6,
	        arabic: "الناس",
	    },
];

pub const JUZ_STARTS: &[(u16, u16)] = &[
    (1, 1),
	    (2, 142),
	    (3, 253),
	    (4, 93),
	    (5, 24),
	    (6, 83),
	    (7, 83),
	    (8, 93),
	    (9, 41),
	    (10, 93),
    (11, 6),
	    (12, 53),
	    (13, 53),
	    (14, 1),
	    (15, 1),
	    (16, 1),
	    (17, 1),
	    (18, 1),
	    (19, 21),
	    (20, 1),
    (21, 1),
	    (22, 1),
	    (23, 1),
	    (24, 32),
	    (25, 21),
	    (26, 1),
	    (27, 1),
	    (28, 1),
	    (29, 1),
	    (30, 1),
];

pub fn juz_of(surah: u16, ayah: u16) -> u8 {
    let mut found = 1u8;
    for (i, &(js, ja)) in JUZ_STARTS.iter().enumerate() {
        if surah > js || (surah == js && ayah >= ja) {
            found = (i + 1) as u8;
        }
    }
    found
}

pub fn juz_start(juz: u8) -> (u16, u16) {
    JUZ_STARTS.get(juz as usize - 1).copied().unwrap_or((1, 1))
}

// --- Ruku Loading & Lookup ---

static RUKUS_MAP: OnceLock<HashMap<(u16, u16), u16>> = OnceLock::new();

pub fn load_rukus_embedded() -> HashMap<(u16, u16), u16> {
    let mut map = HashMap::new();
    let content = include_str!("../assets/rukus.txt");
    for line in content.lines() {
        let parts: Vec<&str> = line.split('|').collect();
        if parts.len() == 3 {
            if let (Ok(s), Ok(a), Ok(r)) = (parts[0].parse(), parts[1].parse(), parts[2].parse()) {
                map.insert((s, a), r);
            }
        }
    }
    map
}

/// Returns the Ruku number if this (surah, ayah) starts a new Ruku section from rukus.txt.
pub fn get_ruku_number(surah: u16, ayah: u16) -> Option<u16> {
    let map = RUKUS_MAP.get_or_init(load_rukus_embedded);
    map.get(&(surah, ayah)).copied()
}
