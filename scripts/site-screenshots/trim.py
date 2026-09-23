"""Crop the unused terminal rows (and columns) off a VHS screenshot, keeping
the tape's padding as a uniform margin, and reduce it to a 256-colour
palette, which a flat terminal render survives without visible loss."""

import sys

from PIL import Image, ImageChops

MARGIN = 28

for path in sys.argv[1:]:
    image = Image.open(path).convert("RGB")
    background = Image.new("RGB", image.size, image.getpixel((1, 1)))
    # Ignore near-background noise such as xterm.js's faint row highlights.
    diff = ImageChops.difference(image, background).convert("L")
    box = diff.point(lambda value: 255 if value > 24 else 0).getbbox()
    if box is None:
        continue
    left, top, right, bottom = box
    image.crop(
        (
            max(left - MARGIN, 0),
            max(top - MARGIN, 0),
            min(right + MARGIN, image.width),
            min(bottom + MARGIN, image.height),
        )
    ).quantize(256, dither=Image.Dither.NONE).save(path, optimize=True)
