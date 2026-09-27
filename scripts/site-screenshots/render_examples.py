"""Fill the example placeholders in site/config.html from components.json, the
theme option tables from theme-options.json, the color tables from
src/colors.rs, and sync every screenshot's width/height attributes on the
site's pages.

A placeholder is `<div class="example" data-example="<component>/<variant>">`
up to the next `<!-- /example -->`. Its contents are rewritten with the
variant's screenshot and the JSON it was rendered from, so the page can never
show a config that differs from the one in the picture.
"""

import html
import json
import re
from pathlib import Path

from PIL import Image

HERE = Path(__file__).resolve().parent
ROOT = HERE.parent.parent
SITE = ROOT / "site"
WIDTH = 66

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


# Components that shape a whole row, shown as a complete config.json so their
# effect on the segments around them is clear.
LAYOUT = {"rows", "separator", "spacers", "padding"}


def segment_name(segment):
    return segment if isinstance(segment, str) else next(iter(segment))


def shown_json(component, variant):
    """What a reader copies: a whole config for layout, otherwise the segment itself."""
    if "config" in variant:
        return variant["config"]
    row = variant["row"]
    if component in LAYOUT:
        return {"rows": [row]}
    segments = [s for side in row.values() for s in side if segment_name(s) == component]
    if len(segments) != 1:
        raise SystemExit(f"{component}/{variant['name']} must use {component} exactly once")
    return segments[0]


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
                "json": shown_json(component, variant),
                "image": f"img/config/{component}-{variant['name']}.png",
            }
    yield from EXTRA.items()


def render(key, example):
    component = key.split("/")[0]
    alt = f"The {component} example: {example['label']}."
    return (
        f'\n  <div class="ex-label">{html.escape(example["label"], quote=False)}</div>'
        f'\n  <figure class="term shot">{image_tag(example["image"], alt)}</figure>'
        f'\n  <pre class="json"><code>{highlight(pretty(example["json"]))}</code></pre>\n'
    )


def theme_table(module, spec):
    rows = "".join(
        f"<tr><td><code>{html.escape(name)}</code></td><td>{kind}</td>"
        f"<td>{html.escape(styles, quote=False)}</td><td>{fallback_html(fallback)}</td></tr>"
        for name, kind, styles, fallback in spec["properties"]
    )
    aliases = spec.get("aliases", [])
    note = ""
    if aliases:
        names = ", ".join(f"<code>modules.{a}</code>" for a in aliases)
        note = f'\n    <p class="note">Also read from {names}, the key this module had before it was renamed.</p>'
    return (
        f'\n    <summary>Theme options <code>modules.{module}</code></summary>'
        '\n    <table class="opts">'
        "<thead><tr><th>Property</th><th>Type</th><th>Styles</th><th>If unset</th></tr></thead>"
        f"<tbody>{rows}</tbody></table>{note}\n  "
    )


def fallback_html(fallback):
    if fallback.startswith("U+") or " " in fallback and not fallback.startswith('"'):
        return html.escape(fallback, quote=False)
    return f"<code>{html.escape(fallback, quote=False)}</code>"


def fill_theme_options(text):
    options = json.loads((HERE / "theme-options.json").read_text())
    options.pop("_comment", None)
    missing = set(options)

    def fill(match):
        module = match[1]
        if module not in options:
            raise SystemExit(f"config.html references unknown theme module {module}")
        missing.discard(module)
        return (
            f'<details class="theme-opts" data-theme="{module}">'
            f"{theme_table(module, options[module])}</details><!-- /theme -->"
        )

    text = re.sub(
        r'<details class="theme-opts" data-theme="([^"]+)">.*?</details><!-- /theme -->',
        fill,
        text,
        flags=re.S,
    )
    if missing:
        print("theme options not placed in config.html:", ", ".join(sorted(missing)))
    return text


# xterm's default palette for the 16 system colors.
SYSTEM_COLORS = [
    "000000", "cd0000", "00cd00", "cdcd00", "0000ee", "cd00cd", "00cdcd", "e5e5e5",
    "7f7f7f", "ff0000", "00ff00", "ffff00", "5c5cff", "ff00ff", "00ffff", "ffffff",
]


def xterm_hex(code):
    if code < 16:
        return SYSTEM_COLORS[code]
    if code < 232:
        steps = [0, 95, 135, 175, 215, 255]
        code -= 16
        return "".join(f"{steps[c]:02x}" for c in (code // 36, code // 6 % 6, code % 6))
    return f"{8 + 10 * (code - 232):02x}" * 3


def ink(code):
    r, g, b = (int(xterm_hex(code)[i:i + 2], 16) for i in (0, 2, 4))
    return "#000" if 0.299 * r + 0.587 * g + 0.114 * b > 128 else "#fff"


def swatch(code, cls="swatch"):
    return f'<span class="{cls}" style="background:#{xterm_hex(code)};color:{ink(code)}">{code}</span>'


def color_codes():
    """The 256-color palette in the layout of `ansi --color-codes`."""

    def block(codes, cols, labels=(), cls=""):
        cells = "".join(swatch(c, "cell") for c in codes)
        grid = f'<span class="palette-grid{cls}" style="--cols:{cols}">{cells}</span>'
        if not labels:
            return grid
        names = "".join(f"<span>{label}</span>" for label in labels)
        return f'<div class="palette-row"><span class="palette-labels">{names}</span>{grid}</div>'

    def cube(band, half):
        start = 16 + 6 * band + 18 * half
        return block([start + 36 * r + c for r in range(6) for c in range(6)], 6)

    parts = [block(range(16), 8, ("Standard:", "Intense:"))]
    parts += [f'<div class="palette-cubes">{cube(b, 0)}{cube(b, 1)}</div>' for b in range(3)]
    parts.append(block(range(232, 256), 12, ("Grays:",), " palette-grays"))
    return '\n    <div class="palette">\n      ' + "\n      ".join(parts) + "\n    </div>\n  "


def color_names():
    """Every name accepted by `Color::from_name`, read from src/colors.rs."""
    source = (ROOT / "src/colors.rs").read_text()
    block = re.search(r"define_colors! \{(.*?)\n\}", source, re.S)[1]
    names = re.findall(r"(\w+) => (\d+)", block)
    items = "".join(
        f'\n      <li>{swatch(int(code))}<code>"{name}"</code></li>' for name, code in names
    )
    return f'\n    <ul class="color-names">{items}\n    </ul>\n  '


def fill_colors(text):
    tables = {"codes": color_codes, "names": color_names}
    return re.sub(
        r'<div class="colors" data-colors="([^"]+)">.*?</div><!-- /colors -->',
        lambda m: f'<div class="colors" data-colors="{m[1]}">{tables[m[1]]()}</div><!-- /colors -->',
        text,
        flags=re.S,
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
        return f'<div class="example" data-example="{key}">{render(key, rendered[key])}</div><!-- /example -->'

    text = re.sub(
        r'<div class="example[^"]*" data-example="([^"]+)">.*?</div><!-- /example -->',
        fill,
        text,
        flags=re.S,
    )
    text = fill_theme_options(text)
    text = fill_colors(text)
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
