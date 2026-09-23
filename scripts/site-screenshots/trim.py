"""Crop the unused terminal rows (and columns) off a VHS screenshot, keeping
the tape's padding as a uniform margin, open a gap between terminal rows so
stacked prompts do not run together, and reduce it to a 256-colour palette,
which a flat terminal render survives without visible loss."""

import sys

from PIL import Image, ImageChops

MARGIN = 28
ROW_GAP = 10
# VHS scales its screenshots, so the row pitch and the first row's offset vary
# between scenes and are found per image. Most scenes have rows every 41px from
# y=32, which wins when the image has too few edges to tell.
DEFAULT = (41, 32)
PITCHES = [p / 20 for p in range(39 * 20, 43 * 20)]
# Only long horizontal edges, such as the top and bottom of a segment, count
# towards a row boundary; glyph strokes are much shorter.
EDGE = 5
EDGE_RUN = 48
# Scaling and the video encoder blur each row's edge a pixel or two into its
# neighbour, which would show up as a thin line beside every gap. Those pixel
# rows are repainted from the row's own content just inside them.
SMEAR = 2


def edge_profile(image):
    gray = image.convert("L")
    width, height = gray.size
    data = gray.tobytes()
    profile = [0] * height
    for y in range(1, height):
        above = data[(y - 1) * width : y * width]
        row = data[y * width : (y + 1) * width]
        run = total = 0
        for a, b in zip(above, row):
            if abs(a - b) > EDGE:
                run += 1
            else:
                if run >= EDGE_RUN:
                    total += run
                run = 0
        profile[y] = total + (run if run >= EDGE_RUN else 0)
    return profile


def row_cuts(image, top, bottom):
    """Pixel rows inside (top, bottom) where one terminal row meets the next."""
    profile = edge_profile(image)
    candidates = [DEFAULT] + [(p, s / 4) for p in PITCHES for s in range(int(p * 4))]
    best = (-1, None, None)
    for pitch, offset in candidates:
        lines = range(int((top - offset) // pitch), int((bottom - offset) // pitch) + 2)
        ys = (round(offset + k * pitch) for k in lines)
        score = sum(profile[y] for y in ys if 0 <= y < len(profile))
        if score > best[0]:
            best = (score, pitch, offset)
    _, pitch, offset = best
    ys = (round(offset + k * pitch) for k in range(int(bottom // pitch) + 2))
    return [y for y in ys if top + pitch / 2 < y < bottom - pitch / 2]


for path in sys.argv[1:]:
    image = Image.open(path).convert("RGB")
    fill = image.getpixel((1, 1))
    background = Image.new("RGB", image.size, fill)
    # Ignore near-background noise such as xterm.js's faint row highlights.
    diff = ImageChops.difference(image, background).convert("L")
    box = diff.point(lambda value: 255 if value > 24 else 0).getbbox()
    if box is None:
        continue
    left, top, right, bottom = box
    left, right = max(left - MARGIN, 0), min(right + MARGIN, image.width)
    cuts = row_cuts(image, top, bottom)
    for cut in cuts:
        above = image.crop((left, cut - SMEAR - 1, right, cut - SMEAR))
        below = image.crop((left, cut + SMEAR, right, cut + SMEAR + 1))
        for i in range(SMEAR):
            image.paste(above, (left, cut - 1 - i))
            image.paste(below, (left, cut + i))
    edges = [max(top - MARGIN, 0), *cuts, min(bottom + MARGIN, image.height)]
    strips = [image.crop((left, a, right, b)) for a, b in zip(edges, edges[1:])]
    height = sum(strip.height for strip in strips) + ROW_GAP * len(cuts)
    out = Image.new("RGB", (right - left, height), fill)
    y = 0
    for strip in strips:
        out.paste(strip, (0, y))
        y += strip.height + ROW_GAP
    out.quantize(256, dither=Image.Dither.NONE).save(path, optimize=True)
