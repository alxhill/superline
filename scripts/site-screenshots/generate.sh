#!/usr/bin/env bash
# Regenerate the website screenshots in site/img with VHS.
#
# Every scene runs the branch-local release build of superline in fish, inside
# an isolated home with a scene-specific config and fixture directory (see
# scripts/lib/vhs-scene.sh). PR lookups, AI usage and the clock are answered
# from fixtures.json, so nothing depends on what is installed or signed in.
#
#   scripts/site-screenshots/generate.sh [scene...]
#   COMPONENTS="git pr" scripts/site-screenshots/generate.sh components
#
# Needs the patched VHS from tests/terminal/build-vhs.sh (pass it
# with SUPERLINE_E2E_VHS, or it is built into target/vhs-bin on first run),
# plus fish, git, jq, uv, ttyd, ffmpeg, the MesloLGS Nerd Font and Symbols
# Nerd Font Mono 3.4 or newer (for the Claude and Codex glyphs).
set -euo pipefail

repo=$(cd "$(dirname "$0")/../.." && pwd)
here="$repo/scripts/site-screenshots"
out="$repo/site/img"
source "$repo/scripts/lib/vhs-scene.sh"
export FIXTURES="$here/fixtures.json"

scene_prepare
mkdir -p "$out"

# Shorter than the macOS $TMPDIR so tape lines stay readable in logs. Resolved
# (/tmp is a symlink on macOS) so resolve_symlinks still sees the fake home.
work=$(cd "$(mktemp -d /tmp/superline-site.XXXXXX)" && pwd -P)
# The read_only fixture leaves a directory without write permission.
trap 'chmod -R u+w "$work"; rm -rf "$work"' EXIT

# Renders one scene to site/img/<name>.png. Arguments: name, columns, rows,
# config path, workdir (relative to the fake home), then the tape body on
# stdin. PRE is an optional hidden fish command run in the workdir first.
capture() {
  local name=$1
  shift
  scene_screenshot "$out/$name.png" "$work/$name" "$@"
}

# Records one scene to site/img/<name>.gif at the size the site shows it.
# Takes the same arguments as `capture`, plus HIDDEN: optional tape commands
# run off camera just before the recording starts.
record() {
  local name=$1
  shift
  scene_recording "$out/$name.gif" "$work/$name" "$@"
}

source "$here/scenes.sh"

scenes=("$@")
if ((${#scenes[@]} == 0)); then
  scenes=("${ALL_SCENES[@]}")
fi
for scene in "${scenes[@]}"; do
  "scene_$scene"
done


uv run --quiet --with pillow python "$here/render_examples.py"
