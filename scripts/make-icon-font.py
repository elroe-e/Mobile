#!/usr/bin/env python3
"""Makes app/assets/fonts/material-symbols-loss.woff2: Material Symbols Rounded (Apache-2.0),
fixed at weight 400, grade 0 and 24 px optical size (the fill axis stays), cut down to the icons in
app/src/ui/icons.rs. Needs fonttools and brotli (pip install fonttools brotli).

    python3 scripts/make-icon-font.py
"""
import io, re, sys, urllib.request
from pathlib import Path
from fontTools.ttLib import TTFont
from fontTools.varLib import instancer
from fontTools import subset

ROOT = Path(__file__).resolve().parent.parent
SRC = "https://cdn.jsdelivr.net/npm/material-symbols@0.27.0/material-symbols-rounded.woff2"
OUT = ROOT / "app/assets/fonts/material-symbols-loss.woff2"

names = sorted(set(re.findall(r'=> "([a-z0-9_]+)",', (ROOT / "app/src/ui/icons.rs").read_text())))
data = urllib.request.urlopen(urllib.request.Request(SRC, headers={"User-Agent": "Loss-build"})).read()
font = TTFont(io.BytesIO(data))
font = instancer.instantiateVariableFont(font, {"wght": 400, "GRAD": 0, "opsz": 24})
cmap = font.getBestCmap()
ligatures = {}
for lookup in font["GSUB"].table.LookupList.Lookup:
    for st in lookup.SubTable:
        t = st.ExtSubTable if lookup.LookupType == 7 else st
        for first, lst in getattr(t, "ligatures", {}).items():
            for lig in lst:
                ligatures[tuple([first] + lig.Component)] = lig.LigGlyph
keep, missing = set(), []
for n in names:
    g = ligatures.get(tuple(cmap[ord(c)] for c in n))
    (keep.add(g) if g else missing.append(n))
if missing:
    sys.exit(f"not in Material Symbols: {missing}")
buf = io.BytesIO()
font.flavor = None
font.save(buf)
opts = subset.Options()
opts.layout_features = ["liga", "rlig", "calt"]
opts.layout_closure = False  # keep only our ligatures, not every icon spelled with the same letters
opts.flavor = "woff2"
f = subset.load_font(io.BytesIO(buf.getvalue()), opts)
s = subset.Subsetter(opts)
s.populate(glyphs=sorted(keep), text="abcdefghijklmnopqrstuvwxyz_0123456789")
s.subset(f)
subset.save_font(f, str(OUT), opts)
print(f"{OUT.relative_to(ROOT)}: {len(keep)} icons, {OUT.stat().st_size} bytes")
