"""Render a roguemap cell dump to PNG using the Unscii 16 font."""
import sys
from PIL import Image, ImageDraw, ImageFont

src, dst = sys.argv[1], sys.argv[2]
CW, CH = 8, 16
font = ImageFont.truetype("/usr/share/fonts/OTF/unscii-16-full.otf", 16)
with open(src) as f:
    w, h = map(int, f.readline().split())
    cells = [f.readline().split() for _ in range(w * h)]
img = Image.new("RGB", (w * CW, h * CH), (0, 0, 0))
d = ImageDraw.Draw(img)
for i, c in enumerate(cells):
    cp, fr, fg, fb, br, bg, bb = map(int, c)
    x, y = (i % w) * CW, (i // w) * CH
    d.rectangle([x, y, x + CW - 1, y + CH - 1], fill=(br, bg, bb))
    if cp != 32:
        d.text((x, y), chr(cp), font=font, fill=(fr, fg, fb))
img.save(dst)
