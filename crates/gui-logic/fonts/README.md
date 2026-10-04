# The GUI's fonts

The bundled type (`crates/gui-logic/src/fonts.rs`, which the GUI
registers with GPUI's text system at start, `crates/gui/src/main.rs`):
every face of every built-in theme, registered whatever the theme so a
switch never waits on one. A theme names the families it draws in
(`theme::Faces`); `navy`'s overlay keeps the system UI face it has always
drawn in (its render guard, `crates/gui/SHOTS.md`), `onyx`'s overlay draws
in Saira. Font files are assets, not dependencies; every family is under
the SIL Open Font License 1.1, whose text sits beside them, and none
declares a Reserved Font Name.

| file | family | weight | theme, use |
|------|--------|--------|-----|
| `BarlowSemiCondensedTabular-Regular.ttf`  | Barlow Semi Condensed Tabular | 400 | `navy`: names, numbers, column heads |
| `BarlowSemiCondensedTabular-Medium.ttf`   | Barlow Semi Condensed Tabular | 500 | `navy`: stat values, the amount column |
| `BarlowSemiCondensedTabular-SemiBold.ttf` | Barlow Semi Condensed Tabular | 600 | `navy`: headings |
| `Marcellus-Regular.ttf`                   | Marcellus                     | 400 | `navy`: encounter titles, the wordmark |
| `SairaTabular-Regular.ttf`   | Saira Tabular  | 400 | `onyx`: names, numbers, column heads, the overlay |
| `SairaTabular-Medium.ttf`    | Saira Tabular  | 500 | `onyx`: stat values, the amount column |
| `SairaTabular-SemiBold.ttf`  | Saira Tabular  | 600 | `onyx`: headings |
| `Michroma-Regular.ttf`                    | Michroma                      | 400 | `onyx`: encounter titles, the wordmark, Home's places |
| `OFL-Barlow.txt`, `OFL-Marcellus.txt`, `OFL-Saira.txt`, `OFL-Michroma.txt` | | | the licenses |

`frost` draws in `navy`'s faces.

## Where they came from

From [google/fonts](https://github.com/google/fonts), raw files:

- `ofl/barlowsemicondensed/BarlowSemiCondensed-{Regular,Medium,SemiBold}.ttf`
  and `OFL.txt`, at commit `3218ae2acc5bce7ccd97c477965b5b4094241fb0`
  (sha256 of the originals: Regular `e8c7242f…d1153`, Medium
  `4998b693…f8b48`, SemiBold `bd299f4b…a1424`).
- `ofl/marcellus/Marcellus-Regular.ttf` and `OFL.txt`, at commit
  `9712ecf0710335ef60aa998f5ee3b3c7e40f5e7c`, unmodified.
- `ofl/saira/Saira[wdth,wght].ttf` (the variable font: its static cuts
  carry no `tnum`) and `OFL.txt`, and
  `ofl/michroma/Michroma-Regular.ttf` and `OFL.txt`, at commit
  `8b0a1d0f5983c89bc2b93f1b5fb55f9e252744b5` (sha256 of the originals:
  Saira `9d050fc5…31dde`, Michroma `b6230116…98f87`); Michroma unmodified.

## Barlow's and Saira's figures are baked tabular

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

`theme::tests::every_theme_s_faces_are_bundled_and_tabular` (crates/gui)
measures `0`–`9` of every built-in theme's window face, at 400, 500 and
600, through the renderer's own text system, and fails if they ever stop
sharing one advance.

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

### Saira

Saira's variable font is instanced at `wdth` 80 — between its Condensed
(75) and Semi Condensed (87.5) cuts, the width at which its text sets the
measure of Barlow Semi Condensed within a few per cent (Semi Condensed ran
5–12 % wider and cut the pull rail's names), so `onyx` keeps every column
and budget `navy` was laid out for — and at `wght` 400, 500 and 600 — fonttools' `varLib.instancer`, which applies its `rvrn` feature
variations at that location — and then baked exactly as Barlow is: its
`tnum` maps `zero` … `nine` to `zero.tf` … `nine.tf` (509 / 515 / 516
units, one advance per weight), and only U+0030–U+0039 are remapped. The
family is renamed **Saira Tabular** (PostScript
`SairaTabular-*`, typographic family and subfamily set, the
legacy family carrying the weight for 500 and 600), `usWeightClass` is set
to the weight and `usWidthClass` to 3 (condensed, the nearest class), and the `STAT`
table (which describes the variable axes no longer there) and any `DSIG`
are dropped. Deterministic with `recalcTimestamp=False`:

```sh
cat > bake_saira.py <<'PY'
import sys
from fontTools.ttLib import TTFont
from fontTools.varLib.instancer import instantiateVariableFont

FAMILY = "Saira Tabular"
PS = "SairaTabular"
src, weight, style, dst = sys.argv[1], int(sys.argv[2]), sys.argv[3], sys.argv[4]
f = TTFont(src, recalcTimestamp=False)
f = instantiateVariableFont(f, {"wght": weight, "wdth": 80})
gsub = f["GSUB"].table
tnum = set()
for fr in gsub.FeatureList.FeatureRecord:
    if fr.FeatureTag == "tnum":
        tnum.update(fr.Feature.LookupListIndex)
subst = {}
for li in tnum:
    for st in gsub.LookupList.Lookup[li].SubTable:
        subst.update(getattr(st, "mapping", {}))
for table in f["cmap"].tables:
    for cp in range(0x30, 0x3A):  # the digits, and only the digits
        glyph = table.cmap.get(cp)
        if glyph in subst:
            table.cmap[cp] = subst[glyph]
name = f["name"]
legacy = FAMILY if style == "Regular" else f"{FAMILY} {style}"
full = f"{FAMILY} {style}" if style != "Regular" else f"{FAMILY} Regular"
for rec in list(name.names):
    if rec.nameID in (25,) or rec.nameID >= 256:
        name.removeNames(nameID=rec.nameID)
for nid, s in ((1, legacy), (2, "Regular"), (3, f"{PS}-{style}"), (4, full),
               (6, f"{PS}-{style}"), (16, FAMILY), (17, style)):
    name.setName(s, nid, 3, 1, 0x409)
    name.setName(s, nid, 1, 0, 0)
f["OS/2"].usWeightClass = weight
f["OS/2"].usWidthClass = 3
for t in ("DSIG", "STAT"):
    if t in f:
        del f[t]
f.save(dst)
PY
curl -sfL -o Saira.ttf "https://raw.githubusercontent.com/google/fonts/8b0a1d0f5983c89bc2b93f1b5fb55f9e252744b5/ofl/saira/Saira%5Bwdth,wght%5D.ttf"
for p in 400:Regular 500:Medium 600:SemiBold; do
  nix shell --impure --expr "$PY" -c python3 bake_saira.py Saira.ttf \
    "${p%%:*}" "${p#*:}" "SairaTabular-${p#*:}.ttf"
done
```

(`$PY` is the fonttools interpreter of the Barlow recipe above.)
