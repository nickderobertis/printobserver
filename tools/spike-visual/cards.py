"""SPIKE MOCK (throwaway): image helpers for the screenshots.

  cards.py card <image> <note> <out.png>   the image an agent got, with its note under it
  cards.py pad <a.png> <b.png>             pad both to one size, in place
"""

import sys
import textwrap

from PIL import Image, ImageDraw, ImageFont

WIDTH, IMAGE_HEIGHT, BACKGROUND = 1100, 1980, (24, 24, 24)


def font(size):
    for name in ("DejaVuSans.ttf", "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf"):
        try:
            return ImageFont.truetype(name, size)
        except OSError:
            continue
    return ImageFont.load_default()


def card(image, note, out):
    shown = Image.open(image).convert("RGB")
    scale = min((WIDTH - 40) / shown.width, IMAGE_HEIGHT / shown.height)
    shown = shown.resize((round(shown.width * scale), round(shown.height * scale)))
    lines = textwrap.wrap("image note: " + note, 95)
    canvas = Image.new("RGB", (WIDTH, 20 + IMAGE_HEIGHT + 30 + 24 * 14), BACKGROUND)
    canvas.paste(shown, ((WIDTH - shown.width) // 2, 20))
    draw = ImageDraw.Draw(canvas)
    for number, line in enumerate(lines):
        draw.text((20, 20 + IMAGE_HEIGHT + 30 + number * 24), line, fill=(230, 230, 230), font=font(18))
    canvas.save(out)


def pad(first, second):
    images = [Image.open(path).convert("RGB") for path in (first, second)]
    size = (max(each.width for each in images), max(each.height for each in images))
    for path, image in zip((first, second), images):
        corner = image.getpixel((1, 1))
        canvas = Image.new("RGB", size, corner)
        canvas.paste(image, (0, 0))
        canvas.save(path)


if sys.argv[1] == "card":
    card(*sys.argv[2:5])
else:
    pad(*sys.argv[2:4])
