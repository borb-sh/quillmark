"""Writes the Typst backend's fallback faces: Google Fonts' variable Roboto,
upright and italic, keeping the weight axis and pinning width to 100. The
TrueType hinting goes, which PDF output and Typst's rasterizer never execute.

    uv run --with fonttools scripts/fallback-fonts.py
"""

import io
import json
import pathlib
import urllib.request

from fontTools import subset
from fontTools.ttLib import TTFont
from fontTools.varLib import instancer

FACES = {
    "Roboto.ttf": "Roboto-VariableFont_wdth,wght.ttf",
    "Roboto-Italic.ttf": "Roboto-Italic-VariableFont_wdth,wght.ttf",
}
OUT = pathlib.Path(__file__).resolve().parent.parent / "crates/backends/typst/src/fonts"


def fetch(url):
    with urllib.request.urlopen(url) as response:
        return response.read()


def load(data):
    return TTFont(io.BytesIO(data), lazy=False, recalcTimestamp=False)


def save(font):
    out = io.BytesIO()
    font.save(out)
    return out.getvalue()


def trim(data):
    # Reloaded between steps: subsetting the instancer's own font object fails.
    font = load(save(instancer.instantiateVariableFont(load(data), {"wdth": 100})))
    options = subset.Options(
        hinting=False,
        layout_features=["*"],
        name_IDs=["*"],
        name_languages=["*"],
        notdef_outline=True,
        legacy_kern=True,
    )
    subsetter = subset.Subsetter(options)
    subsetter.populate(glyphs=font.getGlyphOrder())
    subsetter.subset(font)
    return save(font)


text = fetch("https://fonts.google.com/download/list?family=Roboto").decode()
manifest = json.loads(text[text.index("{") :])["manifest"]

for file in manifest["files"]:
    if file["filename"] == "OFL.txt":
        (OUT / "OFL.txt").write_text(file["contents"].replace("\r\n", "\n"))

urls = {ref["filename"]: ref["url"] for ref in manifest["fileRefs"]}
for name in OUT.glob("*.ttf"):
    name.unlink()
for name, source in FACES.items():
    (OUT / name).write_bytes(trim(fetch(urls[source])))
