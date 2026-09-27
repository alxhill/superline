"""Regenerates src/editor/nerd_glyphs.deflate, the icon list behind the
`superline config` Theme page's Nerd Font browser.

    uv run scripts/nerd-glyphs/generate.py [path/to/glyphnames.json]

Without an argument it downloads glyphnames.json from the nerd-fonts repo. The
output is raw deflate over one `hexcode name` line per glyph, sorted by name.
"""

import json
import sys
import urllib.request
import zlib
from pathlib import Path

SOURCE = "https://raw.githubusercontent.com/ryanoasis/nerd-fonts/master/glyphnames.json"
OUTPUT = Path(__file__).resolve().parents[2] / "src" / "editor" / "nerd_glyphs.deflate"


def main() -> None:
    if len(sys.argv) > 1:
        glyphs = json.loads(Path(sys.argv[1]).read_text())
    else:
        with urllib.request.urlopen(SOURCE) as response:
            glyphs = json.load(response)

    version = glyphs.get("METADATA", {}).get("version", "unknown")
    lines = [
        f"{glyphs[name]['code']} {name}"
        for name in sorted(glyphs)
        if name != "METADATA"
    ]
    text = f"# nerd-fonts {version}\n" + "\n".join(lines) + "\n"

    compressor = zlib.compressobj(9, zlib.DEFLATED, -15)
    data = compressor.compress(text.encode()) + compressor.flush()
    OUTPUT.write_bytes(data)
    print(f"wrote {len(lines)} glyphs from nerd-fonts {version} to {OUTPUT} ({len(data)} bytes)")


if __name__ == "__main__":
    main()
