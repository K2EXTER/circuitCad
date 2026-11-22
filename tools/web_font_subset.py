"""Make the Japanese font of the browser version smaller: BIZ UD Gothic cut down to the characters it can show.

    python tools/web_font_subset.py          # writes crates/web/assets/BIZUDGothic-web.ttf (needs fonttools: pip install fonttools)

The browser has no system fonts, so the font travels inside the wasm file; the whole font is 4.7 MB. Kept: ASCII and Latin-1,
the kana, the JIS level-1 kanji (the common ones), the symbols the screens use, and every other character written in the
sources of the crates the browser version is built from (web, sch-core, sim-core, i18n) and in the sample circuits it opens.
A character left out shows as an empty box: run this again after adding text with a rare kanji, and commit the new file.

The font is under the SIL Open Font License 1.1 (crates/app/assets/fonts/OFL.txt), which allows a cut-down copy under the
same licence (the font has no Reserved Font Name); the page ships OFL.txt next to it (crates/web/index.html).
"""

import pathlib
import re
import sys

from fontTools import subset

ROOT = pathlib.Path(__file__).resolve().parent.parent
SRC = ROOT / "crates/app/assets/fonts/BIZUDGothic-Regular.ttf"
OUT = ROOT / "crates/web/assets/BIZUDGothic-web.ttf"
CRATES = ["web", "sch-core", "sim-core", "i18n"]


def jis_level1() -> set[str]:
    """The JIS X 0208 level-1 kanji (Shift_JIS 0x889F-0x9872)."""
    out = set()
    for lead in range(0x88, 0x99):
        for trail in list(range(0x40, 0x7F)) + list(range(0x80, 0xFD)):
            code = bytes([lead, trail])
            if not (0x889F <= int.from_bytes(code, "big") <= 0x9872):
                continue
            try:
                out.add(code.decode("shift_jis"))
            except UnicodeDecodeError:
                pass
    return out


def main() -> int:
    chars: set[str] = set()
    chars.update(chr(c) for c in range(0x20, 0x7F))
    chars.update(chr(c) for c in range(0xA0, 0x100))
    chars.update(chr(c) for c in range(0x3000, 0x3100))  # CJK punctuation, hiragana, katakana
    chars.update(chr(c) for c in range(0xFF00, 0xFFF0))  # full-width forms
    chars.update(chr(c) for c in range(0x2190, 0x2200))  # arrows
    chars.update(chr(c) for c in range(0x25A0, 0x2600))  # shapes (▶ ▼ ● □)
    chars.update("µΩ±×÷°·…‥—–‐‘’“”•※√∞≒≠≦≧≤≥∠⊥αβγδεθλμπρστφωΔΣΦ℃⏸")
    chars.update(jis_level1())
    for crate in CRATES:
        for path in (ROOT / "crates" / crate).rglob("*"):
            if path.suffix in (".rs", ".tsv") and "target" not in path.parts:
                chars.update(path.read_text(encoding="utf-8"))
    web_samples = (ROOT / "crates/web/src/samples.rs").read_text(encoding="utf-8")
    for name in re.findall(r'"([\w-]+\.schjson)"', web_samples):
        chars.update((ROOT / "docs/examples" / name).read_text(encoding="utf-8"))
    chars = {c for c in chars if c.isprintable() and ord(c) >= 0x20}

    opts = subset.Options()
    opts.layout_features = ["*"]
    opts.hinting = False
    opts.name_IDs = ["*"]
    opts.notdef_outline = True
    font = subset.load_font(str(SRC), opts)
    sub = subset.Subsetter(opts)
    sub.populate(text="".join(sorted(chars)))
    sub.subset(font)
    OUT.parent.mkdir(parents=True, exist_ok=True)
    subset.save_font(font, str(OUT), opts)
    print(f"{len(chars)} characters, {SRC.stat().st_size / 1e6:.2f} MB -> {OUT.stat().st_size / 1e6:.2f} MB: {OUT.relative_to(ROOT)}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
