"""Fill the example placeholders in site/config.html from components.json and
sync every screenshot's width/height attributes on the site's pages.

A placeholder is `<div class="example" data-example="<component>/<variant>">`
up to the next `<!-- /example -->`. Its contents are rewritten with the
variant's screenshot and the JSON it was rendered from, so the page can never
show a config that differs from the one in the picture. Examples whose JSON
or screenshot is too wide for a half-width card span the whole row, and JSON
too long to sit beside its screenshot goes underneath it.
"""

import html
import json
import re
from pathlib import Path

from PIL import Image

HERE = Path(__file__).resolve().parent
SITE = HERE.parent.parent / "site"
WIDTH = 66
# Longest line that fits a half-width example card.
NARROW = 44
# Room in a full-width card for a screenshot and JSON side by side, and the
# width of one character of 13px JetBrains Mono, both in CSS pixels.
ROW_WIDTH = 740
CHAR_WIDTH = 8

# Examples that are not a single prompt row, keyed like the manifest's.
EXTRA = {
    "theme/ocean": {
        "label": "theme.json",
        "json": json.loads((HERE / "configs/ocean-theme.json").read_text()),
        "image": "img/theme-custom.png",
    },
}


def compact(value):
    if isinstance(value, dict):
        if not value:
            return "{}"
        return "{ " + ", ".join(f"{json.dumps(k)}: {compact(v)}" for k, v in value.items()) + " }"
    if isinstance(value, list):
        return "[" + ", ".join(compact(v) for v in value) + "]"
    return json.dumps(value, ensure_ascii=False)


def pretty(value, level=0, column=0):
    """JSON in the README's style: anything that fits on a line stays on one."""
    flat = compact(value)
    if column + len(flat) <= WIDTH or not isinstance(value, (dict, list)) or not value:
        return flat
    pad = "  " * (level + 1)
    if isinstance(value, dict):
        items = [
            f"{pad}{json.dumps(k)}: {pretty(v, level + 1, len(pad) + len(json.dumps(k)) + 2)}"
            for k, v in value.items()
        ]
        brackets = "{}"
    else:
        items = [f"{pad}{pretty(v, level + 1, len(pad))}" for v in value]
        brackets = "[]"
    return brackets[0] + "\n" + ",\n".join(items) + "\n" + "  " * level + brackets[1]


TOKEN = re.compile(r'("(?:[^"\\]|\\.)*")(\s*:)?|(-?\d+(?:\.\d+)?)|\b(true|false|null)\b')


def highlight(source):
    out, last = [], 0
    for match in TOKEN.finditer(source):
        out.append(html.escape(source[last:match.start()], quote=False))
        string, colon, number, literal = match.groups()
        if string and colon:
            out.append(f'<span class="k">{html.escape(string, quote=False)}</span>{colon}')
        elif string:
            out.append(f'<span class="s">{html.escape(string, quote=False)}</span>')
        elif number:
            out.append(f'<span class="n">{number}</span>')
        else:
            out.append(f'<span class="n">{literal}</span>')
        last = match.end()
    out.append(html.escape(source[last:], quote=False))
    return "".join(out)


def shown_json(variant):
    """What a reader copies: a whole config, a single segment, or a row."""
    if "config" in variant:
        return variant["config"]
    row = variant["row"]
    if list(row) == ["left"] and len(row["left"]) == 1:
        return row["left"][0]
    return row


def image_tag(src, alt):
    width, height = Image.open(SITE / src).size
    return (
        f'<img src="{src}" width="{width // 2}" height="{height // 2}" '
        f'loading="lazy" alt="{html.escape(alt, quote=True)}">'
    )


def examples():
    manifest = json.loads((HERE / "components.json").read_text())
    for component, spec in manifest.items():
        for variant in spec["variants"]:
            yield f"{component}/{variant['name']}", {
                "label": variant["label"],
                "json": shown_json(variant),
                "image": f"img/config/{component}-{variant['name']}.png",
            }
    yield from EXTRA.items()


def render(key, example):
    component = key.split("/")[0]
    alt = f"The {component} example: {example['label']}."
    source = pretty(example["json"])
    return (
        f'\n  <div class="ex-label">{html.escape(example["label"], quote=False)}</div>'
        f'\n  <figure class="term shot">{image_tag(example["image"], alt)}</figure>'
        f'\n  <pre class="json"><code>{highlight(source)}</code></pre>\n'
    )


def main():
    rendered = dict(examples())
    config_page = SITE / "config.html"
    text = config_page.read_text()
    missing = set(rendered)

    def fill(match):
        key = match[1]
        if key not in rendered:
            raise SystemExit(f"config.html references unknown example {key}")
        missing.discard(key)
        source = pretty(rendered[key]["json"])
        width = Image.open(SITE / rendered[key]["image"]).size[0] // 2
        longest = max(map(len, source.splitlines()))
        classes = "example"
        if longest > NARROW or width > 380:
            classes += " wide"
        if longest > NARROW or width + longest * CHAR_WIDTH > ROW_WIDTH:
            classes += " stacked"
        return f'<div class="{classes}" data-example="{key}">{render(key, rendered[key])}</div><!-- /example -->'

    text = re.sub(
        r'<div class="example[^"]*" data-example="([^"]+)">.*?</div><!-- /example -->',
        fill,
        text,
        flags=re.S,
    )
    config_page.write_text(text)
    if missing:
        print("examples not placed in config.html:", ", ".join(sorted(missing)))

    # Screenshots are 2x density, so each is laid out at half its size.
    def size(match):
        width, height = Image.open(SITE / match[1]).size
        return f'src="{match[1]}" width="{width // 2}" height="{height // 2}"'

    for page in SITE.glob("*.html"):
        page.write_text(
            re.sub(r'src="(img/[^"]+\.png)" width="\d+" height="\d+"', size, page.read_text())
        )


if __name__ == "__main__":
    main()
