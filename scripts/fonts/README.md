# Recording fonts

The fonts VHS renders with in `scripts/demo/record.sh` and
`scripts/site-screenshots/generate.sh`, from the
[Nerd Fonts v3.5.1 release](https://github.com/ryanoasis/nerd-fonts/releases/tag/v3.5.1):

- `MesloLGSNerdFont-Regular.ttf` from `patched-fonts/Meslo/S` (Apache 2.0,
  `LICENSE-Meslo`; the icon sets carry their own licenses, listed in the
  release's Meslo README).
- `SymbolsNerdFontMono-Regular.ttf` from `NerdFontsSymbolsOnly.zip` (MIT,
  `LICENSE-SymbolsNerdFont`), the fallback for glyphs an older installed
  Meslo lacks, such as the Claude and Codex logos.

`scene_prepare` in `scripts/lib/vhs-scene.sh` copies each into the user font
directory unless a file of that name is already there.
