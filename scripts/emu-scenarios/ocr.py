"""OCR the on-screen rendering of one UI node (what the user SEES, after bidi
reordering), to compare it with the node's logical text from the UI tree.

argv: screenshot.png ui.xml mode text
Finds the first node whose text equals / starts with / regex-matches TEXT (mode
eq|pre|re), crops its bounds from the screenshot (3x upscaled, grayscale) and
prints "<logical text>\t<ocr text>" (OCR whitespace-collapsed). Exit 1 when the
node is absent, 3 when tesseract has no Latin-script language data (eng, else
afr/deu/fra: same alphabet + digits, which is all this check reads).
"""
import re
import shutil
import subprocess
import sys
import tempfile
import xml.etree.ElementTree as ET

from PIL import Image

png, xmlp, mode, want = sys.argv[1:5]
if not shutil.which("tesseract"):
    sys.exit(3)
langs = subprocess.run(["tesseract", "--list-langs"], capture_output=True, text=True).stdout.split()
lang = next((l for l in ("eng", "afr", "deu", "fra") if l in langs), None)
if lang is None:
    sys.exit(3)
raw = open(xmlp, errors="replace").read()
i, j = raw.find("<?xml"), raw.rfind("</hierarchy>")
root = ET.fromstring(raw[i:j + 12])
hit = None
for n in root.iter("node"):
    v = n.get("text", "")
    if v and ((mode == "eq" and v == want) or (mode == "pre" and v.startswith(want))
              or (mode == "re" and re.search(want, v))):
        hit = n
        break
if hit is None:
    sys.exit(1)
x1, y1, x2, y2 = map(int, re.findall(r"\d+", hit.get("bounds")))
im = Image.open(png).convert("L").crop((x1, y1, x2, y2))
im = im.resize((im.width * 3, im.height * 3), Image.LANCZOS)
with tempfile.NamedTemporaryFile(suffix=".png", dir=sys.argv[5] if len(sys.argv) > 5 else None) as f:
    im.save(f.name)
    out = subprocess.run(["tesseract", f.name, "-", "--psm", "6", "-l", lang],
                         capture_output=True, text=True).stdout
print(f"{hit.get('text')}\t{' '.join(out.split())}")
