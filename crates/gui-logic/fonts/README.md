# The window's fonts

The regular window's type (`crates/gui-logic/src/fonts.rs`, which the GUI
registers with GPUI's text system at start, `crates/gui/src/main.rs`). The
overlay draws in none of these: it keeps the system UI face it has always
drawn in (its render guard, `crates/gui/SHOTS.md`). Font files are assets,
not dependencies; both families are under the SIL Open Font License 1.1,
whose text sits beside them.

| file | family | weight | use |
|------|--------|--------|-----|
| `BarlowSemiCondensedTabular-Regular.ttf`  | Barlow Semi Condensed Tabular | 400 | the window's default font: names, numbers, column heads |
| `BarlowSemiCondensedTabular-Medium.ttf`   | Barlow Semi Condensed Tabular | 500 | stat values, the amount column |
| `BarlowSemiCondensedTabular-SemiBold.ttf` | Barlow Semi Condensed Tabular | 600 | headings |
| `Marcellus-Regular.ttf`                   | Marcellus                     | 400 | encounter titles |
| `OFL-Barlow.txt`, `OFL-Marcellus.txt`     | | | the licenses |

## Where they came from

From [google/fonts](https://github.com/google/fonts), raw files:

- `ofl/barlowsemicondensed/BarlowSemiCondensed-{Regular,Medium,SemiBold}.ttf`
  and `OFL.txt`, at commit `3218ae2acc5bce7ccd97c477965b5b4094241fb0`
  (sha256 of the originals: Regular `e8c7242f…d1153`, Medium
  `4998b693…f8b48`, SemiBold `bd299f4b…a1424`).
- `ofl/marcellus/Marcellus-Regular.ttf` and `OFL.txt`, at commit
  `9712ecf0710335ef60aa998f5ee3b3c7e40f5e7c`, unmodified.

## Barlow's figures are baked tabular

Barlow's default digits are proportional (a `1` is 303 units wide, a `0`
504), so a column of numbers would not line up, and iced (the first GUI
to load them) could not switch an OpenType feature on. Its `tnum` feature
holds the tabular digits
(`zero.tf` … `nine.tf`, one advance per weight: 481 / 495 / 507), so the
three files here have those glyphs mapped in place of the default digits
— U+0030–U+0039 and nothing else: the script remaps only those ten code
points, and Barlow's `tnum` holds exactly those ten pairs anyway (checked
on all three weights: no other code point maps to a `tnum` target).
Nothing else in the fonts changed, except:

- the family is renamed **Barlow Semi Condensed Tabular** (and the
  PostScript names `BarlowSemiCondensedTabular-*`), so a proportional
  Barlow Semi Condensed installed on the system can never be the face
  cosmic-text picks for `theme::UI` — the OFL asks a modified font not to
  pass as the original anyway;
- the `DSIG` table is dropped: the edit breaks the signature.

`theme::tests::the_window_digits_are_tabular` measures `0`–`9` through the
renderer's own text system and fails if they ever stop sharing one advance.

Reproduce (fonttools 4.63; `recalcTimestamp=False` makes it byte-for-byte
deterministic):

```sh
cat > bake.py <<'EOF'
import sys
from fontTools.ttLib import TTFont

src, dst = sys.argv[1], sys.argv[2]
f = TTFont(src, recalcTimestamp=False)
gsub = f["GSUB"].table
tnum = set()
for fr in gsub.FeatureList.FeatureRecord:
    if fr.FeatureTag == "tnum":
        tnum.update(fr.Feature.LookupListIndex)
subst = {}
for li in tnum:
    for st in gsub.LookupList.Lookup[li].SubTable:
        subst.update(st.mapping)
for table in f["cmap"].tables:
    for cp in range(0x30, 0x3A):  # the digits, and only the digits
        glyph = table.cmap.get(cp)
        if glyph in subst:
            table.cmap[cp] = subst[glyph]
for rec in f["name"].names:
    s = rec.toUnicode()
    s = s.replace("Barlow Semi Condensed", "Barlow Semi Condensed Tabular")
    s = s.replace("BarlowSemiCondensed", "BarlowSemiCondensedTabular")
    rec.string = s
if "DSIG" in f:
    del f["DSIG"]
f.save(dst)
EOF
PY='(builtins.getFlake "nixpkgs").legacyPackages.x86_64-linux.python3.withPackages (p: [p.fonttools])'
for w in Regular Medium SemiBold; do
  curl -sfLO "https://raw.githubusercontent.com/google/fonts/3218ae2acc5bce7ccd97c477965b5b4094241fb0/ofl/barlowsemicondensed/BarlowSemiCondensed-$w.ttf"
  nix shell --impure --expr "$PY" -c python3 bake.py \
    "BarlowSemiCondensed-$w.ttf" "BarlowSemiCondensedTabular-$w.ttf"
done
```
