"""
Builds the word-by-word file `wbw_en.txt` for the app. Run it ONCE, on a computer or in
Google Colab (colab.research.google.com), then upload the file to your GitHub release.
The app itself never talks to this source: it only downloads your file.

Result: one line per verse,   surah|ayah|meaning1;meaning2;meaning3
(one meaning per Arabic word, in reading order, separated by ;)

Steps:  python make_wbw.py
It saves as it goes, so if it stops you can run it again and it carries on.
Optional: put the app's `quran-uthmani.txt` next to this script and it will also write
`wbw_mismatch.txt`, a list of verses whose word count differs from the app's Arabic text.
"""
import json, os, sys, time, urllib.request

OUT = "wbw_en.txt"
LANG = "en"            # language of the meanings
ARABIC = "quran-uthmani.txt"   # optional, for the word-count check

AYAH = [7,286,200,176,120,165,206,75,129,109,123,111,43,52,99,128,111,110,98,135,112,78,118,64,77,227,93,88,69,60,34,30,73,54,45,83,182,88,75,85,54,53,89,59,37,35,38,29,18,45,60,49,62,55,78,96,29,22,24,13,14,11,11,18,12,12,30,52,52,44,28,28,20,56,40,31,50,40,46,42,29,19,36,25,22,17,19,26,30,20,15,21,11,8,8,19,5,8,8,11,11,8,3,9,5,4,7,3,6,3,5,4,5,6]


def url(s, a):
    return ("https://api.quran.com/api/v4/verses/by_key/%d:%d?language=%s&words=true"
            % (s, a, LANG))


def meanings(data):
    """Short meaning of every word of a verse (the verse-end marker is left out)."""
    verse = data.get("verse") or (data.get("verses") or [{}])[0]
    words = verse.get("words") or []
    out = []
    for w in words:
        if w.get("char_type_name", "word") != "word":
            continue
        t = ((w.get("translation") or {}).get("text") or "").strip()
        out.append(t.replace(";", ",").replace("|", "/").replace("\n", " "))
    return out


def fetch(s, a):
    for attempt in range(5):
        try:
            with urllib.request.urlopen(url(s, a), timeout=30) as r:
                return json.load(r)
        except Exception as e:  # network hiccup: wait and try again
            print("  retry", s, a, e)
            time.sleep(2 + attempt * 3)
    raise SystemExit("Could not fetch %d:%d, run the script again later." % (s, a))


def main():
    done = set()
    if os.path.exists(OUT):
        for line in open(OUT, encoding="utf-8"):
            p = line.split("|", 2)
            if len(p) == 3:
                done.add((int(p[0]), int(p[1])))
    with open(OUT, "a", encoding="utf-8") as f:
        for s, n in enumerate(AYAH, 1):
            for a in range(1, n + 1):
                if (s, a) in done:
                    continue
                m = meanings(fetch(s, a))
                f.write("%d|%d|%s\n" % (s, a, ";".join(m)))
                f.flush()
                time.sleep(0.15)  # be gentle with the server
            print("surah", s, "done")
    check()


def is_mark(c):
    o = ord(c)
    return (0x610 <= o <= 0x61A or 0x64B <= o <= 0x65F or o == 0x670 or 0x6D6 <= o <= 0x6DC
            or 0x6DF <= o <= 0x6E8 or 0x6EA <= o <= 0x6ED or 0x8D3 <= o <= 0x8FF)


def app_words(text):
    """Words the way the app counts them (a token made only of marks is not a word)."""
    return [t for t in text.split() if any(not is_mark(c) for c in t)]


def check():
    if not os.path.exists(ARABIC):
        print("(no %s next to the script: word-count check skipped)" % ARABIC)
        return
    mine = {}
    for line in open(OUT, encoding="utf-8"):
        p = line.rstrip("\n").split("|", 2)
        if len(p) == 3:
            mine[(int(p[0]), int(p[1]))] = len(p[2].split(";")) if p[2] else 0
    bad = []
    for line in open(ARABIC, encoding="utf-8"):
        p = line.rstrip("\n").split("|", 2)
        if len(p) != 3:
            continue
        try:
            key = (int(p[0]), int(p[1]))
        except ValueError:
            continue
        n = len(app_words(p[2]))
        if key in mine and mine[key] != n:
            bad.append("%d:%d  app has %d words, file has %d" % (key[0], key[1], n, mine[key]))
    with open("wbw_mismatch.txt", "w", encoding="utf-8") as f:
        f.write("\n".join(bad))
    print("Verses whose word count differs:", len(bad), "(see wbw_mismatch.txt)")


if __name__ == "__main__":
    main()
